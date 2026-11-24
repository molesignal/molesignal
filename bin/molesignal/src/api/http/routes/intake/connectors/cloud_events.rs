// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `POST /api/v1/_cloud_events`: the HTTP target of a cloud event bus.
//!
//! What the body holds is read by [`crate::infra::connectors::cloud_events`]: CloudEvents
//! (structured or binary mode, one event or a batch), Amazon EventBridge events sent by an API
//! destination, and Azure Event Grid events. Each event becomes a log event in `targetStream`
//! (default `cloud_events`).
//!
//! - Every request carries the connector token, the handshakes included: `?token=` in the address
//!   the subscription is given, or a header the sender can set.
//! - Azure Event Grid validates a subscription with a validation event, answered with
//!   `{"validationResponse": <code>}`; the CloudEvents webhook handshake is an `OPTIONS` with
//!   `WebHook-Request-Origin`, answered by [`allow_origin`].
//! - Success is `200`. A body that is not JSON is `400`; a write that fails for now propagates its
//!   `429` or `5xx`, which EventBridge and Event Grid both retry.
//! - Senders retry with the same `source` and `id`: an event written in the last
//!   [`SEEN_TTL_SECS`] seconds is not written again.

use std::{collections::HashSet, sync::OnceLock, time::Duration};

use axum::{
    Json,
    body::Bytes,
    extract::{RawQuery, State},
    http::{HeaderMap, HeaderValue, StatusCode, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
};
use domain::stream::StreamType;
use kernel::Result;
use serde_json::json;

use super::{
    MAX_BODY_BYTES, decode_body, intake_events, resolve_connector, stream_of, token_from, touch,
};
use crate::{
    api::AppState,
    infra::connectors::{
        ConnectorKind,
        cloud_events::{self, Delivery, Request},
    },
};

/// How long a written event is remembered.
const SEEN_TTL_SECS: u64 = 30 * 60;

fn seen() -> &'static moka::sync::Cache<String, ()> {
    static SEEN: OnceLock<moka::sync::Cache<String, ()>> = OnceLock::new();

    SEEN.get_or_init(|| {
        moka::sync::Cache::builder()
            .max_capacity(200_000)
            .time_to_live(Duration::from_secs(SEEN_TTL_SECS))
            .build()
    })
}

pub(super) async fn receive(
    State(state): State<AppState>,
    RawQuery(raw_query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    let token = token_from(&headers, raw_query.as_deref());
    let connector = resolve_connector(&state, ConnectorKind::CloudEvents, token).await?;
    let raw = decode_body(&headers, &body, MAX_BODY_BYTES)?;
    let header = |name: &str| headers.get(name).and_then(|value| value.to_str().ok());

    let request = Request {
        binary: binary_attributes(&headers),
        content_type: header(CONTENT_TYPE.as_str()),
        event_grid_type: header("aeg-event-type"),
    };

    let events = match cloud_events::read(&raw, &request)? {
        Delivery::Validation(code) => {
            touch(&state, &connector).await;

            return Ok((StatusCode::OK, Json(json!({"validationResponse": code}))).into_response());
        }
        Delivery::Events(events) => events,
    };

    let key = |dedup_key: &str| format!("{}:{dedup_key}", connector.id.0);
    let mut in_request = HashSet::new();
    let mut written = Vec::new();
    let mut fresh = Vec::with_capacity(events.len());

    for event in events {
        if let Some(dedup_key) = event.dedup_key.as_deref().map(key) {
            if seen().contains_key(&dedup_key) || !in_request.insert(dedup_key.clone()) {
                continue;
            }

            written.push(dedup_key);
        }

        fresh.push(event.event);
    }

    let stream = stream_of(&connector, "targetStream", "cloud_events");

    intake_events(
        &state,
        &connector,
        StreamType::LOGS,
        &stream,
        fresh,
        raw.len(),
    )
    .await?;

    for dedup_key in written {
        seen().insert(dedup_key, ());
    }

    touch(&state, &connector).await;
    Ok(StatusCode::OK.into_response())
}

/// The CloudEvents webhook handshake (abuse protection): the sender asks with `OPTIONS` and
/// `WebHook-Request-Origin`, and may deliver once the answer allows that origin. See
/// <https://github.com/cloudevents/spec/blob/v1.0.2/cloudevents/http-webhook.md#4-abuse-protection>.
pub(super) async fn allow_origin(
    State(state): State<AppState>,
    RawQuery(raw_query): RawQuery,
    headers: HeaderMap,
) -> Result<Response> {
    let token = token_from(&headers, raw_query.as_deref());

    resolve_connector(&state, ConnectorKind::CloudEvents, token).await?;
    Ok(handshake(&headers))
}

/// The answer to a handshake: the origin asked for and any rate, or just what is allowed.
fn handshake(headers: &HeaderMap) -> Response {
    let mut response = StatusCode::OK.into_response();
    let answer = response.headers_mut();

    answer.insert("allow", HeaderValue::from_static("POST, OPTIONS"));

    if let Some(origin) = headers.get("webhook-request-origin") {
        answer.insert("webhook-allowed-origin", origin.clone());
        answer.insert("webhook-allowed-rate", HeaderValue::from_static("*"));
    }

    response
}

/// The `ce-*` headers of an event in binary mode, without the prefix.
fn binary_attributes(headers: &HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            let attribute = name.as_str().strip_prefix("ce-")?;
            let value = value.to_str().ok()?;

            Some((attribute.to_string(), value.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_handshake_allows_the_origin_that_asks() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "webhook-request-origin",
            HeaderValue::from_static("eventemitter.example.com"),
        );

        let response = handshake(&headers);

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["webhook-allowed-origin"],
            "eventemitter.example.com"
        );
        assert_eq!(response.headers()["webhook-allowed-rate"], "*");

        let plain = handshake(&HeaderMap::new());

        assert!(plain.headers().get("webhook-allowed-origin").is_none());
        assert_eq!(plain.headers()["allow"], "POST, OPTIONS");
    }

    #[test]
    fn binary_mode_attributes_come_from_the_ce_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("ce-specversion", HeaderValue::from_static("1.0"));
        headers.insert("ce-id", HeaderValue::from_static("42"));
        headers.insert("content-type", HeaderValue::from_static("application/json"));

        let mut attributes = binary_attributes(&headers);
        attributes.sort();

        assert_eq!(
            attributes,
            vec![
                ("id".to_string(), "42".to_string()),
                ("specversion".to_string(), "1.0".to_string()),
            ]
        );
    }
}
