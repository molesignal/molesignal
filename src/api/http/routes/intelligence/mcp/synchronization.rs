// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::{HashMap, HashSet};

use axum::{
    Extension, Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{require_license, runtime, schema};
use crate::{
    api::{AppState, http::routes::activity_audit},
    app::iam::IamContext,
    domain::iam::permission,
    intelligence::{
        model::RiskLevel,
        tool_control::{ManagedToolStatus, McpTool, ToolExecutionMode},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[derive(Debug, Clone, Deserialize, Serialize)]
struct RemoteMcpTool {
    name: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: String,
    #[serde(rename = "inputSchema", default = "empty_object_schema")]
    input_schema: Value,
    #[serde(rename = "outputSchema", default)]
    output_schema: Option<Value>,
    #[serde(default)]
    annotations: Value,
}

fn empty_object_schema() -> Value {
    json!({"type": "object", "properties": {}})
}

#[derive(Debug, Deserialize)]
pub(super) struct SyncRequest {
    #[serde(default)]
    selected_tools: Vec<String>,
}

#[permission("intelligence.manage")]
pub(super) async fn test_server(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    require_license(&state)?;
    let id = Id(id);
    let now = TimestampMicros::now();
    let result = discover_tools(&state, &ctx.org_id, &id).await;
    match result {
        Ok(tools) => {
            let server = state
                .intelligence
                .tool_control
                .update_mcp_server_runtime_status(
                    &ctx.org_id,
                    &id,
                    "healthy",
                    None,
                    Some(now),
                    None,
                )
                .await?;
            activity_audit::record(
                &state,
                &ctx,
                "intelligence.mcp_server.tested",
                "intelligence_mcp_server",
                &id.0,
                json!({"success": true, "discovered_tools": tools.len()}),
            )
            .await;
            Ok(Json(json!({
                "success": true,
                "server": server,
                "discovered_tools": tools,
            })))
        }
        Err(error) => {
            let message = safe_runtime_error(&error);
            let server = state
                .intelligence
                .tool_control
                .update_mcp_server_runtime_status(
                    &ctx.org_id,
                    &id,
                    "error",
                    Some(&message),
                    Some(now),
                    None,
                )
                .await?;
            activity_audit::record(
                &state,
                &ctx,
                "intelligence.mcp_server.tested",
                "intelligence_mcp_server",
                &id.0,
                json!({"success": false, "error": message}),
            )
            .await;
            Ok(Json(json!({
                "success": false,
                "server": server,
                "error": message,
                "discovered_tools": [],
            })))
        }
    }
}

#[permission("intelligence.manage")]
pub(super) async fn sync_server(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<SyncRequest>,
) -> Result<Json<Value>> {
    require_license(&state)?;
    let id = Id(id);
    let server = state
        .intelligence
        .tool_control
        .get_mcp_server(&ctx.org_id, &id)
        .await?;
    let discovered = discover_tools(&state, &ctx.org_id, &id).await?;
    let selection: HashSet<&str> = request.selected_tools.iter().map(String::as_str).collect();
    let existing = state
        .intelligence
        .tool_control
        .list_mcp_tools(&ctx.org_id, Some(&id))
        .await?
        .into_iter()
        .map(|tool| (tool.remote_name.clone(), tool))
        .collect::<HashMap<_, _>>();
    let now = TimestampMicros::now();
    let mut imported = Vec::new();
    for remote in discovered {
        if !selection.is_empty() && !selection.contains(remote.name.as_str()) {
            continue;
        }
        let minimum_risk = inferred_minimum_risk(&remote.annotations);
        let old = existing.get(&remote.name);
        let risk = old
            .map(|tool| tool.risk.max(minimum_risk))
            .unwrap_or(minimum_risk);
        let execution_mode = old
            .map(|tool| tool.execution_mode)
            .filter(|mode| mode.allowed_for_risk(risk))
            .unwrap_or_else(|| ToolExecutionMode::default_for_risk(risk));
        let enabled = old.is_some_and(|tool| tool.enabled);
        let remote_input_schema = remote.input_schema;
        let synchronized_schema = schema::synchronize(remote_input_schema.clone());
        let (input_schema, schema_hash, schema_dialect, unavailable_diagnostic) =
            match synchronized_schema {
                Ok(schema) => (schema.schema, schema.hash, schema.dialect, None),
                Err(error) => unavailable_schema(&remote_input_schema, &error),
            };
        let schema_available = unavailable_diagnostic.is_none();
        imported.push(McpTool {
            id: old.map(|tool| tool.id.clone()).unwrap_or_else(Id::new),
            org_id: ctx.org_id.clone(),
            server_id: id.clone(),
            remote_name: remote.name.clone(),
            name: old
                .map(|tool| tool.name.clone())
                .unwrap_or_else(|| registry_name(&server.name, &remote.name)),
            display_name: remote.title.clone().unwrap_or_else(|| remote.name.clone()),
            description: remote.description,
            input_schema,
            schema_hash,
            schema_dialect,
            schema_synced_at: now,
            unavailable_diagnostic,
            output_schema: remote.output_schema,
            minimum_risk,
            risk,
            execution_mode,
            capabilities: json!({
                "read_only": remote.annotations["readOnlyHint"].as_bool().unwrap_or(false),
                "supports_dry_run": false,
                "idempotent": remote.annotations["idempotentHint"].as_bool().unwrap_or(false),
                "streaming": false
            }),
            tags: vec!["MCP".into()],
            enabled,
            status: if !schema_available {
                ManagedToolStatus::Unavailable
            } else if enabled {
                ManagedToolStatus::Healthy
            } else {
                ManagedToolStatus::Disabled
            },
            version: None,
            last_synced_at: now,
            created_at: old.map(|tool| tool.created_at).unwrap_or(now),
            updated_at: now,
        });
    }
    let tools = state
        .intelligence
        .tool_control
        .upsert_mcp_tools(imported)
        .await?;
    let server = state
        .intelligence
        .tool_control
        .update_mcp_server_runtime_status(&ctx.org_id, &id, "healthy", None, Some(now), Some(now))
        .await?;
    activity_audit::record(
        &state,
        &ctx,
        "intelligence.mcp_server.synced",
        "intelligence_mcp_server",
        &id.0,
        json!({
            "imported_tools": tools.len(),
            "new_tools_default_enabled": false
        }),
    )
    .await;
    Ok(Json(json!({"server": server, "tools": tools})))
}

fn unavailable_schema(
    schema_value: &Value,
    error: &Error,
) -> (Value, String, String, Option<String>) {
    let diagnostic = error.to_string().chars().take(500).collect::<String>();
    let raw = crate::shared::contracts::canonical_json(schema_value);
    let hash =
        crate::shared::contracts::sha256_hex(crate::shared::contracts::canonical_json_bytes(&raw));
    let dialect = schema_value
        .get("$schema")
        .and_then(Value::as_str)
        .unwrap_or(schema::DIALECT_2020_12)
        .chars()
        .take(128)
        .collect();
    (raw, hash, dialect, Some(diagnostic))
}

fn inferred_minimum_risk(annotations: &Value) -> RiskLevel {
    if annotations["destructiveHint"].as_bool().unwrap_or(false) {
        RiskLevel::L4
    } else if annotations["readOnlyHint"].as_bool().unwrap_or(false) {
        RiskLevel::L0
    } else {
        RiskLevel::L2
    }
}

fn registry_name(server_name: &str, remote_name: &str) -> String {
    fn slug(value: &str) -> String {
        let mut result = String::new();
        let mut last_separator = false;
        for character in value.chars().flat_map(char::to_lowercase) {
            if character.is_ascii_alphanumeric() || character == '_' {
                result.push(character);
                last_separator = false;
            } else if !last_separator {
                result.push('_');
                last_separator = true;
            }
        }
        result.trim_matches('_').chars().take(60).collect()
    }
    format!("mcp__{}__{}", slug(server_name), slug(remote_name))
}

fn safe_runtime_error(error: &Error) -> String {
    match error {
        Error::InvalidArgument(message)
        | Error::Forbidden(message)
        | Error::Unavailable(message) => message.chars().take(500).collect(),
        _ => "MCP Server connection failed".into(),
    }
}

async fn discover_tools(
    state: &AppState,
    org_id: &Id,
    server_id: &Id,
) -> Result<Vec<RemoteMcpTool>> {
    let runtime = state
        .intelligence
        .tool_control
        .get_mcp_server_runtime(org_id, server_id)
        .await?;
    let result = runtime::rpc_request(&runtime, "tools/list", json!({})).await?;
    serde_json::from_value(result["tools"].clone())
        .map_err(|error| Error::invalid(format!("invalid MCP tools/list response: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_names_are_namespaced_and_stable() {
        assert_eq!(
            registry_name("Observability Server", "query.logs"),
            "mcp__observability_server__query_logs"
        );
    }

    #[test]
    fn destructive_remote_tools_have_an_l4_floor() {
        assert_eq!(
            inferred_minimum_risk(&json!({"destructiveHint": true})),
            RiskLevel::L4
        );
        assert_eq!(
            inferred_minimum_risk(&json!({"readOnlyHint": true})),
            RiskLevel::L0
        );
    }
}
