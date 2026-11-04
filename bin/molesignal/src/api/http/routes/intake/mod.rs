// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Json, Router,
    body::Bytes,
    extract::{Path, State},
    http::HeaderMap,
    routing::post,
};
use serde_json::Value;

use crate::{
    api::AppState,
    app::{iam::IamContext, intake::usage::IntakeReceipt},
    domain::{
        iam::permission,
        intake::{IntakeBatch, IntakeResult, RawEvent, usage::source::IntakeCredential},
        stream::StreamType,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

mod compat;
mod connectors;
pub(crate) mod otlp;
mod prometheus;

/// 没有 `stream-name` 头时 OTLP 与 Prometheus remote_write 写入的流：与前端 explore /
/// send-test-event 相同的 `default` 流，不存在时 intake 自动建流。
const DEFAULT_STREAM: &str = "default";

/// 请求头 `stream-name` 指定的目标流，缺省为 `default`。
fn stream_name(headers: &HeaderMap, default: &str) -> String {
    headers
        .get("stream-name")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(default)
        .to_string()
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/intake/logs/{stream}", post(intake_logs))
        .route("/intake/metrics/{stream}", post(intake_metrics))
        .route("/intake/traces/{stream}", post(intake_traces))
        .merge(otlp::routes())
        .merge(prometheus::routes())
        .merge(connectors::routes())
        .merge(compat::routes())
}

#[permission("logs.write")]
async fn intake_logs(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(stream): Path<String>,
    body: Bytes,
) -> Result<Json<IntakeResult>> {
    intake(state, ctx, stream, StreamType::LOGS, body).await
}

#[permission("metrics.write")]
async fn intake_metrics(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(stream): Path<String>,
    body: Bytes,
) -> Result<Json<IntakeResult>> {
    intake(state, ctx, stream, StreamType::METRICS, body).await
}

#[permission("traces.write")]
async fn intake_traces(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(stream): Path<String>,
    body: Bytes,
) -> Result<Json<IntakeResult>> {
    intake(state, ctx, stream, StreamType::TRACES, body).await
}

async fn intake(
    state: AppState,
    ctx: IamContext,
    stream: String,
    stream_type: StreamType,
    body: Bytes,
) -> Result<Json<IntakeResult>> {
    let value: Value =
        serde_json::from_slice(&body).map_err(|e| Error::invalid(format!("intake body: {e}")))?;
    let events = events_from_body(value)?;

    let receipt = IntakeReceipt::of_events(
        IntakeCredential::of(&ctx),
        stream_type,
        &stream,
        body.len() as u64,
        &events,
    );

    let batch = IntakeBatch {
        batch_id: Id::new(),
        org_id: ctx.org_id.clone(),
        stream,
        stream_type,
        events,
        received_at: TimestampMicros::now(),
    };

    // Spans meet their stream's realtime pipeline here, before they are handed on.
    let result = if stream_type == StreamType::TRACES {
        state.intake.intake_spans(batch).await?
    } else {
        state.intake.intake(batch).await?
    };

    state
        .platform
        .intake_meter
        .record(&ctx.org_id, receipt, result.accepted as u64);

    Ok(Json(result))
}

fn events_from_body(body: Value) -> Result<Vec<RawEvent>> {
    let arr = match body {
        Value::Array(a) => a,
        Value::Object(_) => vec![body],
        other => {
            return Err(crate::shared::Error::invalid(format!(
                "intake body must be array or object, got {other:?}"
            )));
        }
    };

    let mut out = Vec::with_capacity(arr.len());

    for (i, item) in arr.into_iter().enumerate() {
        let Value::Object(mut fields) = item else {
            return Err(crate::shared::Error::invalid(format!(
                "event #{i} must be a JSON object"
            )));
        };

        let timestamp = fields
            .remove("_timestamp")
            .and_then(|v| v.as_i64())
            .map(TimestampMicros)
            .unwrap_or_else(TimestampMicros::now);

        out.push(RawEvent { timestamp, fields });
    }

    Ok(out)
}
