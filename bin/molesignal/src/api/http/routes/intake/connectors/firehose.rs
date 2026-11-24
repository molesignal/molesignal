// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `POST /api/v1/_kinesis_firehose`: Amazon Data Firehose HTTP endpoint delivery.
//!
//! What the records hold is read by [`crate::infra::connectors::aws::kinesis_firehose`]: log events go
//! to the connector's `targetStream` (default `kinesis`), CloudWatch Metric Streams samples to its
//! `metricsStream` (default `cloudwatch`).
//!
//! The answer follows the delivery specification
//! (<https://docs.aws.amazon.com/firehose/latest/dev/httpdeliveryrequestresponse.html>):
//!
//! - always `application/json` with the request's `requestId`, a `timestamp` in milliseconds and,
//!   on failure, an `errorMessage`; any other body counts as a 500 without one;
//! - only `200` is success. `413` is a permanent failure that Firehose neither retries nor backs up
//!   to S3, so it is reserved for a body over the limit; every other failure is retried;
//! - records that cannot be read are counted and left out with `200`: a refusal would only be
//!   retried until the stream gives up.
//!
//! Firehose keeps `X-Amz-Firehose-Request-Id` across retries of one delivery. A delivery already
//! written in the last [`SEEN_TTL_SECS`] seconds is answered `200` without writing it again, so a
//! retry after a lost answer does not double it.

use std::{sync::OnceLock, time::Duration};

use axum::{
    Json,
    body::Bytes,
    extract::{RawQuery, State},
    http::{HeaderMap, HeaderValue, StatusCode, header::RETRY_AFTER},
    response::{IntoResponse, Response},
};
use domain::stream::StreamType;
use kernel::{Error, Result, time::TimestampMicros};
use serde_json::json;

use super::{decode_body, intake_events, resolve_connector, stream_of, token_from, touch};
use crate::{
    api::AppState,
    infra::connectors::{
        ConnectorKind,
        aws::kinesis_firehose::{self, Delivery, FirehoseRequest},
    },
};

pub(super) const MAX_BODY_BYTES: usize = kinesis_firehose::MAX_BODY_BYTES;

/// How long a written delivery is remembered.
const SEEN_TTL_SECS: u64 = 30 * 60;

fn seen() -> &'static moka::sync::Cache<String, ()> {
    static SEEN: OnceLock<moka::sync::Cache<String, ()>> = OnceLock::new();

    SEEN.get_or_init(|| {
        moka::sync::Cache::builder()
            .max_capacity(100_000)
            .time_to_live(Duration::from_secs(SEEN_TTL_SECS))
            .build()
    })
}

pub(super) async fn receive(
    State(state): State<AppState>,
    RawQuery(raw_query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let request_id = header(&headers, "x-amz-firehose-request-id").unwrap_or_default();

    match deliver(&state, raw_query.as_deref(), &headers, body).await {
        Ok(request_id) => answer(StatusCode::OK, &request_id, None),
        Err(error) => {
            let status =
                StatusCode::from_u16(error.http_status_code()).unwrap_or(StatusCode::BAD_GATEWAY);
            let mut response = answer(status, &request_id, Some(&error.to_string()));

            if matches!(error, Error::ResourceExhausted(_)) {
                response
                    .headers_mut()
                    .insert(RETRY_AFTER, HeaderValue::from_static("1"));
            }

            response
        }
    }
}

/// Writes one delivery; the request id to echo on success.
async fn deliver(
    state: &AppState,
    raw_query: Option<&str>,
    headers: &HeaderMap,
    body: Bytes,
) -> Result<String> {
    let token = token_from(headers, raw_query);
    let connector = resolve_connector(state, ConnectorKind::AwsKinesisFirehose, token).await?;

    let delivery = Delivery::from_headers(
        header(headers, "x-amz-firehose-source-arn").as_deref(),
        header(headers, "x-amz-firehose-common-attributes").as_deref(),
    );

    let headers = headers.clone();

    // Unpacking and reading 64 MiB of records takes a while: off the request workers.
    let (request, decoded, size) = tokio::task::spawn_blocking(move || {
        let raw = decode_body(&headers, &body, MAX_BODY_BYTES)?;
        let request: FirehoseRequest = serde_json::from_slice(&raw)
            .map_err(|e| Error::invalid(format!("firehose body: {e}")))?;
        let request_id = request.request_id.clone();
        let decoded = kinesis_firehose::decode(request, &delivery);

        Ok::<_, Error>((request_id, decoded, raw.len()))
    })
    .await
    .map_err(|e| Error::internal(format!("firehose decode join: {e}")))??;

    let seen_key = format!("{}:{request}", connector.id.0);

    if !request.is_empty() && seen().contains_key(&seen_key) {
        return Ok(request);
    }

    if decoded.rejected > 0 {
        tracing::warn!(
            connector = %connector.id.0,
            rejected = decoded.rejected,
            "firehose records that could not be read were left out"
        );
    }

    // The body's bytes are counted once, shared by the signals by how many events each has.
    let events = (decoded.logs.len() + decoded.metrics.len()).max(1);
    let share = |count: usize| size * count / events;
    let logs_bytes = share(decoded.logs.len());
    let metrics_bytes = share(decoded.metrics.len());

    let logs_stream = stream_of(&connector, "targetStream", "kinesis");
    let metrics_stream = stream_of(&connector, "metricsStream", "cloudwatch");

    let mut refusals = intake_events(
        state,
        &connector,
        StreamType::LOGS,
        &logs_stream,
        decoded.logs,
        logs_bytes,
    )
    .await?;

    refusals.merge(
        intake_events(
            state,
            &connector,
            StreamType::METRICS,
            &metrics_stream,
            decoded.metrics,
            metrics_bytes,
        )
        .await?,
    );

    if !request.is_empty() {
        seen().insert(seen_key, ());
    }

    touch(state, &connector, refusals).await;
    Ok(request)
}

fn header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

/// The answer Firehose reads: `{requestId, timestamp, errorMessage?}`.
fn answer(status: StatusCode, request_id: &str, error: Option<&str>) -> Response {
    let mut body = json!({
        "requestId": request_id,
        "timestamp": TimestampMicros::now().0 / 1_000,
    });

    if let Some(message) = error {
        // Firehose keeps at most 8192 characters of it.
        body["errorMessage"] = json!(message.chars().take(8192).collect::<String>());
    }

    (status, Json(body)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_answer_echoes_the_request_and_says_what_failed() {
        let response = answer(StatusCode::UNAUTHORIZED, "abc", Some("no token"));

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response.headers()["content-type"],
            HeaderValue::from_static("application/json")
        );

        let body = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(body["requestId"], "abc");
        assert_eq!(body["errorMessage"], "no token");
        assert!(body["timestamp"].as_i64().unwrap() > 1_600_000_000_000);
    }

    #[tokio::test]
    async fn a_success_has_no_error_message() {
        let response = answer(StatusCode::OK, "abc", None);
        let body = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert!(body.get("errorMessage").is_none());
    }
}
