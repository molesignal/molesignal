// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::{ToolResult, catalog::BuiltinToolKind};

use super::{
    ToolRuntime,
    common::{bounded, json_result, parse_args, redact_credentials},
};
use crate::{
    app::iam::IamContext,
    domain::function::{Function, FunctionLanguage},
    infra::persistence::repositories::functions::precheck_compile,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

mod search_jobs;

pub(super) async fn execute(
    runtime: &ToolRuntime,
    auth: &IamContext,
    kind: BuiltinToolKind,
    arguments: Value,
) -> Result<ToolResult> {
    match kind {
        BuiltinToolKind::ListSavedViews => list_saved_views(runtime, auth, arguments).await,
        BuiltinToolKind::GetSavedView => get_saved_view(runtime, auth, arguments).await,
        BuiltinToolKind::ListSearchJobs => search_jobs::list(runtime, auth, arguments).await,
        BuiltinToolKind::GetSearchJob => search_jobs::get(runtime, auth, arguments).await,
        BuiltinToolKind::GetSearchJobResults => {
            search_jobs::results(runtime, auth, arguments).await
        }
        BuiltinToolKind::ListScheduledPipelines => list_pipelines(runtime, auth, arguments).await,
        BuiltinToolKind::GetScheduledPipeline => get_pipeline(runtime, auth, arguments).await,
        BuiltinToolKind::ListPipelineRuns => list_pipeline_runs(runtime, auth, arguments).await,
        BuiltinToolKind::ListFunctions => list_functions(runtime, auth, arguments).await,
        BuiltinToolKind::GetFunction => get_function(runtime, auth, arguments).await,
        BuiltinToolKind::TestFunction => test_function(runtime, auth, arguments).await,
        BuiltinToolKind::ListEnrichmentTables => {
            list_enrichment_tables(runtime, auth, arguments).await
        }
        BuiltinToolKind::ListEnrichmentRows => list_enrichment_rows(runtime, auth, arguments).await,
        BuiltinToolKind::GetEnrichmentValue => get_enrichment_value(runtime, auth, arguments).await,
        BuiltinToolKind::ListLogPatterns => list_log_patterns(runtime, auth, arguments).await,
        BuiltinToolKind::GetLogPattern => get_log_pattern(runtime, auth, arguments).await,
        BuiltinToolKind::ListRegexPatterns => list_regex_patterns(runtime, auth, arguments).await,
        BuiltinToolKind::GetRegexPattern => get_regex_pattern(runtime, auth, arguments).await,
        BuiltinToolKind::ListFieldMaskingRules => {
            list_field_masking(runtime, auth, arguments).await
        }
        BuiltinToolKind::GetEffectiveFieldMasking => {
            effective_field_masking(runtime, auth, arguments).await
        }
        BuiltinToolKind::ListDataConnectors => list_connectors(runtime, auth, arguments).await,
        BuiltinToolKind::GetDataConnector => get_connector(runtime, auth, arguments).await,
        _ => unreachable!("data-management handler received unrelated tool"),
    }
}

async fn list_saved_views(
    runtime: &ToolRuntime,
    auth: &IamContext,
    value: Value,
) -> Result<ToolResult> {
    let args: ListArgs = parse_args(value)?;
    let views = runtime
        .data
        .saved_views
        .list(&auth.org_id, args.pinned_only)
        .await?;
    json_result(&bounded(views, args.limit, 500))
}

async fn get_saved_view(
    runtime: &ToolRuntime,
    auth: &IamContext,
    value: Value,
) -> Result<ToolResult> {
    let args: IdArgs = parse_args(value)?;
    let view = runtime
        .data
        .saved_views
        .get_by_id(&Id(args.required("view_id")?))
        .await?;
    ensure_org(&view.org_id, auth)?;
    json_result(&view)
}

async fn list_pipelines(
    runtime: &ToolRuntime,
    auth: &IamContext,
    value: Value,
) -> Result<ToolResult> {
    let args: ListArgs = parse_args(value)?;
    let mut pipelines = runtime.data.scheduled_pipelines.list(&auth.org_id).await?;
    if args.enabled_only {
        pipelines.retain(|pipeline| pipeline.enabled);
    }
    json_result(&bounded(pipelines, args.limit, 500))
}

async fn get_pipeline(
    runtime: &ToolRuntime,
    auth: &IamContext,
    value: Value,
) -> Result<ToolResult> {
    let args: IdArgs = parse_args(value)?;
    json_result(
        &runtime
            .data
            .scheduled_pipelines
            .get(&auth.org_id, &Id(args.required("pipeline_id")?))
            .await?,
    )
}

async fn list_pipeline_runs(
    runtime: &ToolRuntime,
    auth: &IamContext,
    value: Value,
) -> Result<ToolResult> {
    let args: PipelineRunArgs = parse_args(value)?;
    let pipeline_id = Id(args.pipeline_id);
    runtime
        .data
        .scheduled_pipelines
        .get(&auth.org_id, &pipeline_id)
        .await?;
    json_result(
        &runtime
            .data
            .pipeline_runs
            .list(
                &auth.org_id,
                &pipeline_id,
                args.limit.unwrap_or(50).clamp(1, 200) as i64,
                args.before_micros,
            )
            .await?,
    )
}

async fn list_functions(
    runtime: &ToolRuntime,
    auth: &IamContext,
    value: Value,
) -> Result<ToolResult> {
    let args: ListArgs = parse_args(value)?;
    json_result(&bounded(
        runtime.data.functions.list(&auth.org_id).await?,
        args.limit,
        500,
    ))
}

async fn get_function(
    runtime: &ToolRuntime,
    auth: &IamContext,
    value: Value,
) -> Result<ToolResult> {
    let args: IdArgs = parse_args(value)?;
    let function = runtime
        .data
        .functions
        .get_by_id(&Id(args.required("function_id")?))
        .await?;
    if function.org_id.as_str() != "__builtin__" {
        ensure_org(&function.org_id, auth)?;
    }
    json_result(&function)
}

async fn test_function(
    runtime: &ToolRuntime,
    auth: &IamContext,
    value: Value,
) -> Result<ToolResult> {
    let args: TestFunctionArgs = parse_args(value)?;
    if args.language == FunctionLanguage::Llm {
        return Err(Error::invalid(
            "LLM functions cannot be executed by test_function",
        ));
    }
    precheck_compile(
        args.language,
        &args.source,
        runtime.data.functions_js_runtime_enabled,
    )?;
    let now = TimestampMicros::now();
    let function = Function {
        id: Id::from_string("tool-test"),
        org_id: auth.org_id.clone(),
        name: "tool-test".into(),
        language: args.language,
        source: args.source,
        params_schema: json!({}),
        created_at: now,
        updated_at: now,
    };
    let mut output = args.input;
    runtime
        .data
        .function_executor
        .run(&function, &mut output)
        .await?;
    Ok(ToolResult::json(json!({"output": output})))
}

async fn list_enrichment_tables(
    runtime: &ToolRuntime,
    auth: &IamContext,
    value: Value,
) -> Result<ToolResult> {
    let args: ListArgs = parse_args(value)?;
    json_result(&bounded(
        runtime.data.enrichment.list_tables(&auth.org_id).await?,
        args.limit,
        500,
    ))
}

async fn list_enrichment_rows(
    runtime: &ToolRuntime,
    auth: &IamContext,
    value: Value,
) -> Result<ToolResult> {
    let args: EnrichmentArgs = parse_args(value)?;
    let rows = runtime
        .data
        .enrichment
        .list_table(&auth.org_id, &args.table)
        .await?;
    json_result(&bounded(rows, args.limit, 1000))
}

async fn get_enrichment_value(
    runtime: &ToolRuntime,
    auth: &IamContext,
    value: Value,
) -> Result<ToolResult> {
    let args: EnrichmentArgs = parse_args(value)?;
    let key = args
        .key
        .filter(|key| !key.trim().is_empty())
        .ok_or_else(|| Error::invalid("key is required"))?;
    let row = runtime
        .data
        .enrichment
        .list_table(&auth.org_id, &args.table)
        .await?
        .into_iter()
        .find(|row| row.key == key)
        .ok_or_else(|| Error::not_found("enrichment value not found"))?;
    json_result(&row)
}

async fn list_log_patterns(
    runtime: &ToolRuntime,
    auth: &IamContext,
    value: Value,
) -> Result<ToolResult> {
    let args: ListArgs = parse_args(value)?;
    json_result(&bounded(
        runtime.data.log_patterns.list(&auth.org_id).await?,
        args.limit,
        500,
    ))
}

async fn get_log_pattern(
    runtime: &ToolRuntime,
    auth: &IamContext,
    value: Value,
) -> Result<ToolResult> {
    let args: IdArgs = parse_args(value)?;
    json_result(
        &runtime
            .data
            .log_patterns
            .get(&auth.org_id, &Id(args.required("pattern_id")?))
            .await?,
    )
}

async fn list_regex_patterns(
    runtime: &ToolRuntime,
    auth: &IamContext,
    value: Value,
) -> Result<ToolResult> {
    let args: ListArgs = parse_args(value)?;
    json_result(&bounded(
        runtime.data.regex_patterns.list(&auth.org_id).await?,
        args.limit,
        500,
    ))
}

async fn get_regex_pattern(
    runtime: &ToolRuntime,
    auth: &IamContext,
    value: Value,
) -> Result<ToolResult> {
    let args: IdArgs = parse_args(value)?;
    let id = args.required("pattern_id")?;
    let pattern = runtime
        .data
        .regex_patterns
        .list(&auth.org_id)
        .await?
        .into_iter()
        .find(|pattern| pattern.id.as_str() == id)
        .ok_or_else(|| Error::not_found("regex pattern not found"))?;
    json_result(&pattern)
}

async fn list_field_masking(
    runtime: &ToolRuntime,
    auth: &IamContext,
    value: Value,
) -> Result<ToolResult> {
    let args: ListArgs = parse_args(value)?;
    let rules = runtime.data.field_masking_rules.list(&auth.org_id).await?;
    json_result(&bounded(rules, args.limit, 500))
}

async fn effective_field_masking(
    runtime: &ToolRuntime,
    auth: &IamContext,
    value: Value,
) -> Result<ToolResult> {
    let args: IdArgs = parse_args(value)?;
    json_result(
        &runtime
            .data
            .field_masking
            .effective_for_stream(&auth.org_id, &Id(args.required("stream_id")?))
            .await?,
    )
}

async fn list_connectors(
    runtime: &ToolRuntime,
    auth: &IamContext,
    value: Value,
) -> Result<ToolResult> {
    let args: ListArgs = parse_args(value)?;
    let mut connectors = runtime.data.connectors.list(&auth.org_id).await?;
    if args.enabled_only {
        connectors.retain(|connector| connector.enabled);
    }
    connector_result(bounded(connectors, args.limit, 500))
}

async fn get_connector(
    runtime: &ToolRuntime,
    auth: &IamContext,
    value: Value,
) -> Result<ToolResult> {
    let args: IdArgs = parse_args(value)?;
    connector_result(vec![
        runtime
            .data
            .connectors
            .get(&auth.org_id, &Id(args.required("connector_id")?))
            .await?,
    ])
}

fn connector_result(connectors: Vec<crate::infra::connectors::Connector>) -> Result<ToolResult> {
    Ok(ToolResult::json(
        json!({"connectors": connectors.into_iter().map(|connector| json!({
        "id": connector.id, "name": connector.name, "kind": connector.kind,
        "config": redact_credentials(&connector.config_json), "enabled": connector.enabled,
        "last_run_at": connector.last_run_at, "created_at": connector.created_at, "updated_at": connector.updated_at,
    })).collect::<Vec<_>>() }),
    ))
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListArgs {
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    pinned_only: bool,
    #[serde(default)]
    enabled_only: bool,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdArgs {
    view_id: Option<String>,
    pipeline_id: Option<String>,
    function_id: Option<String>,
    pattern_id: Option<String>,
    stream_id: Option<String>,
    connector_id: Option<String>,
}

impl IdArgs {
    fn required(&self, field: &str) -> Result<String> {
        let value = match field {
            "view_id" => &self.view_id,
            "pipeline_id" => &self.pipeline_id,
            "function_id" => &self.function_id,
            "pattern_id" => &self.pattern_id,
            "stream_id" => &self.stream_id,
            "connector_id" => &self.connector_id,
            _ => unreachable!(),
        };
        value
            .as_ref()
            .filter(|value| !value.trim().is_empty())
            .cloned()
            .ok_or_else(|| Error::invalid(format!("{field} is required")))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PipelineRunArgs {
    pipeline_id: String,
    #[serde(default)]
    before_micros: Option<i64>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TestFunctionArgs {
    language: FunctionLanguage,
    source: String,
    input: Value,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct EnrichmentArgs {
    table: String,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

fn ensure_org(org_id: &Id, auth: &IamContext) -> Result<()> {
    if org_id == &auth.org_id {
        Ok(())
    } else {
        Err(Error::not_found("resource not found"))
    }
}
