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
        ingestion::{IngestBatch, IngestResult, RawEvent},
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
        .route("/ingest/logs/{stream}", post(ingest_logs))
        .route("/ingest/metrics/{stream}", post(ingest_metrics))
        .route("/ingest/traces/{stream}", post(ingest_traces))
        .merge(otlp::routes())
        .merge(prometheus::routes())
        .merge(connectors::routes())
        .merge(compat::routes())
}

#[permission("streams.write")]
async fn ingest_logs(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(stream): Path<String>,
    body: Bytes,
) -> Result<Json<IngestResult>> {
    ingest(state, ctx, stream, StreamType::Logs, body).await
}

#[permission("streams.write")]
async fn ingest_metrics(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(stream): Path<String>,
    body: Bytes,
) -> Result<Json<IngestResult>> {
    ingest(state, ctx, stream, StreamType::Metrics, body).await
}

#[permission("streams.write")]
async fn ingest_traces(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(stream): Path<String>,
    body: Bytes,
) -> Result<Json<IngestResult>> {
    ingest(state, ctx, stream, StreamType::Traces, body).await
}

async fn ingest(
    state: AppState,
    ctx: IamContext,
    stream: String,
    stream_type: StreamType,
    body: Bytes,
) -> Result<Json<IngestResult>> {
    // 计费门禁 + 计量：原始字节数用于配额/计量；license 过期 / 订阅停服 / 超 cap → 402。
    crate::api::http::billing::ensure_ingest_allowed(
        &state,
        &ctx.org_id,
        body.len() as u64,
        TimestampMicros::now().0,
    )
    .await?;
    let value: Value =
        serde_json::from_slice(&body).map_err(|e| Error::invalid(format!("ingest body: {e}")))?;
    let events = events_from_body(value)?;
    let batch = IngestBatch {
        batch_id: Id::new(),
        org_id: ctx.org_id.clone(),
        stream,
        stream_type,
        events,
        received_at: TimestampMicros::now(),
    };
    let result = state.ingestion.ingest(batch).await?;
    Ok(Json(result))
}

fn events_from_body(body: Value) -> Result<Vec<RawEvent>> {
    let arr = match body {
        Value::Array(a) => a,
        Value::Object(_) => vec![body],
        other => {
            return Err(crate::shared::Error::invalid(format!(
                "ingest body must be array or object, got {other:?}"
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
