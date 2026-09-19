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
                "time_range": time_range_schema(), "service": {"type": "string"},
                "trace_id": {"type": "string"}, "error_only": {"type": "boolean"},
                "min_duration_ms": {"type": "number", "minimum": 0},
                "limit": {"type": "integer", "minimum": 1, "maximum": 200, "default": 50}
            })),
            vec!["Traces", "Search"],
        ),
        BuiltinToolKind::GetTrace => (
            "Fetch a bounded set of spans for one trace id. Use get_trace_dag when only structure is needed.",
            json!({
                "type": "object", "required": ["trace_id"],
                "properties": {
                    "trace_id": {"type": "string"},
                    "time_range": time_range_schema(),
                    "max_spans": {"type": "integer", "minimum": 1, "maximum": 5000, "default": 2000}
                }, "additionalProperties": false
            }),
            vec!["Traces", "Spans"],
        ),
        BuiltinToolKind::GetTraceDag => (
            "Return a compact service/span dependency DAG for one trace without full span payloads.",
            json!({
                "type": "object", "required": ["trace_id"],
                "properties": {"trace_id": {"type": "string"}, "time_range": time_range_schema()},
                "additionalProperties": false
            }),
            vec!["Traces", "DAG"],
        ),
        BuiltinToolKind::ListTraceSessions => (
            "List bounded LLM/AI trace sessions when session fields exist in the trace stream.",
            object_schema(json!({
                "time_range": time_range_schema(), "service": {"type": "string"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 200, "default": 50}
            })),
            vec!["Traces", "AI", "Sessions"],
        ),
        BuiltinToolKind::GetTraceSession => (
            "Get bounded per-trace summaries for one LLM/AI session.",
            json!({
                "type": "object", "required": ["session_id"],
                "properties": {
                    "session_id": {"type": "string"}, "time_range": time_range_schema(),
                    "limit": {"type": "integer", "minimum": 1, "maximum": 500, "default": 100}
                }, "additionalProperties": false
            }),
            vec!["Traces", "AI", "Sessions"],
        ),
        BuiltinToolKind::ListTraceUsers => (
            "List bounded AI trace-user aggregates when user fields exist in the trace stream.",
            object_schema(json!({
                "time_range": time_range_schema(), "service": {"type": "string"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 200, "default": 50}
            })),
            vec!["Traces", "AI", "Users"],
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
        &["streams.query", "sys.telemetry.read"],
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
