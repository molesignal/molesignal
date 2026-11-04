// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The backend traces of a session: those its requests carried, or else those made while it ran.

use std::collections::HashSet;

use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::ToolResult;

use super::{Read, list_rows, rum_range, session_row, value_i64};
use crate::{
    app::{
        iam::IamContext,
        tools::{ToolRuntime, common::*},
    },
    domain::rum::event::RumStream,
    infra::{
        rum::query::Condition,
        traces::summary_reader::{SummaryOrder, TraceSummaryQuery, TraceSummaryReader},
    },
    shared::{
        Result,
        time::{TimeRange, TimestampMicros},
    },
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RelatedArgs {
    session_id: String,
    #[serde(default)]
    time_range: Option<TimeRangeArg>,
    #[serde(default)]
    limit: Option<usize>,
}

pub(super) async fn get(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: RelatedArgs = parse_args(arguments)?;
    let range = rum_range(args.time_range)?;
    let limit = args.limit.unwrap_or(20).clamp(1, 100);

    let Some(trace_stream) = resolve_traces_stream(runtime, &auth.org_id).await? else {
        return Ok(ToolResult::json(json!({
            "sessionId": args.session_id, "traces": [], "traceStreamAvailable": false,
        })));
    };

    let of_session = [Condition::equals("session.id", args.session_id.trim())];
    let direct = direct_trace_ids(runtime, auth, range, &of_session).await?;

    let reader = TraceSummaryReader::new(
        runtime.observability.catalog_files.clone(),
        runtime.observability.object_store.clone(),
    )
    .with_catalog_source(runtime.observability.catalog_query.clone());

    let (trace_ids, relation, scan_range) = if direct.is_empty() {
        let Some(session) = session_row(runtime, auth, range, &of_session).await? else {
            return Ok(ToolResult::json(
                json!({"sessionId": args.session_id, "traces": []}),
            ));
        };

        let Some(started) = session.get("timestamp_micros").and_then(value_i64) else {
            return Ok(ToolResult::json(
                json!({"sessionId": args.session_id, "traces": []}),
            ));
        };

        // A session lasts at least a minute when looking for the traces made in it.
        let duration_micros = session
            .get("session.duration")
            .and_then(value_i64)
            .map_or(60_000_000, |nanoseconds| {
                (nanoseconds / 1_000).max(60_000_000)
            });

        (
            None,
            "time_correlated",
            TimeRange::new(
                TimestampMicros(started),
                TimestampMicros(started.saturating_add(duration_micros)),
            ),
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
                "traceId": row.trace_id, "service": row.service,
                "spanCount": row.span_count, "durationMs": row.duration_ns as f64 / 1_000_000.0,
                "startedAt": row.start_ns / 1_000, "relation": relation,
            })
        })
        .collect::<Vec<_>>();

    Ok(ToolResult::json(json!({
        "sessionId": args.session_id, "traces": traces,
        "traceStreamAvailable": true,
    })))
}

/// The trace ids the requests of a session carried (`resource.trace_id`).
async fn direct_trace_ids(
    runtime: &ToolRuntime,
    auth: &IamContext,
    range: TimeRange,
    of_session: &[Condition],
) -> Result<HashSet<String>> {
    let mut conditions = of_session.to_vec();

    conditions.push(Condition::raw(
        "(\"resource.trace_id\" IS NOT NULL AND \"resource.trace_id\" <> '')",
        &["resource.trace_id"],
        false,
    ));

    let Some(listed) = list_rows(
        runtime,
        auth,
        &Read {
            stream: RumStream::Resources,
            range,
            conditions: &conditions,
            columns: &["resource.trace_id"],
            limit: 1_000,
            oldest_first: true,
        },
    )
    .await?
    else {
        return Ok(HashSet::new());
    };

    Ok(listed
        .items
        .iter()
        .filter_map(|item| item.get("resource.trace_id").and_then(Value::as_str))
        .map(str::to_string)
        .take(100)
        .collect())
}
