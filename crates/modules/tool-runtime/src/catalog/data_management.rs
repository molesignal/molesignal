// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::{Value, json};

use super::{BuiltinToolKind, object_schema, open_output};
use crate::{RiskLevel, ToolSpec};

mod mutations;

pub(super) fn spec(kind: BuiltinToolKind) -> ToolSpec {
    if matches!(
        kind,
        BuiltinToolKind::CreateSavedView
            | BuiltinToolKind::UpdateSavedView
            | BuiltinToolKind::DeleteSavedView
            | BuiltinToolKind::CreateFunction
            | BuiltinToolKind::UpdateFunction
            | BuiltinToolKind::DeleteFunction
    ) {
        return mutations::spec(kind);
    }
    let (description, category, input, permissions, tags) = match kind {
        BuiltinToolKind::ListSavedViews => entry(
            "List saved SQL and PromQL views.",
            "saved_views",
            object_schema(
                json!({"pinned_only": {"type": "boolean", "default": false}, "limit": limit(500, 100)}),
            ),
            &["saved_views.read"],
            &["Search", "Saved views"],
        ),
        BuiltinToolKind::GetSavedView => id_entry(
            "Get one saved query view.",
            "saved_views",
            "view_id",
            &["saved_views.read"],
            &["Search", "Saved views"],
        ),
        BuiltinToolKind::ListSearchJobs => entry(
            "List bounded asynchronous search jobs.",
            "search_jobs",
            object_schema(json!({"limit": limit(200, 50)})),
            &["streams.query", "sys.telemetry.read"],
            &["Search", "Jobs"],
        ),
        BuiltinToolKind::GetSearchJob => id_entry(
            "Get asynchronous search-job status and result metadata.",
            "search_jobs",
            "job_id",
            &["streams.query", "sys.telemetry.read"],
            &["Search", "Jobs"],
        ),
        BuiltinToolKind::GetSearchJobResults => entry(
            "Get one bounded page of decoded asynchronous search-job results.",
            "search_jobs",
            json!({
                "type": "object", "required": ["job_id"],
                "properties": {
                    "job_id": {"type": "string", "minLength": 1},
                    "page": {"type": "integer", "minimum": 1, "default": 1},
                    "page_size": {"type": "integer", "minimum": 1, "maximum": 1000, "default": 100}
                }, "additionalProperties": false
            }),
            &["streams.query", "sys.telemetry.read"],
            &["Search", "Jobs", "Results"],
        ),
        BuiltinToolKind::SubmitSearchJob => entry(
            "Submit one durable asynchronous SQL or PromQL search job.",
            "search_jobs",
            submit_search_job_input(),
            &["streams.query", "sys.telemetry.read"],
            &["Search", "Jobs", "Write"],
        ),
        BuiltinToolKind::CancelSearchJob => search_job_operation(
            "Cancel one pending or running asynchronous search job.",
            &["Search", "Jobs", "Cancel"],
        ),
        BuiltinToolKind::RetrySearchJob => search_job_operation(
            "Retry one failed or cancelled asynchronous search job.",
            &["Search", "Jobs", "Retry"],
        ),
        BuiltinToolKind::DeleteSearchJob => search_job_operation(
            "Delete one asynchronous search job and its stored result.",
            &["Search", "Jobs", "Delete"],
        ),
        BuiltinToolKind::ListScheduledPipelines => entry(
            "List scheduled pipelines and recent run health.",
            "pipelines",
            object_schema(
                json!({"enabled_only": {"type": "boolean", "default": false}, "limit": limit(500, 100)}),
            ),
            &["pipelines.read"],
            &["Pipelines"],
        ),
        BuiltinToolKind::GetScheduledPipeline => id_entry(
            "Get one scheduled pipeline.",
            "pipelines",
            "pipeline_id",
            &["pipelines.read"],
            &["Pipelines"],
        ),
        BuiltinToolKind::ListPipelineRuns => entry(
            "List bounded execution history for one scheduled pipeline.",
            "pipelines",
            json!({"type": "object", "required": ["pipeline_id"], "properties": {
                "pipeline_id": {"type": "string"}, "before_micros": {"type": "integer"}, "limit": limit(200, 50)
            }, "additionalProperties": false}),
            &["pipelines.read"],
            &["Pipelines", "Runs"],
        ),
        BuiltinToolKind::EnableScheduledPipeline => pipeline_operation(
            "Enable one scheduled pipeline.",
            &["Pipelines", "Enable"],
            &["pipelines.edit"],
        ),
        BuiltinToolKind::DisableScheduledPipeline => pipeline_operation(
            "Disable one scheduled pipeline.",
            &["Pipelines", "Disable"],
            &["pipelines.edit"],
        ),
        BuiltinToolKind::DeleteScheduledPipeline => pipeline_operation(
            "Delete one scheduled pipeline.",
            &["Pipelines", "Delete"],
            &["pipelines.delete"],
        ),
        BuiltinToolKind::ListFunctions => entry(
            "List organization and built-in processing functions.",
            "functions",
            object_schema(json!({"limit": limit(500, 100)})),
            &["functions.read"],
            &["Functions", "Pipelines"],
        ),
        BuiltinToolKind::GetFunction => id_entry(
            "Get one processing function, including source.",
            "functions",
            "function_id",
            &["functions.read"],
            &["Functions", "Pipelines"],
        ),
        BuiltinToolKind::TestFunction => entry(
            "Run a bounded side-effect-free function test against supplied sample input.",
            "functions",
            json!({"type": "object", "required": ["language", "source", "input"], "properties": {
                "language": {"type": "string", "enum": ["vrl", "js"]}, "source": {"type": "string"}, "input": {}
            }, "additionalProperties": false}),
            &["functions.run"],
            &["Functions", "Test"],
        ),
        BuiltinToolKind::ListEnrichmentTables => entry(
            "List enrichment/KV tables.",
            "enrichment",
            object_schema(json!({"limit": limit(500, 100)})),
            &["functions.read"],
            &["Enrichment", "KV"],
        ),
        BuiltinToolKind::ListEnrichmentRows => entry(
            "List bounded rows from one enrichment table.",
            "enrichment",
            json!({"type": "object", "required": ["table"], "properties": {
                "table": {"type": "string"}, "limit": limit(1000, 200)
            }, "additionalProperties": false}),
            &["functions.read"],
            &["Enrichment", "KV"],
        ),
        BuiltinToolKind::GetEnrichmentValue => entry(
            "Get one enrichment-table row by key.",
            "enrichment",
            json!({"type": "object", "required": ["table", "key"], "properties": {
                "table": {"type": "string"}, "key": {"type": "string"}
            }, "additionalProperties": false}),
            &["functions.read"],
            &["Enrichment", "KV"],
        ),
        BuiltinToolKind::ListLogPatterns => entry(
            "List persisted log patterns.",
            "patterns",
            object_schema(json!({"limit": limit(500, 100)})),
            &["streams.read"],
            &["Logs", "Patterns"],
        ),
        BuiltinToolKind::GetLogPattern => id_entry(
            "Get one persisted log pattern.",
            "patterns",
            "pattern_id",
            &["streams.read"],
            &["Logs", "Patterns"],
        ),
        BuiltinToolKind::ListRegexPatterns => entry(
            "List reusable regular-expression patterns.",
            "patterns",
            object_schema(json!({"limit": limit(500, 100)})),
            &["org.settings.read"],
            &["Regex", "Patterns"],
        ),
        BuiltinToolKind::GetRegexPattern => id_entry(
            "Get one reusable regular-expression pattern.",
            "patterns",
            "pattern_id",
            &["org.settings.read"],
            &["Regex", "Patterns"],
        ),
        BuiltinToolKind::ListFieldMaskingRules => entry(
            "List field-masking rules without secret key material.",
            "field_masking",
            object_schema(json!({"limit": limit(500, 100)})),
            &["org.settings.read"],
            &["Masking", "Security"],
        ),
        BuiltinToolKind::GetEffectiveFieldMasking => entry(
            "Resolve effective field-masking rules for one stream.",
            "field_masking",
            json!({"type": "object", "required": ["stream_id"], "properties": {"stream_id": {"type": "string"}}, "additionalProperties": false}),
            &["org.settings.read"],
            &["Masking", "Security"],
        ),
        BuiltinToolKind::ListDataConnectors => entry(
            "List data connectors with credential-like config values redacted.",
            "connectors",
            object_schema(
                json!({"enabled_only": {"type": "boolean", "default": false}, "limit": limit(500, 100)}),
            ),
            &["pipelines.read"],
            &["Connectors", "Ingestion"],
        ),
        BuiltinToolKind::GetDataConnector => id_entry(
            "Get one data connector with credential-like config values redacted.",
            "connectors",
            "connector_id",
            &["pipelines.read"],
            &["Connectors", "Ingestion"],
        ),
        _ => unreachable!("data-management catalog received unrelated kind"),
    };
    let tool = ToolSpec::read(
        kind.name(),
        description,
        "data_management",
        category,
        input,
        open_output(),
        &permissions,
        &tags,
    );
    let tool = if matches!(
        kind,
        BuiltinToolKind::ListSearchJobs
            | BuiltinToolKind::GetSearchJob
            | BuiltinToolKind::GetSearchJobResults
            | BuiltinToolKind::SubmitSearchJob
            | BuiltinToolKind::CancelSearchJob
            | BuiltinToolKind::RetrySearchJob
            | BuiltinToolKind::DeleteSearchJob
    ) {
        tool.any_permission()
    } else {
        tool
    };
    match kind {
        BuiltinToolKind::SubmitSearchJob | BuiltinToolKind::CancelSearchJob => {
            tool.managed_mutation(RiskLevel::L1, false, false)
        }
        BuiltinToolKind::RetrySearchJob => tool.managed_mutation(RiskLevel::L2, false, false),
        BuiltinToolKind::DeleteSearchJob => tool.managed_mutation(RiskLevel::L3, true, true),
        BuiltinToolKind::EnableScheduledPipeline | BuiltinToolKind::DisableScheduledPipeline => {
            tool.managed_mutation(RiskLevel::L2, false, true)
        }
        BuiltinToolKind::DeleteScheduledPipeline => {
            tool.managed_mutation(RiskLevel::L3, true, true)
        }
        _ => tool,
    }
}

