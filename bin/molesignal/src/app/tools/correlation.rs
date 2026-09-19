// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::{ToolContent, ToolResult, catalog::BuiltinToolKind};

use super::{ToolRuntime, common::*, profiles, traces};
use crate::{
    app::iam::IamContext,
    domain::stream::StreamType,
    shared::{Error, Result, ids::Id},
};

pub(super) async fn execute(
    runtime: &ToolRuntime,
    auth: &IamContext,
    kind: BuiltinToolKind,
    arguments: Value,
) -> Result<ToolResult> {
    match kind {
        BuiltinToolKind::GetServiceTopology => topology(runtime, auth, arguments).await,
        BuiltinToolKind::CorrelateSignals => correlate(runtime, auth, arguments).await,
        _ => unreachable!("correlation handler received unrelated tool"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TopologyArgs {
    time_range: TimeRangeArg,
    #[serde(default)]
    service: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Default)]
struct EdgeAggregate {
    request_count: u64,
    error_count: u64,
    p95_us: Option<i64>,
}

async fn topology(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: TopologyArgs = parse_args(arguments)?;
    let range = time_range(args.time_range)?;
    let limit = args.limit.unwrap_or(100).clamp(1, 500);
    let snapshots = runtime
        .observability
        .service_graph
        .query(
            &auth.org_id,
            range.start.0,
            range.end.0,
            args.service.as_deref(),
        )
        .await?;
    let mut aggregate = BTreeMap::<(String, String), EdgeAggregate>::new();
    for edge in snapshots {
        let item = aggregate
            .entry((edge.client_service, edge.server_service))
            .or_default();
        item.request_count = item.request_count.saturating_add(edge.request_count);
        item.error_count = item.error_count.saturating_add(edge.error_count);
        item.p95_us = match (item.p95_us, edge.p95_us) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (left, right) => left.or(right),
        };
    }
    let mut edges = aggregate
        .into_iter()
        .map(|((client, server), edge)| json!({
            "client_service": client, "server_service": server,
            "request_count": edge.request_count, "error_count": edge.error_count,
            "error_rate": if edge.request_count == 0 { 0.0 } else { edge.error_count as f64 / edge.request_count as f64 },
            "p95_us": edge.p95_us,
        }))
        .collect::<Vec<_>>();
    edges.sort_by_key(|edge| std::cmp::Reverse(edge["request_count"].as_u64().unwrap_or(0)));
    let truncated = edges.len() > limit;
    edges.truncate(limit);
    Ok(ToolResult::json(json!({
        "edges": edges, "truncated": truncated,
    })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CorrelateArgs {
    time_range: TimeRangeArg,
    #[serde(default)]
    service: Option<String>,
    #[serde(default)]
    trace_id: Option<String>,
    #[serde(default)]
    incident_id: Option<String>,
    #[serde(default)]
    limit_per_signal: Option<usize>,
}

async fn correlate(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: CorrelateArgs = parse_args(arguments)?;
    if args.service.is_none() && args.trace_id.is_none() && args.incident_id.is_none() {
        return Err(Error::invalid(
            "at least one of service, trace_id, or incident_id is required",
        ));
    }
    if let Some(trace_id) = args.trace_id.as_deref() {
        validate_trace_id(trace_id)?;
    }
    let limit = args.limit_per_signal.unwrap_or(20).clamp(1, 100);
    let range = time_range(args.time_range)?;
    let incident = if let Some(id) = args.incident_id.as_deref() {
        let incident = runtime
            .alerting
            .service
            .get_incident(&Id(id.to_string()))
            .await?;
        if incident.org_id != auth.org_id {
            return Err(Error::not_found("incident not found"));
        }
        Some(incident)
    } else {
        None
    };
    let service = args.service.clone().or_else(|| {
        incident
            .as_ref()
            .and_then(|incident| incident.affected_services.first().cloned())
    });
    let trace_id = args.trace_id.clone().or_else(|| {
        incident
            .as_ref()
            .and_then(|incident| incident.trace_ids.first().cloned())
    });

    let traces = traces::execute(
        runtime,
        auth,
        BuiltinToolKind::ListTraces,
        json!({
            "time_range": args.time_range, "service": service,
            "trace_id": trace_id, "limit": limit,
        }),
    );
    let profiles = profiles::execute(
        runtime,
        auth,
        BuiltinToolKind::ListContinuousProfiles,
        json!({
            "time_range": args.time_range, "service": service,
            "trace_id": trace_id, "limit": limit,
        }),
    );
    let rum = correlated_rum(
        runtime,
        auth,
        range,
        service.as_deref(),
        trace_id.as_deref(),
        limit,
    );
    let topology = topology(
        runtime,
        auth,
        json!({"time_range": args.time_range, "service": service, "limit": limit}),
    );
    let (traces, profiles, rum, topology) = tokio::join!(traces, profiles, rum, topology);
    Ok(ToolResult::json(json!({
        "resolved_context": {"service": service, "trace_id": trace_id},
        "incident": incident,
        "traces": json_content(traces?),
        "profiles": json_content(profiles?),
        "rum": rum?,
        "topology": json_content(topology?),
        "slow_queries": runtime.observability.slow_queries.list_recent(&auth.org_id, limit as i64).await?,
    })))
}

async fn correlated_rum(
    runtime: &ToolRuntime,
    auth: &IamContext,
    range: crate::shared::time::TimeRange,
    service: Option<&str>,
    trace_id: Option<&str>,
    limit: usize,
) -> Result<Value> {
    let mut filters = Vec::new();
    push_string_filter(&mut filters, "service", service);
    push_string_filter(&mut filters, "trace_id", trace_id);
    let statement = format!(
        "SELECT * FROM rum_actions{} ORDER BY ts_micros DESC LIMIT {limit}",
        where_clause(&filters),
    );
    let result = run_optional_stream_query(
        runtime,
        &auth.org_id,
        statement,
        range,
        "rum_actions",
        StreamType::LOGS,
        limit,
    )
    .await?;
    Ok(json!({
        "events": result.as_ref().map(rows_as_objects).unwrap_or_default(),
        "stream_available": result.is_some(),
    }))
}

fn json_content(result: ToolResult) -> Value {
    result
        .content
        .into_iter()
        .find_map(|content| match content {
            ToolContent::Json { json } => Some(json),
            ToolContent::Text { .. } => None,
        })
        .unwrap_or(Value::Null)
}
