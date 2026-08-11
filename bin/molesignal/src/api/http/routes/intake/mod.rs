// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Json, Router,
    body::Bytes,
    extract::{Path, State},
    routing::post,
};
use serde_json::Value;

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{
        iam::permission,
        intake::{IntakeBatch, IntakeResult, RawEvent},
        stream::StreamType,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

mod compat;
mod connectors;
pub(crate) mod otlp;
mod prometheus;

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

#[permission("streams.write")]
async fn intake_logs(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(stream): Path<String>,
    body: Bytes,
) -> Result<Json<IntakeResult>> {
    intake(state, ctx, stream, StreamType::Logs, body).await
}

#[permission("streams.write")]
async fn intake_metrics(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(stream): Path<String>,
    body: Bytes,
) -> Result<Json<IntakeResult>> {
    intake(state, ctx, stream, StreamType::Metrics, body).await
}

#[permission("streams.write")]
async fn intake_traces(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(stream): Path<String>,
    body: Bytes,
) -> Result<Json<IntakeResult>> {
    intake(state, ctx, stream, StreamType::Traces, body).await
}

async fn intake(
    state: AppState,
    ctx: IamContext,
    stream: String,
    stream_type: StreamType,
    body: Bytes,
) -> Result<Json<IntakeResult>> {
    // 计费门禁 + 计量：原始字节数用于配额/计量；license 过期 / 订阅停服 / 超 cap → 402。
    crate::api::http::billing::ensure_intake_allowed(
        &state,
        &ctx.org_id,
        body.len() as u64,
        TimestampMicros::now().0,
    )
    .await?;
    let value: Value =
        serde_json::from_slice(&body).map_err(|e| Error::invalid(format!("intake body: {e}")))?;
    let events = events_from_body(value)?;
    let batch = IntakeBatch {
        batch_id: Id::new(),
        org_id: ctx.org_id.clone(),
        stream,
        stream_type,
        events,
        received_at: TimestampMicros::now(),
    };
    let result = state.intake.intake(batch).await?;
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
