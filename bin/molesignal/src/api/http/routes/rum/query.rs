// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! RUM 跨信号查询 endpoint（BACKEND_REQUIREMENTS.md）。
//!
//! `GET /rum/sessions/{id}/related-traces`：把 RUM session 跟 backend traces 关联。
//! 优先看 `rum_actions.trace_id`（W3C traceparent direct）；为空时退化到时间窗 +
//! service 推断（time-correlated）。
//!
//! 设计要点：
//! - RUM 侧走写入时物理摘要 + 专用 Parquet reader，不经过 DataFusion；
//! - direct / time-correlated 在响应里通过 `relation` 字段标注，前端可显示置信度；
//! - traces 表字段走标准 OTEL / OTLP proto 列名（`trace_id` / `"service.name"` /
//!   `start_time_unix_nano` / `end_time_unix_nano`），流名按当前 org 的 traces
//!   stream 解析，优先 `default`。

use std::collections::HashSet;

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::Serialize;

use crate::{
    api::{AppState, http::routes::web::trace::resolve_traces_stream},
    app::iam::IamContext,
    domain::iam::permission,
    infra::{
        rum::read_model::RumReadModelReader,
        traces::summary_reader::{
            SummaryOrder, TraceSummaryQuery, TraceSummaryReader, TraceSummaryRecord,
        },
    },
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

/// Session detail can be opened from the 30-day list, so correlation uses the same maximum window.
const DEFAULT_LOOKBACK_US: i64 = 30 * 24 * 3600 * 1_000_000;
const ACTION_GRACE_US: i64 = 5 * 60 * 1_000_000;
const TRACE_ACTION_COLUMNS: &[&str] = &[
    "_timestamp",
    "ts_micros",
    "session_id",
    "trace_id",
    "service",
];

pub fn routes() -> Router<AppState> {
    Router::new().route("/rum/sessions/{id}/related-traces", get(related_traces))
}

#[derive(Debug, Serialize)]
pub struct RelatedTraceEntry {
    pub trace_id: String,
    pub service: Option<String>,
    pub span_count: u64,
    pub duration_ms: Option<f64>,
    pub started_at_micros: Option<i64>,
    /// `direct` = rum_actions 里带着这个 trace_id；`time-correlated` = 仅靠
    /// 时间窗 + service 推断，可能 false positive。
    pub relation: &'static str,
}

#[derive(Debug, Serialize)]
pub struct RelatedTracesResponse {
    pub session_id: String,
    pub primary_service: Option<String>,
    pub traces: Vec<RelatedTraceEntry>,
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn related_traces(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(session_id): Path<String>,
) -> Result<Json<RelatedTracesResponse>> {
    if session_id.is_empty() {
        return Err(Error::invalid("empty session id"));
    }
    let now_us = TimestampMicros::now().0;
    let lookback_range = TimeRange::new(
        TimestampMicros(now_us - DEFAULT_LOOKBACK_US),
        TimestampMicros(now_us + 60 * 1_000_000),
    );

    let rum = RumReadModelReader::new(
        state.storage.catalog_files.clone(),
        state.storage.read_store.clone(),
    )
    .with_catalog_source(state.storage.catalog_query.clone());
    let session_ids = HashSet::from([session_id.clone()]);
    let mut session = None;
    rum.visit_raw_sessions_for_ids(&ctx.org_id, lookback_range, &session_ids, |candidate| {
        if session.as_ref().is_none_or(
            |current: &crate::infra::rum::read_model::RumSessionRecord| {
                candidate.timestamp_micros > current.timestamp_micros
            },
        ) {
            session = Some(candidate);
        }
    })
    .await?;
    let Some(session_record) = session else {
        return Ok(Json(RelatedTracesResponse {
            session_id,
            primary_service: None,
            traces: Vec::new(),
        }));
    };
    let window_start = session_record.timestamp_micros;
    let duration_us = session_record
        .duration_ms
        .map(|duration| (duration.max(0.0) * 1_000.0) as i64)
        .unwrap_or(60 * 1_000_000)
        .max(60 * 1_000_000);
    let trace_window = TimeRange::new(
        TimestampMicros(window_start),
        TimestampMicros(
            window_start
                .saturating_add(duration_us)
                .saturating_add(ACTION_GRACE_US),
        ),
    );

    // 1. rum_action_summary：拿 (trace_id, service)，先按 direct 路径查。
    let mut direct: Vec<(String, Option<String>)> = Vec::new();
    rum.visit_actions_for_sessions(
        &ctx.org_id,
        trace_window,
        &session_ids,
        TRACE_ACTION_COLUMNS,
        |action| {
            let Some(trace_id) = action.trace_id.filter(|trace_id| !trace_id.is_empty()) else {
                return;
            };
            if !direct.iter().any(|(existing, _)| existing == &trace_id) {
                direct.push((trace_id, action.service));
            }
        },
    )
    .await?;
    let primary_service = direct
        .iter()
        .find_map(|(_, s)| s.clone())
        .filter(|s| !s.is_empty());

    if !direct.is_empty() {
        let trace_ids: Vec<String> = direct.iter().map(|(t, _)| t.clone()).collect();
        let traces = aggregate_traces(&state, &ctx, &trace_ids, trace_window, "direct").await?;
        return Ok(Json(RelatedTracesResponse {
            session_id,
            primary_service,
            traces: enrich_with_action_service(traces, &direct),
        }));
    }

    // 2. 退化：按 session 的真实时间窗做 time correlation。
    let traces = aggregate_traces_by_window(&state, &ctx, trace_window).await?;
    Ok(Json(RelatedTracesResponse {
        session_id,
        primary_service: None,
        traces,
    }))
}

async fn aggregate_traces(
    state: &AppState,
    ctx: &IamContext,
    trace_ids: &[String],
    range: TimeRange,
    relation: &'static str,
) -> Result<Vec<RelatedTraceEntry>> {
    let Some(stream) = resolve_traces_stream(state, &ctx.org_id).await else {
        return Ok(Vec::new());
    };
    let trace_ids = trace_ids.iter().cloned().collect::<HashSet<_>>();
    let reader = TraceSummaryReader::new(
        state.storage.catalog_files.clone(),
        state.storage.read_store.clone(),
    )
    .with_catalog_source(state.storage.catalog_query.clone());
    let rows = reader
        .scan(
            &ctx.org_id,
            &stream,
            range,
            TraceSummaryQuery {
                trace_ids: Some(&trace_ids),
                require_contained: false,
                order: SummaryOrder::Latest,
                limit: trace_ids.len().min(200),
            },
        )
        .await?;
    Ok(summary_rows_to_entries(rows, relation))
}

async fn aggregate_traces_by_window(
    state: &AppState,
    ctx: &IamContext,
    range: TimeRange,
) -> Result<Vec<RelatedTraceEntry>> {
    let Some(stream) = resolve_traces_stream(state, &ctx.org_id).await else {
        return Ok(Vec::new());
    };
    let reader = TraceSummaryReader::new(
        state.storage.catalog_files.clone(),
        state.storage.read_store.clone(),
    )
    .with_catalog_source(state.storage.catalog_query.clone());
    let rows = reader
        .scan(
            &ctx.org_id,
            &stream,
            range,
            TraceSummaryQuery {
                trace_ids: None,
                require_contained: true,
                order: SummaryOrder::Earliest,
                limit: 20,
            },
        )
        .await?;
    Ok(summary_rows_to_entries(rows, "time-correlated"))
}

fn summary_rows_to_entries(
    rows: Vec<TraceSummaryRecord>,
    relation: &'static str,
) -> Vec<RelatedTraceEntry> {
    rows.into_iter()
        .map(|row| RelatedTraceEntry {
            trace_id: row.trace_id,
            service: row.service,
            span_count: row.span_count,
            duration_ms: Some(row.duration_ns as f64 / 1_000_000.0),
            started_at_micros: Some(row.start_ns / 1_000),
            relation,
        })
        .collect()
}

fn enrich_with_action_service(
    mut entries: Vec<RelatedTraceEntry>,
    action_pairs: &[(String, Option<String>)],
) -> Vec<RelatedTraceEntry> {
    for e in entries.iter_mut() {
        if e.service.is_some() {
            continue;
        }
        if let Some((_, svc)) = action_pairs.iter().find(|(t, _)| t == &e.trace_id)
            && let Some(s) = svc.as_ref()
            && !s.is_empty()
        {
            e.service = Some(s.clone());
        }
    }
    entries
}