fn pipeline_operation(
    description: &'static str,
    tags: &[&'static str],
    permissions: &[&'static str],
) -> Entry {
    entry(
        description,
        "pipelines",
        proposal_id_input("pipeline_id"),
        permissions,
        tags,
    )
}

fn proposal_id_input(field: &str) -> serde_json::Value {
    json!({
        "type": "object", "required": [field, "reason", "impact"],
        "properties": {
            (field): {"type": "string", "minLength": 1},
            "reason": {"type": "string", "minLength": 1, "maxLength": 2000},
            "impact": {"type": "string", "minLength": 1, "maxLength": 2000},
            "expires_at_micros": {"type": "integer"}
        }, "additionalProperties": false
    })
}

type Entry = (
    &'static str,
    &'static str,
    serde_json::Value,
    Vec<&'static str>,
    Vec<&'static str>,
);

fn entry(
    description: &'static str,
    category: &'static str,
    input: serde_json::Value,
    permissions: &[&'static str],
    tags: &[&'static str],
) -> Entry {
    (
        description,
        category,
        input,
        permissions.to_vec(),
        tags.to_vec(),
    )
}

fn id_entry(
    description: &'static str,
    category: &'static str,
    field: &'static str,
    permissions: &[&'static str],
    tags: &[&'static str],
) -> Entry {
    entry(
        description,
        category,
        json!({"type": "object", "required": [field], "properties": {(field): {"type": "string"}}, "additionalProperties": false}),
        permissions,
        tags,
    )
}

fn limit(maximum: u32, default: u32) -> serde_json::Value {
    json!({"type": "integer", "minimum": 1, "maximum": maximum, "default": default})
}

fn search_job_operation(description: &'static str, tags: &[&'static str]) -> Entry {
    entry(
        description,
        "search_jobs",
        proposal_schema(
            vec!["job_id"],
            json!({"job_id": {"type": "string", "minLength": 1}}),
        ),
        &["streams.query", "sys.telemetry.read"],
        tags,
    )
}

fn proposal_schema(required: Vec<&str>, properties: Value) -> Value {
    let mut properties = properties.as_object().cloned().unwrap_or_default();
    properties.extend(
        json!({
            "reason": {"type": "string", "minLength": 1, "maxLength": 2000},
            "impact": {"type": "string", "minLength": 1, "maxLength": 2000},
            "expires_at_micros": {"type": "integer"}
        })
        .as_object()
        .cloned()
        .expect("static approval schema"),
    );
    json!({
        "type": "object",
        "required": required.into_iter().chain(["reason", "impact"]).collect::<Vec<_>>(),
        "properties": properties,
        "additionalProperties": false
    })
}

fn submit_search_job_input() -> Value {
    proposal_schema(
        vec!["language", "statement", "time_range"],
        json!({
            "language": {"type": "string", "enum": ["sql", "promql"]},
            "statement": {"type": "string", "minLength": 1},
            "time_range": {
                "type": "object", "required": ["start_micros", "end_micros"],
                "properties": {"start_micros": {"type": "integer"}, "end_micros": {"type": "integer"}},
                "additionalProperties": false
            },
            "stream": {
                "type": "object", "required": ["name", "stream_type"],
                "properties": {
                    "name": {"type": "string"},
                    "stream_type": {"type": "string", "enum": ["logs", "metrics", "traces", "profiles", "extend"]}
                }, "additionalProperties": false
            },
            "limit": {"type": "integer", "minimum": 1, "maximum": 100000},
            "federation_clusters": {"type": "array", "items": {"type": "string"}, "maxItems": 32},
            "ttl_secs": {"type": "integer", "minimum": 60, "maximum": 2592000}
        }),
    )
}
