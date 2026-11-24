// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::{BuiltinToolKind, object_schema, open_output, time_range_schema};
use crate::ToolSpec;

pub(super) fn spec(kind: BuiltinToolKind) -> ToolSpec {
    let (description, input, tags) = match kind {
        BuiltinToolKind::ListTraces => (
            "List bounded trace summaries for a time range, with optional service, duration, error, and trace-id filters.",
            object_schema(json!({
                "timeRange": time_range_schema(), "service": {"type": "string"},
                "traceId": {"type": "string"}, "errorOnly": {"type": "boolean"},
                "minDurationMs": {"type": "number", "minimum": 0},
                "limit": {"type": "integer", "minimum": 1, "maximum": 200, "default": 50}
            })),
            vec!["Traces", "Search"],
        ),
        BuiltinToolKind::GetTrace => (
            "Fetch a bounded set of spans for one trace id. Use get_trace_dag when only structure is needed.",
            json!({
                "type": "object", "required": ["traceId"],
                "properties": {
                    "traceId": {"type": "string"},
                    "timeRange": time_range_schema(),
                    "maxSpans": {"type": "integer", "minimum": 1, "maximum": 5000, "default": 2000}
                }, "additionalProperties": false
            }),
            vec!["Traces", "Spans"],
        ),
        BuiltinToolKind::GetTraceDag => (
            "Return a compact service/span dependency DAG for one trace without full span payloads.",
            json!({
                "type": "object", "required": ["traceId"],
                "properties": {"traceId": {"type": "string"}, "timeRange": time_range_schema()},
                "additionalProperties": false
            }),
            vec!["Traces", "DAG"],
        ),
        BuiltinToolKind::ListTraceSessions => (
            "List bounded LLM/AI trace sessions when session fields exist in the trace stream.",
            object_schema(json!({
                "timeRange": time_range_schema(), "service": {"type": "string"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 200, "default": 50}
            })),
            vec!["Traces", "AI", "Sessions"],
        ),
        BuiltinToolKind::GetTraceSession => (
            "Get bounded per-trace summaries for one LLM/AI session.",
            json!({
                "type": "object", "required": ["sessionId"],
                "properties": {
                    "sessionId": {"type": "string"}, "timeRange": time_range_schema(),
                    "limit": {"type": "integer", "minimum": 1, "maximum": 500, "default": 100}
                }, "additionalProperties": false
            }),
            vec!["Traces", "AI", "Sessions"],
        ),
        BuiltinToolKind::ListTraceUsers => (
            "List bounded AI trace-user aggregates when user fields exist in the trace stream.",
            object_schema(json!({
                "timeRange": time_range_schema(), "service": {"type": "string"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 200, "default": 50}
            })),
            vec!["Traces", "AI", "Users"],
        ),
        BuiltinToolKind::CompareAttributeDistributions => (
            "Compare the slow or failed requests in a time range with the other requests of the same service (or of every service), field by field, and rank the fields by how differently their values are shared: a score from 0 (the same mix) to 1 (no value in common), with the values whose shares differ most. Say which requests to compare with errors, minDurationMs and maxDurationMs (all that are given must hold). Read from a sample of up to 1,000 Spans of each set; ids and moments are left out. Call it to find what slow or failing requests have in common, such as a region, a version, a tenant or a route.",
            json!({
                "type": "object",
                "properties": {
                    "timeRange": time_range_schema(),
                    "service": {"type": "string", "description": "Only this service's requests; every service's when left out."},
                    "operation": {"type": "string", "description": "Only Spans of this name."},
                    "errors": {"type": "boolean", "default": false, "description": "Compare the failed requests."},
                    "minDurationMs": {"type": "number", "minimum": 0, "description": "Compare the requests that took at least this long."},
                    "maxDurationMs": {"type": "number", "minimum": 0, "description": "Compare the requests that took less than this long."},
                    "allSpans": {"type": "boolean", "default": false, "description": "Read every Span, not only those where a service took a request in."},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 20, "default": 10}
                }, "additionalProperties": false
            }),
            vec!["Traces", "Investigation"],
        ),
        _ => unreachable!("trace catalog received non-trace kind"),
    };

    let tool = ToolSpec::read(
        kind.name(),
        description,
        "observability",
        "traces",
        input,
        open_output(),
        &["traces.read", "sys.telemetry.read"],
        &tags,
    );

    let tool = tool.any_permission();

    if matches!(
        kind,
        BuiltinToolKind::ListTraces | BuiltinToolKind::GetTrace
    ) {
        tool.pinned()
    } else {
        tool
    }
}
