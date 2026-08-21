// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::{BTreeMap, BTreeSet, HashSet};

use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::{ToolResult, catalog::BuiltinToolKind};

use super::{ToolRuntime, common::*};
use crate::{
    app::{
        iam::IamContext,
        web::trace::view::{TraceResponse, rows_to_spans},
    },
    domain::{query::QueryLanguage, stream::StreamType},
    infra::traces::summary_reader::{SummaryOrder, TraceSummaryQuery, TraceSummaryReader},
    shared::{
        Result,
        time::{TimeRange, TimestampMicros},
    },
};

const TRACE_LOOKBACK_MICROS: i64 = 24 * 60 * 60 * 1_000_000;
const DEFAULT_MAX_SPANS: usize = 2_000;
const HARD_MAX_SPANS: usize = 5_000;

mod sessions;

pub(super) async fn execute(
    runtime: &ToolRuntime,
    auth: &IamContext,
    kind: BuiltinToolKind,
    arguments: Value,
) -> Result<ToolResult> {
    match kind {
        BuiltinToolKind::ListTraces => list_traces(runtime, auth, arguments).await,
        BuiltinToolKind::GetTrace => get_trace(runtime, auth, arguments).await,
        BuiltinToolKind::GetTraceDag => get_trace_dag(runtime, auth, arguments).await,
        BuiltinToolKind::ListTraceSessions
        | BuiltinToolKind::GetTraceSession
        | BuiltinToolKind::ListTraceUsers => {
            sessions::execute(runtime, auth, kind, arguments).await
        }
        _ => unreachable!("trace handler received unrelated tool"),
    }
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListTracesArgs {
    #[serde(default)]
    time_range: Option<TimeRangeArg>,
    #[serde(default)]
    service: Option<String>,
    #[serde(default)]
    trace_id: Option<String>,
    #[serde(default)]
    error_only: bool,
    #[serde(default)]
    min_duration_ms: Option<f64>,
    #[serde(default)]
    limit: Option<usize>,
}

async fn list_traces(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: ListTracesArgs = parse_args(arguments)?;
    let range = optional_time_range(args.time_range)?;
    let limit = args.limit.unwrap_or(50).clamp(1, 200);
    let Some(stream) = resolve_traces_stream(runtime, &auth.org_id).await? else {
        return Ok(ToolResult::json(
            json!({"traces": [], "stream_available": false}),
        ));
    };
    let trace_filter = match args.trace_id.as_deref() {
        Some(trace_id) => {
            validate_trace_id(trace_id)?;
            Some(HashSet::from([trace_id.to_string()]))
        }
        None => None,
    };
    let error_ids = if args.error_only {
        Some(error_trace_ids(runtime, auth, &stream.name, range, limit.saturating_mul(10)).await?)
    } else {
        None
    };
    let effective_ids = match (trace_filter, error_ids) {
        (Some(mut requested), Some(errors)) => {
            requested.retain(|id| errors.contains(id));
            Some(requested)
        }
        (Some(requested), None) => Some(requested),
        (None, Some(errors)) => Some(errors),
        (None, None) => None,
    };
    if effective_ids.as_ref().is_some_and(HashSet::is_empty) {
        return Ok(ToolResult::json(
            json!({"traces": [], "stream_available": true}),
        ));
    }
    let scan_limit = limit.saturating_mul(10).clamp(limit, 2_000);
    let reader = TraceSummaryReader::new(
        runtime.observability.catalog_files.clone(),
        runtime.observability.object_store.clone(),
    )
    .with_catalog_source(runtime.observability.catalog_query.clone());
    let rows = reader
        .scan(
            &auth.org_id,
            &stream.name,
            range,
            TraceSummaryQuery {
                trace_ids: effective_ids.as_ref(),
                require_contained: false,
                order: SummaryOrder::Latest,
                limit: scan_limit,
            },
        )
        .await?;
    let service = args.service.as_deref().map(str::to_ascii_lowercase);
    let min_duration_ns = args
        .min_duration_ms
        .map(|value| (value.max(0.0) * 1_000_000.0) as i64);
    let traces = rows
        .into_iter()
        .filter(|row| {
            service.as_ref().is_none_or(|want| {
                row.service
                    .as_ref()
                    .is_some_and(|value| value.to_ascii_lowercase().contains(want))
            }) && min_duration_ns.is_none_or(|minimum| row.duration_ns >= minimum)
        })
        .take(limit)
        .map(|row| {
            json!({
                "trace_id": row.trace_id,
                "service": row.service,
                "started_at_micros": row.start_ns / 1_000,
                "duration_ms": row.duration_ns as f64 / 1_000_000.0,
                "span_count": row.span_count,
                "has_error": args.error_only.then_some(true),
            })
        })
        .collect::<Vec<_>>();
    Ok(ToolResult::json(json!({
        "traces": traces, "stream": stream.name, "stream_available": true,
    })))
}

async fn error_trace_ids(
    runtime: &ToolRuntime,
    auth: &IamContext,
    stream: &str,
    range: TimeRange,
    limit: usize,
) -> Result<HashSet<String>> {
    let statement = format!(
        "SELECT DISTINCT trace_id FROM {} WHERE status_code = 'ERROR' LIMIT {limit}",
        quote_ident(stream),
    );
    let result = run_query(
        runtime,
        &auth.org_id,
        QueryLanguage::Sql,
        statement,
        range,
        Some((stream, StreamType::TRACES)),
        limit,
    )
    .await?;
    let index = result
        .columns
        .iter()
        .position(|column| column == "trace_id");
    Ok(result
        .rows
        .into_iter()
        .filter_map(|row| {
            index
                .and_then(|index| row.get(index))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GetTraceArgs {
    trace_id: String,
    #[serde(default)]
    time_range: Option<TimeRangeArg>,
    #[serde(default)]
    max_spans: Option<usize>,
}

async fn trace_query(
    runtime: &ToolRuntime,
    auth: &IamContext,
    args: &GetTraceArgs,
) -> Result<Option<(crate::domain::query::QueryResult, usize)>> {
    validate_trace_id(&args.trace_id)?;
    let Some(stream) = resolve_traces_stream(runtime, &auth.org_id).await? else {
        return Ok(None);
    };
    let max_spans = args
        .max_spans
        .unwrap_or(DEFAULT_MAX_SPANS)
        .clamp(1, HARD_MAX_SPANS);
    let range = args
        .time_range
        .map(time_range)
        .transpose()?
        .unwrap_or_else(|| {
            let end = TimestampMicros::now();
            TimeRange::new(
                TimestampMicros(end.0.saturating_sub(TRACE_LOOKBACK_MICROS)),
                end,
            )
        });
    let statement = format!(
        "SELECT * FROM {} WHERE trace_id = '{}' ORDER BY _timestamp ASC LIMIT {}",
        quote_ident(&stream.name),
        sql_literal(&args.trace_id),
        max_spans + 1,
    );
    let result = run_query(
        runtime,
        &auth.org_id,
        QueryLanguage::Sql,
        statement,
        range,
        Some((&stream.name, StreamType::TRACES)),
        max_spans + 1,
    )
    .await?;
    Ok(Some((result, max_spans)))
}

async fn get_trace(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: GetTraceArgs = parse_args(arguments)?;
    let Some((mut result, max_spans)) = trace_query(runtime, auth, &args).await? else {
        return Ok(ToolResult::error(format!(
            "trace {} not found",
            args.trace_id
        )));
    };
    let was_truncated = result.rows.len() > max_spans;
    result.rows.truncate(max_spans);
    let (spans, parser_truncated) = rows_to_spans(&result);
    if spans.is_empty() {
        return Ok(ToolResult::error(format!(
            "trace {} not found",
            args.trace_id
        )));
    }
    let response = TraceResponse::new(args.trace_id, spans, was_truncated || parser_truncated);
    Ok(ToolResult::json(
        serde_json::to_value(response).unwrap_or(Value::Null),
    ))
}

async fn get_trace_dag(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let mut args: GetTraceArgs = parse_args(arguments)?;
    args.max_spans = Some(HARD_MAX_SPANS);
    let Some((mut result, max_spans)) = trace_query(runtime, auth, &args).await? else {
        return Ok(ToolResult::error(format!(
            "trace {} not found",
            args.trace_id
        )));
    };
    let truncated = result.rows.len() > max_spans;
    result.rows.truncate(max_spans);
    let (spans, parser_truncated) = rows_to_spans(&result);
    if spans.is_empty() {
        return Ok(ToolResult::error(format!(
            "trace {} not found",
            args.trace_id
        )));
    }
    let mut services = BTreeSet::new();
    let mut span_service = BTreeMap::new();
    for span in &spans {
        services.insert(span.service.clone());
        span_service.insert(span.span_id.clone(), span.service.clone());
    }
    let span_edges = spans
        .iter()
        .filter_map(|span| {
            span.parent_span_id.as_ref().map(|parent| {
                json!({
                    "parent_span_id": parent, "span_id": span.span_id,
                    "parent_service": span_service.get(parent), "service": span.service,
                })
            })
        })
        .collect::<Vec<_>>();
    let service_edges = spans
        .iter()
        .filter_map(|span| {
            let parent = span.parent_span_id.as_ref()?;
            let parent_service = span_service.get(parent)?;
            (parent_service != &span.service)
                .then(|| (parent_service.clone(), span.service.clone()))
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|(from, to)| json!({"from": from, "to": to}))
        .collect::<Vec<_>>();
    Ok(ToolResult::json(json!({
        "trace_id": args.trace_id,
        "root_span_ids": spans.iter().filter(|span| span.parent_span_id.as_deref().is_none_or(str::is_empty)).map(|span| span.span_id.as_str()).collect::<Vec<_>>(),
        "services": services, "service_edges": service_edges, "span_edges": span_edges,
        "span_count": spans.len(), "truncated": truncated || parser_truncated,
    })))
}
