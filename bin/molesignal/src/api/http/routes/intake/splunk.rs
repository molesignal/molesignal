// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Splunk HTTP Event Collector (HEC) compatible intake, for senders that speak only HEC: Tencent
//! Cloud CLS shipping to Splunk, Vector's and Fluent Bit's Splunk outputs, SIEM forwarders.
//!
//! - `POST /api/v1/services/collector/event` (and `/services/collector`): a stream of JSON objects,
//!   one after another, each `{"time", "host", "source", "sourcetype", "index", "event", "fields"}`.
//! - `POST /api/v1/services/collector/raw`: text, one log event per line; `host`, `source`,
//!   `sourcetype` and `index` come from the query.
//! - `GET /api/v1/services/collector/health`: answers without a token, as Splunk's does.
//!
//! A sender authenticates as with Splunk, `Authorization: Splunk <token>`, with an API Token that
//! may write logs. The `index` names the stream, else the `index` of the query, else the
//! `stream-name` header, else `default`. Indexer acknowledgement is not offered: a sender that
//! asks for it keeps waiting for an ack ID. See
//! <https://docs.splunk.com/Documentation/Splunk/latest/Data/FormateventsforHTTPEventCollector>.

use axum::{
    Extension, Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, RawQuery, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use domain::{
    iam::{IamContext, permission},
    intake::RawEvent,
};
use kernel::{Result, time::TimestampMicros};
use serde_json::{Map, Value, json};

use super::{
    MAX_BODY_BYTES,
    compat::{ByStream, Refused, write_streams},
    decoded, gzipped, stream_name,
};
use crate::api::AppState;

pub fn routes() -> Router<AppState> {
    let limit = || DefaultBodyLimit::max(MAX_BODY_BYTES);

    Router::new()
        .route("/services/collector", post(event).layer(limit()))
        .route("/services/collector/event", post(event).layer(limit()))
        .route("/services/collector/event/1.0", post(event).layer(limit()))
        .route("/services/collector/raw", post(raw).layer(limit()))
        .route("/services/collector/raw/1.0", post(raw).layer(limit()))
        .route("/services/collector/health", get(health))
        .route("/services/collector/health/1.0", get(health))
}

/// What a request's query says about its events: their metadata when the events do not.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Defaults {
    index: Option<String>,
    host: Option<String>,
    source: Option<String>,
    sourcetype: Option<String>,
}

impl Defaults {
    fn of(raw_query: Option<&str>) -> Self {
        let mut defaults = Self::default();

        for (key, value) in url::form_urlencoded::parse(raw_query.unwrap_or_default().as_bytes()) {
            let value = Some(value.trim().to_string()).filter(|value| !value.is_empty());

            match key.as_ref() {
                "index" => defaults.index = value,
                "host" => defaults.host = value,
                "source" => defaults.source = value,
                "sourcetype" => defaults.sourcetype = value,
                _ => {}
            }
        }

        defaults
    }

    /// Adds the metadata to `fields` where the event has none of its own.
    fn stamp(&self, fields: &mut Map<String, Value>) {
        for (key, value) in [
            ("host", &self.host),
            ("source", &self.source),
            ("sourcetype", &self.sourcetype),
        ] {
            if let Some(value) = value {
                fields
                    .entry(key)
                    .or_insert_with(|| Value::String(value.clone()));
            }
        }
    }
}

/// A request HEC refuses, answered the way HEC answers.
#[derive(Debug, PartialEq)]
struct Refusal {
    code: u16,
    text: String,
    /// The position of the event that could not be read.
    event: Option<usize>,
}

impl IntoResponse for Refusal {
    fn into_response(self) -> Response {
        let mut body = json!({ "text": self.text, "code": self.code });

        if let Some(event) = self.event {
            body["invalid-event-number"] = json!(event);
        }

        (StatusCode::BAD_REQUEST, Json(body)).into_response()
    }
}

fn success() -> Response {
    Json(json!({ "text": "Success", "code": 0 })).into_response()
}

