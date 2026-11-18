// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::{BuiltinToolKind, object_schema, open_output, time_range_schema, timestamp_schema};
use crate::ToolSpec;

pub(super) fn spec(kind: BuiltinToolKind) -> ToolSpec {
    let (description, category, input, tags) = match kind {
        BuiltinToolKind::QueryLogs => (
            "Run bounded read-only SQL against one logs stream. First inspect the exact schema with get_stream_schema; timeRange already constrains event time.",
            "logs",
            json!({
                "type": "object", "required": ["sql", "stream", "timeRange"],
                "properties": {
                    "sql": {"type": "string"}, "stream": {"type": "string"},
                    "timeRange": time_range_schema(),
                    "limit": {"type": "integer", "minimum": 1, "maximum": 5000, "default": 500}
                }, "additionalProperties": false
            }),
            vec!["Logs", "SQL"],
        ),
        BuiltinToolKind::QueryMetrics => (
            "Run one bounded read-only PromQL range query; step_seconds sets the resolution and defaults to a hundredth of the time range. Grafana template helpers such as label_values() are not PromQL.",
            "metrics",
            json!({
                "type": "object", "required": ["promql", "timeRange"],
                "properties": {
                    "promql": {"type": "string"}, "timeRange": time_range_schema(),
                    "stepSeconds": {"type": "integer", "minimum": 1},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 5000, "default": 1000}
                }, "additionalProperties": false
            }),
            vec!["Metrics", "PromQL"],
        ),
        BuiltinToolKind::ListStreams => (
            "List queryable streams as compact summaries; use get_stream_schema for fields.",
            "streams",
            object_schema(json!({
                "streamType": {"type": "string", "enum": ["logs", "metrics", "traces", "profiles", "extend"]},
                "limit": {"type": "integer", "minimum": 1, "maximum": 1000, "default": 200}
            })),
            vec!["Streams", "Discovery"],
        ),
        BuiltinToolKind::GetStreamSchema => (
            "Return exact fields, types, settings, and retention for one stream.",
            "streams",
            json!({
                "type": "object", "required": ["stream", "streamType"],
                "properties": {
                    "stream": {"type": "string"},
                    "streamType": {"type": "string", "enum": ["logs", "metrics", "traces", "profiles", "extend"]}
                }, "additionalProperties": false
            }),
            vec!["Streams", "Schema"],
        ),
        BuiltinToolKind::GetStreamSettings => (
            "Return query, retention, partitioning, and ingestion settings for one stream without repeating its full schema.",
            "streams",
            json!({
                "type": "object", "required": ["stream", "streamType"],
                "properties": {
                    "stream": {"type": "string"},
                    "streamType": {"type": "string", "enum": ["logs", "metrics", "traces", "profiles", "extend"]}
                }, "additionalProperties": false
            }),
            vec!["Streams", "Settings"],
        ),
        BuiltinToolKind::ListMetricNames => (
            "List metric names, optionally filtered by a substring.",
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
                "type": "object", "required": ["metric", "label", "timeRange"],
                "properties": {
                    "metric": {"type": "string"}, "label": {"type": "string"},
                    "timeRange": time_range_schema(), "query": {"type": "string"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 1000, "default": 200}
                }, "additionalProperties": false
            }),
            vec!["Metrics", "Labels"],
        ),
        BuiltinToolKind::ListMetricLabels => (
            "List real label fields for one metric.",
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
                "type": "object", "required": ["metric", "timeRange"],
                "properties": {
                    "metric": {"type": "string"}, "timeRange": time_range_schema(),
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
                "type": "object", "required": ["stream", "anchorTime"],
                "properties": {
                    "stream": {"type": "string"}, "anchorTime": timestamp_schema(),
                    "before": {"type": "integer", "minimum": 1, "maximum": 250, "default": 25},
                    "after": {"type": "integer", "minimum": 1, "maximum": 250, "default": 25},
                    "filterSql": {"type": "string", "description": "Optional boolean SQL expression; no SELECT/ORDER/LIMIT."}
                }, "additionalProperties": false
            }),
            vec!["Logs", "Context"],
        ),
        BuiltinToolKind::SearchFieldValues => (
            "Return bounded distinct values and counts for one exact stream field.",
            "logs",
            json!({
                "type": "object", "required": ["stream", "field", "timeRange"],
                "properties": {
                    "stream": {"type": "string"}, "field": {"type": "string"},
                    "streamType": {"type": "string", "enum": ["logs", "metrics", "traces", "profiles", "extend"], "default": "logs"},
                    "timeRange": time_range_schema(), "query": {"type": "string"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 500, "default": 50}
                }, "additionalProperties": false
            }),
            vec!["Search", "Fields"],
        ),
        BuiltinToolKind::AnalyzeLogPatterns => (
            "Group the logs of one stream in a time range into message patterns, with the parts that change from log to log (numbers, IDs, IPs, URLs, times) folded into placeholders, and compare each pattern with a baseline stretch: its count, share, first time seen, levels, and whether it is new, surging, dropping or steady. Call it before reading raw logs; read examples of a pattern with query_logs or search_around afterwards.",
            "logs",
            json!({
                "type": "object", "required": ["stream", "timeRange"],
                "properties": {
                    "stream": {"type": "string"}, "timeRange": time_range_schema(),
                    "filterSql": {"type": "string", "description": "Optional boolean SQL expression that narrows the logs; no SELECT/ORDER/LIMIT."},
                    "field": {"type": "string", "description": "The field that holds the message; found from the stream's schema when left out."},
                    "baseline": {"type": "string", "enum": ["previous", "week_before", "none"], "default": "previous", "description": "previous: the stretch of the same length just before; week_before: the same stretch a week earlier."},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 100, "default": 30}
                }, "additionalProperties": false
            }),
            vec!["Logs", "Patterns", "Investigation"],
        ),
        BuiltinToolKind::AnalyzeMetricTrend => (
            "Summarize how one PromQL query moved in a time range against a baseline stretch: its level before and after, the change point (when the move began, how far and which way), and which series and which label values contributed most to the change. Call it instead of reading raw series with query_metrics when the question is what changed.",
            "metrics",
            json!({
                "type": "object", "required": ["promql", "timeRange"],
                "properties": {
                    "promql": {"type": "string"}, "timeRange": time_range_schema(),
                    "baseline": {"type": "string", "enum": ["previous", "week_before"], "default": "previous", "description": "previous: the stretch of the same length just before; week_before: the same stretch a week earlier."},
                    "stepSeconds": {"type": "integer", "minimum": 1},
                    "splitBy": {"type": "array", "items": {"type": "string"}, "maxItems": 5, "description": "Labels to attribute the change to; every label of the series when left out."},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 50, "default": 10}
                }, "additionalProperties": false
            }),
            vec!["Metrics", "Trend", "Investigation"],
        ),
        BuiltinToolKind::CheckDataReadiness => (
            "Check how ready the organization's telemetry is for an investigation: for each stream, whether the service, environment and trace id are recorded (synonyms such as app or app_name included); whether a service is named the same way in logs, metrics and traces; whether environment values are spelled one way; and whether metric names carry their unit. Each finding comes with a suggested fix.",
            "streams",
            object_schema(json!({
                "streams": {"type": "array", "items": {"type": "string"}, "maxItems": 50, "description": "Streams to check; every stream when left out."},
                "timeRange": time_range_schema()
            })),
            vec!["Streams", "Data quality"],
        ),
        BuiltinToolKind::CorrelateSignals => (
            "Return bounded links between incidents, traces, logs, RUM, and profiles for a service or trace.",
            "correlation",
            json!({
                "type": "object", "required": ["timeRange"],
                "properties": {
                    "timeRange": time_range_schema(), "service": {"type": "string"},
                    "traceId": {"type": "string"}, "incidentId": {"type": "string"},
                    "limitPerSignal": {"type": "integer", "minimum": 1, "maximum": 100, "default": 20}
                }, "additionalProperties": false
            }),
            vec!["Correlation", "Investigation"],
        ),
        BuiltinToolKind::GetServiceTopology => (
            "Return a bounded service dependency graph for a time range.",
            "topology",
            json!({
                "type": "object", "required": ["timeRange"],
                "properties": {
                    "timeRange": time_range_schema(), "service": {"type": "string"},
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

    let tool = if matches!(
        kind,
        BuiltinToolKind::QueryLogs
            | BuiltinToolKind::QueryMetrics
            | BuiltinToolKind::ListStreams
            | BuiltinToolKind::GetStreamSchema
    ) {
        tool.pinned()
    } else {
        tool
    };

    // What a scheduled log inspection reads with: the schema of its stream, the logs, the values a
    // field takes, and the patterns of its logs. Nothing else is offered to unattended automation.
    if matches!(
        kind,
        BuiltinToolKind::QueryLogs
            | BuiltinToolKind::GetStreamSchema
            | BuiltinToolKind::SearchFieldValues
            | BuiltinToolKind::AnalyzeLogPatterns
    ) {
        tool.automation()
    } else {
        tool
    }
}
