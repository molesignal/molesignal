// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Events pushed by cloud event buses to `POST /api/v1/_cloud_events`, one log event each.
//!
//! A request holds one event or a JSON array of them, in any of these shapes:
//!
//! - CloudEvents 1.0 in structured mode ([`cloudevents`]): Alibaba Cloud EventBridge, Tencent
//!   Cloud EventBridge, Huawei Cloud EventGrid, Baidu Cloud BCM, Azure Event Grid with the
//!   CloudEvents schema, Knative and so on; or in binary mode, the attributes in `ce-*` headers
//!   and the body as the data;
//! - Amazon EventBridge events sent by an API destination ([`event_bridge`]);
//! - the Azure Event Grid event schema ([`event_grid`]), whose subscription validation is
//!   answered here too;
//! - any other JSON object, kept as one event with its own fields.
//!
//! Every shape writes the same fields: `event_id`, `event_source`, `event_type`, `event_subject`,
//! `account_id`, `region`, `resources`, `event_format`, and the payload as `data` (kept whole as
//! one JSON field) or, when it is a string, as `message`. The event's time is its own, else now.

use base64::Engine as _;
use domain::intake::RawEvent;
use kernel::{Error, Result, time::TimestampMicros};
use serde_json::{Map, Value};

mod cloudevents;
mod event_bridge;
mod event_grid;

/// What one request asks for.
#[derive(Debug)]
pub enum Delivery {
    /// Azure Event Grid confirms a new subscription: the answer echoes this code.
    Validation(String),
    Events(Vec<CloudEvent>),
}

/// One event, read.
#[derive(Debug)]
pub struct CloudEvent {
    /// The event's `source` and `id`: the same pair again is a retry. `None` without an id.
    pub dedup_key: Option<String>,
    pub event: RawEvent,
}

/// What a request carries beside its body.
#[derive(Debug, Default)]
pub struct Request<'a> {
    /// The `ce-*` headers, names lower-cased and without the prefix: an event in binary mode.
    pub binary: Vec<(String, String)>,
    pub content_type: Option<&'a str>,
    /// The `aeg-event-type` header Azure Event Grid sends.
    pub event_grid_type: Option<&'a str>,
}

/// Reads one request. A body that is not JSON is refused, unless the event is in binary mode.
pub fn read(body: &[u8], request: &Request<'_>) -> Result<Delivery> {
    if request.binary.iter().any(|(name, _)| name == "specversion") {
        let event = cloudevents::binary(&request.binary, request.content_type, body);

        return Ok(Delivery::Events(vec![event]));
    }

    let value: Value = serde_json::from_slice(body)
        .map_err(|e| Error::invalid(format!("the body is not JSON: {e}")))?;

    let items = match value {
        Value::Array(items) => items,
        other => vec![other],
    };

    if let Some(code) = event_grid::validation_code(&items, request.event_grid_type)? {
        return Ok(Delivery::Validation(code));
    }

    Ok(Delivery::Events(items.into_iter().map(event).collect()))
}

fn event(value: Value) -> CloudEvent {
    match value {
        Value::Object(object) if object.contains_key("specversion") => {
            cloudevents::structured(object)
        }
        Value::Object(object) if event_bridge::is_event(&object) => event_bridge::event(object),
        Value::Object(object) if event_grid::is_event(&object) => event_grid::event(object),
        Value::Object(fields) => CloudEvent {
            dedup_key: None,
            event: RawEvent {
                timestamp: TimestampMicros::now(),
                fields,
            },
        },
        other => {
            let mut fields = Map::new();

            payload(&mut fields, other, None);

            CloudEvent {
                dedup_key: None,
                event: RawEvent {
                    timestamp: TimestampMicros::now(),
                    fields,
                },
            }
        }
    }
}

/// The fields every shape names, taken out of the sender's own names.
#[derive(Debug, Default)]
struct Named {
    id: Option<String>,
    source: Option<String>,
    kind: Option<String>,
    subject: Option<String>,
    account: Option<String>,
    region: Option<String>,
    time: Option<TimestampMicros>,
}

impl Named {
    /// Writes the named fields over `fields` and makes the event.
    fn into_event(self, format: &str, mut fields: Map<String, Value>) -> CloudEvent {
        let dedup_key = self
            .id
            .as_ref()
            .map(|id| format!("{}\n{id}", self.source.as_deref().unwrap_or_default()));

        let named = [
            ("event_id", self.id),
            ("event_source", self.source),
            ("event_type", self.kind),
            ("event_subject", self.subject),
            ("account_id", self.account),
            ("region", self.region),
            ("event_format", Some(format.to_string())),
        ];

        for (name, value) in named {
            if let Some(value) = value {
                fields.insert(name.into(), Value::String(value));
            }
        }

        CloudEvent {
            dedup_key,
            event: RawEvent {
                timestamp: self.time.unwrap_or_else(TimestampMicros::now),
                fields,
            },
        }
    }
}

/// A non-empty string, or a number written out.
fn text(value: Option<&Value>) -> Option<String> {
    let text = match value? {
        Value::String(text) => text.trim().to_string(),
        Value::Number(number) => number.to_string(),
        _ => return None,
    };

    (!text.is_empty()).then_some(text)
}