/// The events of an `/event` body, by the stream each goes to, each with its position in the body.
fn parse_events(
    body: &[u8],
    defaults: &Defaults,
    fallback_stream: &str,
) -> std::result::Result<ByStream, Refusal> {
    let mut by_stream = ByStream::new();
    let values = serde_json::Deserializer::from_slice(body).into_iter::<Value>();

    for (position, value) in values.enumerate() {
        let Ok(Value::Object(mut object)) = value else {
            return Err(Refusal {
                code: 6,
                text: "Invalid data format".into(),
                event: Some(position),
            });
        };

        let Some(payload) = object.remove("event") else {
            return Err(Refusal {
                code: 12,
                text: "Event field is required".into(),
                event: Some(position),
            });
        };

        let mut fields = match payload {
            Value::Object(fields) => fields,
            Value::String(text) if text.is_empty() => {
                return Err(Refusal {
                    code: 13,
                    text: "Event field cannot be blank".into(),
                    event: Some(position),
                });
            }
            Value::String(text) => Map::from_iter([("message".into(), Value::String(text))]),
            other => Map::from_iter([("message".into(), Value::String(other.to_string()))]),
        };

        if let Some(Value::Object(indexed)) = object.remove("fields") {
            for (key, value) in indexed {
                fields.entry(key).or_insert(value);
            }
        }

        for key in ["host", "source", "sourcetype"] {
            if let Some(value) = object.remove(key).filter(|value| !value.is_null()) {
                fields.entry(key).or_insert(value);
            }
        }

        defaults.stamp(&mut fields);

        let timestamp = object
            .get("time")
            .and_then(epoch_seconds)
            .unwrap_or_else(TimestampMicros::now);

        let stream = object
            .get("index")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|index| !index.is_empty())
            .map(str::to_string)
            .or_else(|| defaults.index.clone())
            .unwrap_or_else(|| fallback_stream.to_string());

        by_stream
            .entry(stream)
            .or_default()
            .push((position, RawEvent { timestamp, fields }));
    }

    if by_stream.is_empty() {
        return Err(Refusal {
            code: 5,
            text: "No data".into(),
            event: None,
        });
    }

    Ok(by_stream)
}

/// HEC's `time`: seconds since the epoch, a number or a string, with a fraction for less.
fn epoch_seconds(value: &Value) -> Option<TimestampMicros> {
    let seconds = match value {
        Value::Number(number) => number.as_f64()?,
        Value::String(text) => text.trim().parse::<f64>().ok()?,
        _ => return None,
    };

    (seconds.is_finite() && seconds > 0.0).then(|| TimestampMicros((seconds * 1e6).round() as i64))
}

/// The events of a `/raw` body: one per non-empty line.
fn parse_raw(body: &[u8], defaults: &Defaults) -> Vec<RawEvent> {
    String::from_utf8_lossy(body)
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .map(|line| {
            let mut fields = Map::from_iter([("message".into(), Value::String(line.into()))]);
            defaults.stamp(&mut fields);

            RawEvent {
                timestamp: TimestampMicros::now(),
                fields,
            }
        })
        .collect()
}

fn body(headers: &HeaderMap, body: Bytes) -> Result<Bytes> {
    decoded(gzipped(headers), body)
}

/// Writes the events, and answers with HEC's refusal when a stream refused some of them: code 6
/// with the first refused event's position and why, so the sender knows what was lost.
async fn write(
    state: &AppState,
    ctx: &IamContext,
    by_stream: ByStream,
    bytes: usize,
) -> Result<Response> {
    let refused = write_streams(state, ctx, by_stream, bytes as u64).await?;

    Ok(match refused.first() {
        Some(first) => refusal(first, refused.len()).into_response(),
        None => success(),
    })
}

fn refusal(first: &Refused, count: usize) -> Refusal {
    Refusal {
        code: 6,
        text: format!(
            "Invalid data format: {count} events rejected, the first: {}",
            first.reason
        ),
        event: Some(first.position),
    }
}

#[permission("logs.write")]
async fn event(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    RawQuery(raw_query): RawQuery,
    headers: HeaderMap,
    payload: Bytes,
) -> Result<Response> {
    let raw = body(&headers, payload)?;
    let defaults = Defaults::of(raw_query.as_deref());
    let fallback = stream_name(&headers, "default");

    let by_stream = match parse_events(&raw, &defaults, &fallback) {
        Ok(by_stream) => by_stream,
        Err(refusal) => return Ok(refusal.into_response()),
    };

    write(&state, &ctx, by_stream, raw.len()).await
}

#[permission("logs.write")]
async fn raw(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    RawQuery(raw_query): RawQuery,
    headers: HeaderMap,
    payload: Bytes,
) -> Result<Response> {
    let raw = body(&headers, payload)?;
    let defaults = Defaults::of(raw_query.as_deref());
    let events = parse_raw(&raw, &defaults);

    if events.is_empty() {
        return Ok(Refusal {
            code: 5,
            text: "No data".into(),
            event: None,
        }
        .into_response());
    }

    let stream = defaults
        .index
        .clone()
        .unwrap_or_else(|| stream_name(&headers, "default"));

    let by_stream = ByStream::from([(stream, events.into_iter().enumerate().collect())]);

    write(&state, &ctx, by_stream, raw.len()).await
}

