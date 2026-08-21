// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashSet;

use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::{ToolResult, catalog::BuiltinToolKind};

use super::{ToolRuntime, common::*};
use crate::{
    app::iam::IamContext,
    domain::stream::StreamType,
    infra::traces::summary_reader::{SummaryOrder, TraceSummaryQuery, TraceSummaryReader},
    shared::{
        Result,
        time::{TimeRange, TimestampMicros},
    },
};

const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 500;
const RUM_LOOKBACK_MICROS: i64 = 12 * 60 * 60 * 1_000_000;

pub(super) async fn execute(
    runtime: &ToolRuntime,
    auth: &IamContext,
    kind: BuiltinToolKind,
    arguments: Value,
) -> Result<ToolResult> {
    match kind {
        BuiltinToolKind::ListRumSessions => {
            list(
                runtime,
                auth,
                "rum_sessions",
                "sessions",
                "started_at_micros",
                arguments,
            )
            .await
        }
        BuiltinToolKind::ListRumActions => {
            list(
                runtime,
                auth,
                "rum_actions",
                "actions",
                "ts_micros",
                arguments,
            )
            .await
        }
        BuiltinToolKind::ListRumErrors => {
            list(
                runtime,
                auth,
                "rum_errors",
                "errors",
                "timestamp",
                arguments,
            )
            .await
        }
        BuiltinToolKind::GetRumSession => get_session(runtime, auth, arguments).await,
        BuiltinToolKind::GetRumRelatedTraces => related_traces(runtime, auth, arguments).await,
        _ => unreachable!("RUM handler received unrelated tool"),
    }
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListRumArgs {
    #[serde(default)]
    time_range: Option<TimeRangeArg>,
    #[serde(default)]
    application: Option<String>,
    #[serde(default)]
    environment: Option<String>,
    #[serde(default)]
    end_user_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    action_type: Option<String>,
    #[serde(default)]
    fingerprint: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

async fn list(
    runtime: &ToolRuntime,
    auth: &IamContext,
    stream: &str,
    result_key: &str,
    order_column: &str,
    arguments: Value,
) -> Result<ToolResult> {
    let args: ListRumArgs = parse_args(arguments)?;
    let range = optional_time_range(args.time_range)?;
    let limit = args.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let mut filters = Vec::new();
    push_string_filter(&mut filters, "application", args.application.as_deref());
    push_string_filter(&mut filters, "environment", args.environment.as_deref());
    push_string_filter(&mut filters, "session_id", args.session_id.as_deref());
    match stream {
        "rum_sessions" => push_string_filter(&mut filters, "user_id", args.end_user_id.as_deref()),
        "rum_errors" => {
            push_string_filter(&mut filters, "fingerprint", args.fingerprint.as_deref())
        }
        _ => push_string_filter(&mut filters, "type", args.action_type.as_deref()),
    }
    let statement = format!(
        "SELECT * FROM {}{} ORDER BY {} DESC LIMIT {limit}",
        quote_ident(stream),
        where_clause(&filters),
        quote_ident(order_column),
    );
    let Some(result) = run_optional_stream_query(
        runtime,
        &auth.org_id,
        statement,
        range,
        stream,
        StreamType::LOGS,
        limit,
    )
    .await?
    else {
        return Ok(ToolResult::json(json!({
            (result_key): [], "stream_available": false,
            "message": format!("{stream} has not received data yet"),
        })));
    };
    Ok(ToolResult::json(json!({
        (result_key): rows_as_objects(&result), "stream_available": true,
        "scanned_rows": result.scanned_rows, "took_ms": result.took_ms,
    })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionArgs {
    session_id: String,
    #[serde(default)]
    time_range: Option<TimeRangeArg>,
    #[serde(default)]
    event_limit: Option<usize>,
}

async fn get_session(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: SessionArgs = parse_args(arguments)?;
    let range = rum_range(args.time_range)?;
    let limit = args.event_limit.unwrap_or(100).clamp(1, 500);
    let session =
        query_by_session(runtime, auth, "rum_sessions", range, &args.session_id, 1).await?;
    let Some(session) = session.and_then(|result| rows_as_objects(&result).into_iter().next())
    else {
        return Ok(ToolResult::error(format!(
            "RUM session {} not found",
            args.session_id
        )));
    };
    let actions = query_by_session(runtime, auth, "rum_actions", range, &args.session_id, limit)
        .await?
        .map(|result| rows_as_objects(&result))
        .unwrap_or_default();
    let errors = query_by_session(runtime, auth, "rum_errors", range, &args.session_id, limit)
        .await?
        .map(|result| rows_as_objects(&result))
        .unwrap_or_default();
    Ok(ToolResult::json(json!({
        "session": session, "actions": actions, "errors": errors,
        "events_truncated": actions.len() == limit || errors.len() == limit,
    })))
}

async fn query_by_session(
    runtime: &ToolRuntime,
    auth: &IamContext,
    stream: &str,
    range: TimeRange,
    session_id: &str,
    limit: usize,
) -> Result<Option<crate::domain::query::QueryResult>> {
    let statement = format!(
        "SELECT * FROM {} WHERE session_id = '{}' LIMIT {limit}",
        quote_ident(stream),
        sql_literal(session_id),
    );
    run_optional_stream_query(
        runtime,
        &auth.org_id,
        statement,
        range,
        stream,
        StreamType::LOGS,
        limit,
    )
    .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RelatedArgs {
    session_id: String,
    #[serde(default)]
    time_range: Option<TimeRangeArg>,
    #[serde(default)]
    limit: Option<usize>,
}

async fn related_traces(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: RelatedArgs = parse_args(arguments)?;
    let range = rum_range(args.time_range)?;
    let limit = args.limit.unwrap_or(20).clamp(1, 100);
    let Some(trace_stream) = resolve_traces_stream(runtime, &auth.org_id).await? else {
        return Ok(ToolResult::json(json!({
            "session_id": args.session_id, "traces": [], "trace_stream_available": false,
        })));
    };
    let direct = direct_trace_ids(runtime, auth, range, &args.session_id).await?;
    let reader = TraceSummaryReader::new(
        runtime.observability.catalog_files.clone(),
        runtime.observability.object_store.clone(),
    )
    .with_catalog_source(runtime.observability.catalog_query.clone());
    let (trace_ids, relation, scan_range) = if direct.is_empty() {
        let Some(session) =
            query_by_session(runtime, auth, "rum_sessions", range, &args.session_id, 1)
                .await?
                .and_then(|result| rows_as_objects(&result).into_iter().next())
        else {
            return Ok(ToolResult::json(
                json!({"session_id": args.session_id, "traces": []}),
            ));
        };
        let started = session.get("started_at_micros").and_then(value_i64);
        let duration_ms = session.get("duration_ms").and_then(value_f64);
        let Some(started) = started else {
            return Ok(ToolResult::json(
                json!({"session_id": args.session_id, "traces": []}),
            ));
        };
        let end =
            started.saturating_add((duration_ms.unwrap_or(60_000.0) as i64).max(60_000) * 1_000);
        (
            None,
            "time_correlated",
            TimeRange::new(TimestampMicros(started), TimestampMicros(end)),
        )
    } else {
        (Some(direct), "direct", range)
    };
    let rows = reader
        .scan(
            &auth.org_id,
            &trace_stream.name,
            scan_range,
            TraceSummaryQuery {
                trace_ids: trace_ids.as_ref(),
                require_contained: trace_ids.is_none(),
                order: if trace_ids.is_some() {
                    SummaryOrder::Latest
                } else {
                    SummaryOrder::Earliest
                },
                limit,
            },
        )
        .await?;
    let traces = rows
        .into_iter()
        .map(|row| {
            json!({
                "trace_id": row.trace_id, "service": row.service,
                "span_count": row.span_count, "duration_ms": row.duration_ns as f64 / 1_000_000.0,
                "started_at_micros": row.start_ns / 1_000, "relation": relation,
            })
        })
        .collect::<Vec<_>>();
    Ok(ToolResult::json(json!({
        "session_id": args.session_id, "traces": traces,
        "trace_stream_available": true,
    })))
}

async fn direct_trace_ids(
    runtime: &ToolRuntime,
    auth: &IamContext,
    range: TimeRange,
    session_id: &str,
) -> Result<HashSet<String>> {
    let statement = format!(
        "SELECT DISTINCT trace_id FROM rum_actions WHERE session_id = '{}' AND trace_id IS NOT NULL AND trace_id != '' LIMIT 100",
        sql_literal(session_id),
    );
    let Some(result) = run_optional_stream_query(
        runtime,
        &auth.org_id,
        statement,
        range,
        "rum_actions",
        StreamType::LOGS,
        100,
    )
    .await?
    else {
        return Ok(HashSet::new());
    };
    let index = result
        .columns
        .iter()
        .position(|column| column == "trace_id");
    Ok(result
        .rows
        .iter()
        .filter_map(|row| {
            index
                .and_then(|index| row.get(index))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect())
}

fn rum_range(value: Option<TimeRangeArg>) -> Result<TimeRange> {
    value.map(time_range).transpose().map(|range| {
        range.unwrap_or_else(|| {
            let end = TimestampMicros::now();
            TimeRange::new(
                TimestampMicros(end.0.saturating_sub(RUM_LOOKBACK_MICROS)),
                end,
            )
        })
    })
}

fn value_i64(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| value.as_str()?.parse().ok())
}

fn value_f64(value: &Value) -> Option<f64> {
    value.as_f64().or_else(|| value.as_str()?.parse().ok())
}
