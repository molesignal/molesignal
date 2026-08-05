// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! RUM 跨信号查询 endpoint（BACKEND_REQUIREMENTS.md）。
//!
//! `GET /rum/sessions/{id}/related-traces`：把 RUM session 跟 backend traces 关联。
//! 优先看 `rum_actions.trace_id`（W3C traceparent direct）；为空时退化到时间窗 +
//! service 推断（time-correlated）。
//!
//! 设计要点：
//! - 两条 SQL 都走 `state.query.run`，复用现有 DataFusion engine + 多表注册逻辑；
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
use serde_json::Value;

use crate::{
    api::{AppState, http::routes::web::trace::resolve_traces_stream},
    app::iam::IamContext,
    domain::{
        iam::permission,
        query::{QueryLanguage, QueryRequest, QueryResult, StreamHint},
        stream::StreamType,
    },
    infra::traces::summary_reader::{
        SummaryOrder, TraceSummaryQuery, TraceSummaryReader, TraceSummaryRecord,
    },
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

/// 默认时间窗（µs）：12 小时；查 rum_actions / rum_sessions 不会因为列表回放过头
/// 而错过较老 session。
const DEFAULT_LOOKBACK_US: i64 = 12 * 3600 * 1_000_000;

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

    // 1. rum_actions：拿 (trace_id, service)，先按 direct 路径查。
    let action_rows = state
        .query
        .run(QueryRequest {
            org_id: ctx.org_id.clone(),
            language: QueryLanguage::Sql,
            statement: format!(
                "SELECT DISTINCT trace_id, service FROM rum_actions \
                 WHERE session_id = '{}' AND trace_id IS NOT NULL AND trace_id != ''",
                sql_escape(&session_id)
            ),
            time_range: lookback_range,
            stream: Some(StreamHint {
                name: "rum_actions".into(),
                stream_type: StreamType::Logs,
            }),
            limit: Some(50),
            federation_clusters: Vec::new(),
        })
        .await?;
    let mut direct: Vec<(String, Option<String>)> = Vec::new();
    let trace_idx = column_index(&action_rows, "trace_id");
    let service_idx = column_index(&action_rows, "service");
    if let Some(ti) = trace_idx {
        for row in &action_rows.rows {
            let trace = row
                .get(ti)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if trace.is_empty() {
                continue;
            }
            let svc =
                service_idx.and_then(|si| row.get(si).and_then(Value::as_str).map(String::from));
            if !direct.iter().any(|(t, _)| t == &trace) {
                direct.push((trace, svc));
            }
        }
    }
    let primary_service = direct
        .iter()
        .find_map(|(_, s)| s.clone())
        .filter(|s| !s.is_empty());

    if !direct.is_empty() {
        let trace_ids: Vec<String> = direct.iter().map(|(t, _)| t.clone()).collect();
        let traces = aggregate_traces(&state, &ctx, &trace_ids, lookback_range, "direct").await?;
        return Ok(Json(RelatedTracesResponse {
            session_id,
            primary_service,
            traces: enrich_with_action_service(traces, &direct),
        }));
    }

    // 2. 退化：rum_sessions 查 started_at_micros + duration_ms，按时间窗 + service
    //    在 traces 找。service 不知道时按 session 自身没法继续，返空。
    let session_rows = state
        .query
        .run(QueryRequest {
            org_id: ctx.org_id.clone(),
            language: QueryLanguage::Sql,
            statement: format!(
                "SELECT started_at_micros, duration_ms FROM rum_sessions \
                 WHERE session_id = '{}' LIMIT 1",
                sql_escape(&session_id)
            ),
            time_range: lookback_range,
            stream: Some(StreamHint {
                name: "rum_sessions".into(),
                stream_type: StreamType::Logs,
            }),
            limit: Some(1),
            federation_clusters: Vec::new(),
        })
        .await?;
    let (started_us, duration_ms) = first_row_started(&session_rows);
    let (Some(started_us), Some(duration_ms)) = (started_us, duration_ms) else {
        return Ok(Json(RelatedTracesResponse {
            session_id,
            primary_service: None,
            traces: Vec::new(),
        }));
    };
    let window_start = started_us;
    let window_end = started_us + (duration_ms as i64).max(60_000) * 1_000;
    let trace_window = TimeRange::new(TimestampMicros(window_start), TimestampMicros(window_end));
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
        state.storage.parquet_file_meta.clone(),
        state.storage.object_store.clone(),
    );
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
        state.storage.parquet_file_meta.clone(),
        state.storage.object_store.clone(),
    );
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

fn first_row_started(rows: &QueryResult) -> (Option<i64>, Option<f64>) {
    let Some(row) = rows.rows.first() else {
        return (None, None);
    };
    let started =
        column_index(rows, "started_at_micros").and_then(|i| row.get(i).and_then(Value::as_i64));
    let duration =
        column_index(rows, "duration_ms").and_then(|i| row.get(i).and_then(Value::as_f64));
    (started, duration)
}

fn column_index(rows: &QueryResult, name: &str) -> Option<usize> {
    rows.columns
        .iter()
        .position(|c| c.eq_ignore_ascii_case(name))
}

fn sql_escape(s: &str) -> String {
    s.replace('\'', "''")
}