/// What HEC's health check answers.
async fn health() -> Response {
    Json(json!({ "text": "HEC is healthy", "code": 17 })).into_response()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn streams(body: &str, query: Option<&str>) -> BTreeMap<String, Vec<RawEvent>> {
        parse_events(body.as_bytes(), &Defaults::of(query), "default")
            .unwrap()
            .into_iter()
            .map(|(stream, placed)| (stream, placed.into_iter().map(|(_, event)| event).collect()))
            .collect()
    }

    #[test]
    fn events_follow_one_another_and_go_to_their_index() {
        let body = r#"{"time": 1700000000.5, "host": "web-1", "sourcetype": "nginx",
                       "index": "edge", "event": "GET / 200", "fields": {"region": "eu"}}
                      {"event": {"level": "error", "msg": "boom"}, "source": "app"}"#;

        let by_stream = streams(body, None);
        let edge = &by_stream["edge"][0];
        let default = &by_stream["default"][0];

        assert_eq!(edge.timestamp.0, 1_700_000_000_500_000);
        assert_eq!(edge.fields["message"], "GET / 200");
        assert_eq!(edge.fields["host"], "web-1");
        assert_eq!(edge.fields["sourcetype"], "nginx");
        assert_eq!(edge.fields["region"], "eu");
        assert_eq!(default.fields["level"], "error");
        assert_eq!(default.fields["source"], "app");
    }

    #[test]
    fn the_query_fills_in_what_an_event_does_not_say() {
        let by_stream = streams(
            r#"{"event": "hello", "host": "own"}"#,
            Some("index=tencent&host=cls&sourcetype=cls_log"),
        );
        let event = &by_stream["tencent"][0];

        assert_eq!(event.fields["host"], "own");
        assert_eq!(event.fields["sourcetype"], "cls_log");
    }

    #[test]
    fn a_body_hec_would_refuse_is_refused_the_same_way() {
        let refuse = |body: &str| parse_events(body.as_bytes(), &Defaults::default(), "d").err();

        assert_eq!(refuse(r#"{"time": 1}"#).map(|r| r.code), Some(12));
        assert_eq!(refuse(r#"{"event": ""}"#).map(|r| r.code), Some(13));
        assert_eq!(
            refuse(r#"{"event": "a"} not json"#).map(|r| (r.code, r.event)),
            Some((6, Some(1)))
        );
        assert_eq!(refuse("").map(|r| r.code), Some(5));
    }

    #[test]
    fn a_raw_body_is_one_event_per_line() {
        let events = parse_raw(
            b"first line\n\nsecond line\r\n",
            &Defaults::of(Some("sourcetype=syslog")),
        );

        assert_eq!(events.len(), 2);
        assert_eq!(events[1].fields["message"], "second line");
        assert_eq!(events[1].fields["sourcetype"], "syslog");
    }

    #[test]
    fn time_is_seconds_as_a_number_or_a_string() {
        assert_eq!(
            epoch_seconds(&json!("1700000000")).unwrap().0,
            1_700_000_000_000_000
        );
        assert_eq!(epoch_seconds(&json!(1.25)).unwrap().0, 1_250_000);
        assert!(epoch_seconds(&json!("soon")).is_none());
    }

    /// Events follow one another in the body whatever stream they go to: a refusal names the
    /// event's place in the body, as HEC's `invalid-event-number` does.
    #[test]
    fn a_refused_event_is_named_by_its_place_in_the_body() {
        let body = r#"{"event": "a", "index": "x"} {"event": "b"} {"event": "c", "index": "x"}"#;
        let placed = parse_events(body.as_bytes(), &Defaults::default(), "d").unwrap();

        let positions: Vec<(&str, usize)> = placed
            .iter()
            .flat_map(|(stream, events)| {
                events
                    .iter()
                    .map(move |(position, _)| (stream.as_str(), *position))
            })
            .collect();
        assert_eq!(positions, [("d", 1), ("x", 0), ("x", 2)]);

        let first = Refused {
            position: 2,
            reason: "type mismatch on field `code`: expected Int64, got String".into(),
        };
        let refusal = refusal(&first, 1);

        assert_eq!((refusal.code, refusal.event), (6, Some(2)));
        assert!(refusal.text.contains("field `code`"));
    }
}
