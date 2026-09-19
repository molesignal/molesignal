// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::{BuiltinToolKind, object_schema, open_output, time_range_schema};
use crate::ToolSpec;

pub(super) fn spec(kind: BuiltinToolKind) -> ToolSpec {
    let (description, category, input, tags) = match kind {
        BuiltinToolKind::QueryLogs => (
            "Run bounded read-only SQL against one logs stream. First inspect the exact schema with get_stream_schema; time_range already constrains event time.",
            "logs",
            json!({
                "type": "object", "required": ["sql", "stream", "time_range"],
                "properties": {
                    "sql": {"type": "string"}, "stream": {"type": "string"},
                    "time_range": time_range_schema(),
                    "limit": {"type": "integer", "minimum": 1, "maximum": 5000, "default": 500}
                }, "additionalProperties": false
            }),
            vec!["Logs", "SQL"],
        ),
        BuiltinToolKind::QueryMetrics => (
            "Run one bounded read-only PromQL range query. Grafana template helpers such as label_values() are not PromQL.",
            "metrics",
            json!({
                "type": "object", "required": ["promql", "time_range"],
                "properties": {
                    "promql": {"type": "string"}, "time_range": time_range_schema(),
                    "limit": {"type": "integer", "minimum": 1, "maximum": 5000, "default": 1000}
                }, "additionalProperties": false
            }),
            vec!["Metrics", "PromQL"],
        ),
        BuiltinToolKind::ListStreams => (
            "List queryable streams as compact summaries; use get_stream_schema for fields.",
            "streams",
            object_schema(json!({
                "stream_type": {"type": "string", "enum": ["logs", "metrics", "traces", "profiles", "extend"]},
                "limit": {"type": "integer", "minimum": 1, "maximum": 1000, "default": 200}
            })),
            vec!["Streams", "Discovery"],
        ),
        BuiltinToolKind::GetStreamSchema => (
            "Return exact fields, types, settings, and retention for one stream.",
            "streams",
            json!({
                "type": "object", "required": ["stream", "stream_type"],
                "properties": {
                    "stream": {"type": "string"},
                    "stream_type": {"type": "string", "enum": ["logs", "metrics", "traces", "profiles", "extend"]}
                }, "additionalProperties": false
            }),
            vec!["Streams", "Schema"],
        ),
        BuiltinToolKind::GetStreamSettings => (
            "Return query, retention, partitioning, and ingestion settings for one stream without repeating its full schema.",
            "streams",
            json!({
                "type": "object", "required": ["stream", "stream_type"],
                "properties": {
                    "stream": {"type": "string"},
                    "stream_type": {"type": "string", "enum": ["logs", "metrics", "traces", "profiles", "extend"]}
                }, "additionalProperties": false
            }),
            vec!["Streams", "Settings"],
        ),
        BuiltinToolKind::ListMetricNames => (
            "List metric stream names, optionally filtered by a substring.",
            "metrics",
            object_schema(json!({
                "query": {"type": "string"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 1000, "default": 200}
            })),
            vec!["Metrics", "Discovery"],
        ),
        BuiltinToolKind::ListMetricLabelValues => (
            "List distinct values for one real metric label field without using non-PromQL helpers.",
            "metrics",
            json!({
                "type": "object", "required": ["metric", "label", "time_range"],
                "properties": {
                    "metric": {"type": "string"}, "label": {"type": "string"},
                    "time_range": time_range_schema(), "query": {"type": "string"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 1000, "default": 200}
                }, "additionalProperties": false
            }),
            vec!["Metrics", "Labels"],
        ),
        BuiltinToolKind::ListMetricLabels => (
            "List real label fields for one metric stream.",
            "metrics",
            json!({
                "type": "object", "required": ["metric"],
                "properties": {"metric": {"type": "string"}}, "additionalProperties": false
            }),
            vec!["Metrics", "Labels", "Metadata"],
        ),
        BuiltinToolKind::ListMetricSeries => (
            "List bounded distinct metric-label series for a time range.",
            "metrics",
            json!({
                "type": "object", "required": ["metric", "time_range"],
                "properties": {
                    "metric": {"type": "string"}, "time_range": time_range_schema(),
                    "match": {"type": "object", "additionalProperties": {"type": "string"}},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 1000, "default": 200}
                }, "additionalProperties": false
            }),
            vec!["Metrics", "Series", "Metadata"],
        ),
        BuiltinToolKind::SearchAround => (
            "Fetch log rows immediately before and after an anchor timestamp.",
            "logs",
            json!({
                "type": "object", "required": ["stream", "anchor_micros"],
                "properties": {
                    "stream": {"type": "string"}, "anchor_micros": {"type": "integer"},
                    "before": {"type": "integer", "minimum": 1, "maximum": 250, "default": 25},
                    "after": {"type": "integer", "minimum": 1, "maximum": 250, "default": 25},
                    "filter_sql": {"type": "string", "description": "Optional boolean SQL expression; no SELECT/ORDER/LIMIT."}
                }, "additionalProperties": false
            }),
            vec!["Logs", "Context"],
        ),
        BuiltinToolKind::SearchFieldValues => (
            "Return bounded distinct values and counts for one exact stream field.",
            "logs",
            json!({
                "type": "object", "required": ["stream", "field", "time_range"],
                "properties": {
                    "stream": {"type": "string"}, "field": {"type": "string"},
                    "stream_type": {"type": "string", "enum": ["logs", "metrics", "traces", "profiles", "extend"], "default": "logs"},
                    "time_range": time_range_schema(), "query": {"type": "string"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 500, "default": 50}
                }, "additionalProperties": false
            }),
            vec!["Search", "Fields"],
        ),
        BuiltinToolKind::CorrelateSignals => (
            "Return bounded links between incidents, traces, logs, RUM, and profiles for a service or trace.",
            "correlation",
            json!({
                "type": "object", "required": ["time_range"],
                "properties": {
                    "time_range": time_range_schema(), "service": {"type": "string"},
                    "trace_id": {"type": "string"}, "incident_id": {"type": "string"},
                    "limit_per_signal": {"type": "integer", "minimum": 1, "maximum": 100, "default": 20}
                }, "additionalProperties": false
            }),
            vec!["Correlation", "Investigation"],
        ),
        BuiltinToolKind::GetServiceTopology => (
            "Return a bounded service dependency graph for a time range.",
            "topology",
            json!({
                "type": "object", "required": ["time_range"],
                "properties": {
                    "time_range": time_range_schema(), "service": {"type": "string"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 500, "default": 100}
                }, "additionalProperties": false
            }),
            vec!["Topology", "Services"],
        ),
        _ => unreachable!("observability catalog received unrelated kind"),
    };
    let tool = ToolSpec::read(
        kind.name(),
        description,
        "observability",
        category,
        input,
        open_output(),
        &["streams.query", "sys.telemetry.read"],
        &tags,
    );
    let tool = tool.any_permission();
    if matches!(
        kind,
        BuiltinToolKind::QueryLogs
            | BuiltinToolKind::QueryMetrics
            | BuiltinToolKind::ListStreams
            | BuiltinToolKind::GetStreamSchema
    ) {
        tool.pinned()
    } else {
        tool
    }
}