/// An event time: RFC 3339, or Unix seconds or milliseconds as Tencent Cloud writes them.
fn time_of(value: Option<&Value>) -> Option<TimestampMicros> {
    let text = text(value)?;

    if let Ok(time) = chrono::DateTime::parse_from_rfc3339(&text) {
        return Some(TimestampMicros(time.timestamp_micros()));
    }

    let number = text.parse::<i64>().ok().filter(|number| *number > 0)?;

    if number < 100_000_000_000 {
        Some(TimestampMicros::from_secs(number))
    } else {
        Some(TimestampMicros::from_millis(number))
    }
}

/// Whether a content type is JSON: `application/json`, `text/json`, `…+json`, with parameters.
fn is_json(content_type: Option<&str>) -> bool {
    content_type.is_some_and(|value| {
        let media = value.split(';').next().unwrap_or_default().trim();

        media.eq_ignore_ascii_case("application/json")
            || media.eq_ignore_ascii_case("text/json")
            || media.to_ascii_lowercase().ends_with("+json")
    })
}

/// Writes a payload: a string as `message` (or as `data` when it is JSON of a JSON content
/// type), anything else as `data`, kept whole.
fn payload(fields: &mut Map<String, Value>, data: Value, content_type: Option<&str>) {
    match data {
        Value::Null => {}
        Value::String(text) => {
            let parsed = is_json(content_type)
                .then(|| serde_json::from_str::<Value>(&text).ok())
                .flatten()
                .filter(|value| value.is_object() || value.is_array());

            match parsed {
                Some(value) => {
                    fields.insert("data".into(), value);
                }
                None if !text.is_empty() => {
                    fields.insert("message".into(), Value::String(text));
                }
                None => {}
            }
        }
        other => {
            fields.insert("data".into(), other);
        }
    }
}

/// Writes a payload given as bytes: JSON as `data`, text as `message`, anything else as
/// `data_base64`.
fn bytes_payload(fields: &mut Map<String, Value>, bytes: &[u8], content_type: Option<&str>) {
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return;
    }

    let first = bytes.iter().find(|byte| !byte.is_ascii_whitespace());
    let looks_json = is_json(content_type) || matches!(first, Some(b'{' | b'['));

    if looks_json && let Ok(value) = serde_json::from_slice::<Value>(bytes) {
        payload(fields, value, None);
        return;
    }

    match std::str::from_utf8(bytes) {
        Ok(text) => {
            fields.insert("message".into(), Value::String(text.to_string()));
        }
        Err(_) => {
            let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);

            fields.insert("data_base64".into(), Value::String(encoded));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events(body: &str) -> Vec<CloudEvent> {
        match read(body.as_bytes(), &Request::default()).unwrap() {
            Delivery::Events(events) => events,
            Delivery::Validation(code) => panic!("validation {code}"),
        }
    }

    #[test]
    fn a_body_that_is_not_json_is_refused() {
        assert!(read(b"hello", &Request::default()).is_err());
    }

    #[test]
    fn another_json_object_keeps_its_fields_and_has_no_dedup_key() {
        let events = events(r#"{"alert": "disk full", "host": "web-1"}"#);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].dedup_key, None);
        assert_eq!(events[0].event.fields["alert"], "disk full");
        assert!(!events[0].event.fields.contains_key("event_format"));
    }

    #[test]
    fn a_batch_mixes_shapes_and_a_bare_value_becomes_a_message() {
        let events = events(
            r#"[{"specversion": "1.0", "id": "1", "source": "s", "type": "t"},
                {"version": "0", "id": "2", "detail-type": "d", "source": "aws.ec2",
                 "account": "1", "time": "2017-12-22T18:43:48Z", "region": "us-west-1",
                 "resources": [], "detail": {}},
                "just text"]"#,
        );

        assert_eq!(events.len(), 3);
        assert_eq!(events[0].event.fields["event_format"], "cloudevents");
        assert_eq!(events[1].event.fields["event_format"], "eventbridge");
        assert_eq!(events[2].event.fields["message"], "just text");
    }

    #[test]
    fn times_are_rfc3339_or_unix_seconds_or_millis() {
        assert_eq!(
            time_of(Some(&Value::from("2021-08-12T13:52:31+08:00"))),
            Some(TimestampMicros::from_secs(1_628_747_551))
        );
        assert_eq!(
            time_of(Some(&Value::from("1615430559146"))),
            Some(TimestampMicros::from_millis(1_615_430_559_146))
        );
        assert_eq!(time_of(Some(&Value::from("soon"))), None);
    }

    #[test]
    fn a_json_string_payload_is_read_only_under_a_json_content_type() {
        let mut fields = Map::new();
        payload(
            &mut fields,
            Value::from(r#"{"a": 1}"#),
            Some("application/json; charset=utf-8"),
        );
        assert_eq!(fields["data"]["a"], 1);

        let mut fields = Map::new();
        payload(&mut fields, Value::from(r#"{"a": 1}"#), Some("text/plain"));
        assert_eq!(fields["message"], r#"{"a": 1}"#);

        let mut fields = Map::new();
        bytes_payload(&mut fields, &[0xff, 0xfe], None);
        assert_eq!(fields["data_base64"], "//4=");
    }
}
