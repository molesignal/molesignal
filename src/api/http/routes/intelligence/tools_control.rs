// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Intelligence 工具发现、策略、测试、依赖与调用记录 API。

use std::{collections::HashMap, time::Instant};

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    routing::{get, post, put},
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    mcp::validate_schema,
    tool_dispatcher::{RealToolDispatcher, redact_sensitive_value},
    toolsets,
};
use crate::{
    api::{AppState, http::routes::activity_audit},
    app::iam::IamContext,
    domain::iam::permission,
    intelligence::{
        FEATURE,
        model::{RiskLevel, ToolCallRecord},
        tool_control::{
            ManagedToolStatus, McpServer, McpTool, ToolExecutionMode, ToolPolicy,
            ToolPolicyDefaults,
        },
        tools::{
            BuiltinToolKind, ToolAuthContext, ToolCall, ToolDispatcher, builtin_tools,
            is_builtin_tool,
        },
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const DEFAULT_TIMEOUT_MS: i64 = 30_000;
const DEFAULT_MAX_CALLS: i32 = 32;
const DEFAULT_MAX_RESPONSE_BYTES: i64 = 1_048_576;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/intelligence/tools", get(list_tools))
        .route(
            "/intelligence/tools/policies",
            get(get_policy_defaults).put(update_policy_defaults),
        )
        .route("/intelligence/tools/{id}", get(get_tool))
        .route("/intelligence/tools/{id}/policy", put(update_tool_policy))
        .route("/intelligence/tools/{id}/test", post(test_tool))
        .route("/intelligence/tools/{id}/enable", post(enable_tool))
        .route("/intelligence/tools/{id}/disable", post(disable_tool))
        .route(
            "/intelligence/tools/{id}/dependencies",
            get(tool_dependencies),
        )
        .route("/intelligence/tools/{id}/calls", get(tool_calls))
}

fn require_license(state: &AppState) -> Result<()> {
    if !state.platform.license.has_feature(FEATURE) {
        return Err(Error::forbidden(format!("{FEATURE} feature not licensed")));
    }
    Ok(())
}

#[permission("intelligence.use")]
async fn list_tools(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    require_license(&state)?;
    let tools = managed_tools(&state, &ctx).await?;
    let servers = state
        .intelligence
        .tool_control
        .list_mcp_servers(&ctx.org_id)
        .await?;
    Ok(Json(json!({
        "tools": tools,
        "dynamic_http": false,
        "shell": false,
        "browser": false,
        "open_mcp": servers.iter().any(|server| server.enabled),
        "mcp_servers": {
            "total": servers.len(),
            "healthy": servers.iter().filter(|server| server.status == "healthy").count(),
            "unhealthy": servers
                .iter()
                .filter(|server| server.enabled && server.status != "healthy")
                .count()
        }
    })))
}

#[permission("intelligence.use")]
async fn get_tool(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    require_license(&state)?;
    let tools = managed_tools(&state, &ctx).await?;
    let tool = tools
        .into_iter()
        .find(|tool| {
            tool["id"].as_str() == Some(id.as_str()) || tool["name"].as_str() == Some(id.as_str())
        })
        .ok_or_else(|| Error::not_found(format!("tool `{id}` not found")))?;
    let dependencies =
        dependency_value(&state, &ctx.org_id, tool["name"].as_str().unwrap_or("")).await?;
    Ok(Json(json!({"tool": tool, "dependencies": dependencies})))
}

async fn managed_tools(state: &AppState, ctx: &IamContext) -> Result<Vec<Value>> {
    let resolution = toolsets::resolve_toolsets(state, &ctx.org_id).await?;
    let policies = state
        .intelligence
        .tool_control
        .list_policies(&ctx.org_id)
        .await?
        .into_iter()
        .map(|policy| (policy.tool_name.clone(), policy))
        .collect::<HashMap<_, _>>();
    let calls = state
        .intelligence
        .repository
        .list_tool_calls(&ctx.org_id, None, 500)
        .await?;
    let servers = state
        .intelligence
        .tool_control
        .list_mcp_servers(&ctx.org_id)
        .await?
        .into_iter()
        .map(|server| (server.id.0.clone(), server))
        .collect::<HashMap<_, _>>();
    let mut result = Vec::new();
    for tool in builtin_tools() {
        let kind = BuiltinToolKind::from_name(&tool.name)
            .ok_or_else(|| Error::internal("builtin tool registry is inconsistent"))?;
        let presentation = kind.presentation();
        let policy = policies.get(&tool.name);
        let execution_mode = resolution.execution_mode_for_builtin(&tool.name);
        let enabled = policy.is_none_or(|value| value.enabled)
            && execution_mode != ToolExecutionMode::Disabled;
        let available_to_agent = resolution.builtin_enabled(&tool.name);
        let stats = call_stats(&calls, &tool.name);
        result.push(json!({
            "id": tool.name,
            "name": tool.name,
            "display_name": presentation.display_name,
            "description": presentation.description_zh,
            "technical_description": tool.description,
            "domain": presentation.domain,
            "category": presentation.category,
            "source": {"kind": "builtin", "label": "builtin"},
            "risk": tool.risk,
            "execution_mode": execution_mode,
            "enabled": enabled,
            "available_to_agent": available_to_agent,
            "status": if enabled { "healthy" } else { "disabled" },
            "input_schema": tool.input_schema,
            "output_schema": presentation.output_schema,
            "capabilities": presentation.capabilities,
            "limits": {
                "timeout_ms": policy.map(|value| value.timeout_ms).unwrap_or(DEFAULT_TIMEOUT_MS),
                "max_calls_per_run": policy.map(|value| value.max_calls_per_run).unwrap_or(DEFAULT_MAX_CALLS),
                "max_response_bytes": policy.map(|value| value.max_response_bytes).unwrap_or(DEFAULT_MAX_RESPONSE_BYTES)
            },
            "environment_overrides": policy.map(|value| value.environment_overrides.clone()).unwrap_or_else(|| json!({})),
            "tags": presentation.tags,
            "access": tool.access,
            "statistics": stats
        }));
    }
    let mcp_tools = state
        .intelligence
        .tool_control
        .list_mcp_tools(&ctx.org_id, None)
        .await?;
    for tool in mcp_tools {
        let server = servers.get(&tool.server_id.0);
        let policy = policies.get(&tool.name);
        let execution_mode = resolution.execution_mode_for_mcp(&tool);
        let available_to_agent = resolution.mcp_tool(&tool.name).is_some();
        let stats = call_stats(&calls, &tool.name);
        result.push(mcp_tool_value(
            &tool,
            server,
            policy,
            execution_mode,
            available_to_agent,
            stats,
        ));
    }
    result.sort_by(|left, right| {
        left["domain"]
            .as_str()
            .unwrap_or_default()
            .cmp(right["domain"].as_str().unwrap_or_default())
            .then_with(|| {
                left["name"]
                    .as_str()
                    .unwrap_or_default()
                    .cmp(right["name"].as_str().unwrap_or_default())
            })
    });
    Ok(result)
}

fn mcp_tool_value(
    tool: &McpTool,
    server: Option<&McpServer>,
    policy: Option<&ToolPolicy>,
    execution_mode: ToolExecutionMode,
    available_to_agent: bool,
    statistics: Value,
) -> Value {
    let read_only = tool.capabilities["read_only"].as_bool().unwrap_or(false);
    let configured_enabled = tool.enabled
        && policy.is_none_or(|policy| policy.enabled)
        && execution_mode != ToolExecutionMode::Disabled;
    let server_healthy = server.is_some_and(|server| server.enabled && server.status == "healthy");
    json!({
        "id": tool.id,
        "name": tool.name,
        "remote_name": tool.remote_name,
        "display_name": tool.display_name,
        "description": tool.description,
        "technical_description": tool.description,
        "domain": domain_for_mcp_tool(tool),
        "category": tool.tags.first().cloned().unwrap_or_else(|| "MCP".into()),
        "source": {
            "kind": "mcp",
            "label": server.map(|value| value.name.as_str()).unwrap_or("MCP"),
            "server_id": tool.server_id,
            "server_name": server.map(|value| value.name.as_str())
        },
        "risk": tool.risk,
        "minimum_risk": tool.minimum_risk,
        "execution_mode": execution_mode,
        "enabled": configured_enabled,
        "available_to_agent": available_to_agent,
        "status": if !configured_enabled { "disabled" } else if server_healthy { "healthy" } else { "unavailable" },
        "input_schema": tool.input_schema,
        "output_schema": tool.output_schema,
        "capabilities": tool.capabilities,
        "limits": {
            "timeout_ms": policy.map(|value| value.timeout_ms).unwrap_or_else(|| server.map(|value| value.timeout_ms).unwrap_or(DEFAULT_TIMEOUT_MS)),
            "max_calls_per_run": policy.map(|value| value.max_calls_per_run).unwrap_or(DEFAULT_MAX_CALLS),
            "max_response_bytes": policy.map(|value| value.max_response_bytes).unwrap_or_else(|| server.map(|value| value.max_response_bytes).unwrap_or(DEFAULT_MAX_RESPONSE_BYTES))
        },
        "environment_overrides": policy.map(|value| value.environment_overrides.clone()).unwrap_or_else(|| json!({})),
        "tags": tool.tags,
        "access": if read_only { "read_only" } else { "creates_approval_request" },
        "last_synced_at": tool.last_synced_at,
        "version": tool.version,
        "statistics": statistics
    })
}

fn domain_for_mcp_tool(tool: &McpTool) -> &'static str {
    let text =
        format!("{} {} {}", tool.name, tool.description, tool.tags.join(" ")).to_ascii_lowercase();
    if text.contains("alert") || text.contains("oncall") || text.contains("on-call") {
        "alerts_on_call"
    } else if text.contains("dashboard") || text.contains("report") {
        "dashboard_reports"
    } else if text.contains("notify") || text.contains("message") {
        "notify"
    } else if text.contains("knowledge") || text.contains("context") || text.contains("search") {
        "knowledge_context"
    } else if text.contains("admin") || text.contains("organization") || text.contains("user") {
        "administration"
    } else if text.contains("create")
        || text.contains("update")
        || text.contains("delete")
        || text.contains("execute")
        || text.contains("restart")
    {
        "automation"
    } else {
        "observability"
    }
}

fn call_stats(calls: &[ToolCallRecord], tool_name: &str) -> Value {
    let cutoff = TimestampMicros::now().0 - 86_400_000_000;
    let mut matching = calls
        .iter()
        .filter(|call| call.tool_name == tool_name && call.created_at.0 >= cutoff)
        .collect::<Vec<_>>();
    let total = matching.len();
    let successes = matching
        .iter()
        .filter(|call| call.status == "success")
        .count();
    let success_rate = if total == 0 {
        None
    } else {
        Some((successes as f64 / total as f64) * 100.0)
    };
    matching.sort_by_key(|call| call.duration_ms);
    let p95_ms = (!matching.is_empty()).then(|| {
        let index = ((matching.len() as f64 * 0.95).ceil() as usize)
            .saturating_sub(1)
            .min(matching.len() - 1);
        matching[index].duration_ms
    });
    let last_called_at = matching.iter().map(|call| call.created_at.0).max();
    let last_error = calls
        .iter()
        .find(|call| {
            call.tool_name == tool_name && call.created_at.0 >= cutoff && call.status != "success"
        })
        .and_then(|call| call.error.as_deref());
    json!({
        "calls_24h": total,
        "success_rate": success_rate,
        "p95_ms": p95_ms,
        "last_called_at": last_called_at,
        "last_error": last_error
    })
}

fn default_mode_from_value(value: &Value, risk: RiskLevel) -> ToolExecutionMode {
    let key = format!("{risk:?}").to_ascii_lowercase();
    value
        .get(&key)
        .and_then(Value::as_str)
        .and_then(parse_execution_mode)
        .filter(|mode| mode.allowed_for_risk(risk))
        .unwrap_or_else(|| ToolExecutionMode::default_for_risk(risk))
}

fn parse_execution_mode(value: &str) -> Option<ToolExecutionMode> {
    match value {
        "automatic" => Some(ToolExecutionMode::Automatic),
        "confirmation" => Some(ToolExecutionMode::Confirmation),
        "single_approval" => Some(ToolExecutionMode::SingleApproval),
        "dual_approval" => Some(ToolExecutionMode::DualApproval),
        "disabled" => Some(ToolExecutionMode::Disabled),
        _ => None,
    }
}

#[derive(Debug, Deserialize)]
struct ToolPolicyRequest {
    enabled: Option<bool>,
    execution_mode: Option<ToolExecutionMode>,
    risk: Option<RiskLevel>,
    #[serde(default)]
    environment_overrides: Option<Value>,
    timeout_ms: Option<i64>,
    max_calls_per_run: Option<i32>,
    max_response_bytes: Option<i64>,
}

#[permission("intelligence.manage")]
async fn update_tool_policy(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<ToolPolicyRequest>,
) -> Result<Json<Value>> {
    require_license(&state)?;
    let updated = apply_tool_policy(&state, &ctx, &id, request).await?;
    activity_audit::record(
        &state,
        &ctx,
        "intelligence.tool.policy_updated",
        "intelligence_tool",
        &id,
        json!({
            "enabled": updated["enabled"],
            "risk": updated["risk"],
            "execution_mode": updated["execution_mode"]
        }),
    )
    .await;
    Ok(Json(updated))
}

async fn apply_tool_policy(
    state: &AppState,
    ctx: &IamContext,
    id: &str,
    request: ToolPolicyRequest,
) -> Result<Value> {
    if let Some(kind) = BuiltinToolKind::from_name(id) {
        let definition = builtin_tools()
            .into_iter()
            .find(|tool| tool.name == id)
            .ok_or_else(|| Error::internal("builtin tool registry is inconsistent"))?;
        if request.risk.is_some_and(|risk| risk != definition.risk) {
            return Err(Error::invalid(
                "the risk level of a built-in tool cannot be changed",
            ));
        }
        let defaults = state
            .intelligence
            .tool_control
            .get_policy_defaults(&ctx.org_id)
            .await?
            .unwrap_or_else(|| {
                ToolPolicyDefaults::system_defaults(ctx.org_id.clone(), ctx.user_id.clone())
            });
        let existing = state
            .intelligence
            .tool_control
            .get_policy(&ctx.org_id, id)
            .await?;
        let now = TimestampMicros::now();
        let execution_mode = request
            .execution_mode
            .or_else(|| existing.as_ref().map(|policy| policy.execution_mode))
            .unwrap_or_else(|| default_mode_from_value(&defaults.risk_modes, definition.risk));
        if !execution_mode.allowed_for_risk(definition.risk) {
            return Err(Error::invalid(format!(
                "{execution_mode:?} is below the hard policy floor for {:?}",
                definition.risk
            )));
        }
        let environment_overrides = request
            .environment_overrides
            .or_else(|| {
                existing
                    .as_ref()
                    .map(|policy| policy.environment_overrides.clone())
            })
            .unwrap_or_else(|| json!({}));
        validate_tool_environment_overrides(&environment_overrides, definition.risk)?;
        let policy = ToolPolicy {
            org_id: ctx.org_id.clone(),
            tool_name: kind.name().into(),
            enabled: request
                .enabled
                .or_else(|| existing.as_ref().map(|policy| policy.enabled))
                .unwrap_or(true)
                && execution_mode != ToolExecutionMode::Disabled,
            execution_mode,
            environment_overrides,
            timeout_ms: checked_i64(
                request
                    .timeout_ms
                    .or_else(|| existing.as_ref().map(|policy| policy.timeout_ms))
                    .unwrap_or(DEFAULT_TIMEOUT_MS),
                1_000,
                120_000,
                "timeout_ms",
            )?,
            max_calls_per_run: checked_i32(
                request
                    .max_calls_per_run
                    .or_else(|| existing.as_ref().map(|policy| policy.max_calls_per_run))
                    .unwrap_or(DEFAULT_MAX_CALLS),
                1,
                256,
                "max_calls_per_run",
            )?,
            max_response_bytes: checked_i64(
                request
                    .max_response_bytes
                    .or_else(|| existing.as_ref().map(|policy| policy.max_response_bytes))
                    .unwrap_or(DEFAULT_MAX_RESPONSE_BYTES),
                1_024,
                16 * 1_048_576,
                "max_response_bytes",
            )?,
            updated_by: ctx.user_id.clone(),
            created_at: existing
                .as_ref()
                .map(|policy| policy.created_at)
                .unwrap_or(now),
            updated_at: now,
        };
        let saved = state
            .intelligence
            .tool_control
            .upsert_policy(policy)
            .await?;
        return Ok(json!({
            "id": id,
            "name": id,
            "risk": definition.risk,
            "enabled": saved.enabled,
            "execution_mode": saved.execution_mode,
            "environment_overrides": saved.environment_overrides,
            "limits": {
                "timeout_ms": saved.timeout_ms,
                "max_calls_per_run": saved.max_calls_per_run,
                "max_response_bytes": saved.max_response_bytes
            }
        }));
    }
    let tools = state
        .intelligence
        .tool_control
        .list_mcp_tools(&ctx.org_id, None)
        .await?;
    let mut tool = tools
        .into_iter()
        .find(|tool| tool.id.0 == id || tool.name == id)
        .ok_or_else(|| Error::not_found(format!("tool `{id}` not found")))?;
    let existing_policy = state
        .intelligence
        .tool_control
        .get_policy(&ctx.org_id, &tool.name)
        .await?;
    let server = state
        .intelligence
        .tool_control
        .get_mcp_server(&ctx.org_id, &tool.server_id)
        .await?;
    let risk = request.risk.unwrap_or(tool.risk);
    if risk < tool.minimum_risk {
        return Err(Error::invalid(format!(
            "risk cannot be lower than the MCP tool floor {:?}",
            tool.minimum_risk
        )));
    }
    let execution_mode = request.execution_mode.unwrap_or(tool.execution_mode);
    if !execution_mode.allowed_for_risk(risk) {
        return Err(Error::invalid(format!(
            "{execution_mode:?} is below the hard policy floor for {risk:?}"
        )));
    }
    let environment_overrides = request
        .environment_overrides
        .or_else(|| {
            existing_policy
                .as_ref()
                .map(|policy| policy.environment_overrides.clone())
        })
        .unwrap_or_else(|| json!({}));
    validate_tool_environment_overrides(&environment_overrides, risk)?;
    let enabled = request
        .enabled
        .or_else(|| existing_policy.as_ref().map(|policy| policy.enabled))
        .unwrap_or(tool.enabled)
        && execution_mode != ToolExecutionMode::Disabled;
    let now = TimestampMicros::now();
    let policy = ToolPolicy {
        org_id: ctx.org_id.clone(),
        tool_name: tool.name.clone(),
        enabled,
        execution_mode,
        environment_overrides,
        timeout_ms: checked_i64(
            request
                .timeout_ms
                .or_else(|| existing_policy.as_ref().map(|policy| policy.timeout_ms))
                .unwrap_or(server.timeout_ms),
            1_000,
            120_000,
            "timeout_ms",
        )?,
        max_calls_per_run: checked_i32(
            request
                .max_calls_per_run
                .or_else(|| {
                    existing_policy
                        .as_ref()
                        .map(|policy| policy.max_calls_per_run)
                })
                .unwrap_or(DEFAULT_MAX_CALLS),
            1,
            256,
            "max_calls_per_run",
        )?,
        max_response_bytes: checked_i64(
            request
                .max_response_bytes
                .or_else(|| {
                    existing_policy
                        .as_ref()
                        .map(|policy| policy.max_response_bytes)
                })
                .unwrap_or(server.max_response_bytes),
            1_024,
            16 * 1_048_576,
            "max_response_bytes",
        )?,
        updated_by: ctx.user_id.clone(),
        created_at: existing_policy
            .as_ref()
            .map(|policy| policy.created_at)
            .unwrap_or(now),
        updated_at: now,
    };
    tool.risk = risk;
    tool.execution_mode = execution_mode;
    tool.enabled = enabled;
    tool.status = if tool.enabled {
        ManagedToolStatus::Healthy
    } else {
        ManagedToolStatus::Disabled
    };
    tool.updated_at = now;
    let saved = state
        .intelligence
        .tool_control
        .update_mcp_tool_policy(tool)
        .await?;
    let saved_policy = state
        .intelligence
        .tool_control
        .upsert_policy(policy)
        .await?;
    Ok(json!({
        "id": saved.id,
        "name": saved.name,
        "risk": saved.risk,
        "minimum_risk": saved.minimum_risk,
        "enabled": saved.enabled,
        "execution_mode": saved.execution_mode,
        "environment_overrides": saved_policy.environment_overrides,
        "limits": {
            "timeout_ms": saved_policy.timeout_ms,
            "max_calls_per_run": saved_policy.max_calls_per_run,
            "max_response_bytes": saved_policy.max_response_bytes
        }
    }))
}

fn checked_i64(value: i64, minimum: i64, maximum: i64, name: &str) -> Result<i64> {
    if !(minimum..=maximum).contains(&value) {
        return Err(Error::invalid(format!(
            "{name} must be between {minimum} and {maximum}"
        )));
    }
    Ok(value)
}

fn checked_i32(value: i32, minimum: i32, maximum: i32, name: &str) -> Result<i32> {
    if !(minimum..=maximum).contains(&value) {
        return Err(Error::invalid(format!(
            "{name} must be between {minimum} and {maximum}"
        )));
    }
    Ok(value)
}

#[derive(Debug, Deserialize)]
struct DisableRequest {
    #[serde(default)]
    force: bool,
}

#[permission("intelligence.manage")]
async fn enable_tool(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    require_license(&state)?;
    let updated = apply_tool_policy(
        &state,
        &ctx,
        &id,
        ToolPolicyRequest {
            enabled: Some(true),
            execution_mode: None,
            risk: None,
            environment_overrides: None,
            timeout_ms: None,
            max_calls_per_run: None,
            max_response_bytes: None,
        },
    )
    .await?;
    activity_audit::record(
        &state,
        &ctx,
        "intelligence.tool.enabled",
        "intelligence_tool",
        &id,
        json!({}),
    )
    .await;
    Ok(Json(updated))
}

#[permission("intelligence.manage")]
async fn disable_tool(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<DisableRequest>,
) -> Result<Json<Value>> {
    require_license(&state)?;
    let name = resolve_tool_name(&state, &ctx.org_id, &id).await?;
    let dependencies = dependency_value(&state, &ctx.org_id, &name).await?;
    let count = dependencies["total"].as_u64().unwrap_or_default();
    if count > 0 && !request.force {
        return Err(Error::conflict(format!(
            "tool `{name}` has {count} active dependencies; confirm force disable"
        )));
    }
    let updated = apply_tool_policy(
        &state,
        &ctx,
        &id,
        ToolPolicyRequest {
            enabled: Some(false),
            execution_mode: None,
            risk: None,
            environment_overrides: None,
            timeout_ms: None,
            max_calls_per_run: None,
            max_response_bytes: None,
        },
    )
    .await?;
    activity_audit::record(
        &state,
        &ctx,
        "intelligence.tool.disabled",
        "intelligence_tool",
        &id,
        json!({"forced": request.force, "dependency_count": count}),
    )
    .await;
    Ok(Json(json!({"tool": updated, "dependencies": dependencies})))
}

async fn resolve_tool_name(state: &AppState, org_id: &Id, id: &str) -> Result<String> {
    if is_builtin_tool(id) {
        return Ok(id.into());
    }
    state
        .intelligence
        .tool_control
        .list_mcp_tools(org_id, None)
        .await?
        .into_iter()
        .find(|tool| tool.id.0 == id || tool.name == id)
        .map(|tool| tool.name)
        .ok_or_else(|| Error::not_found(format!("tool `{id}` not found")))
}

#[permission("intelligence.use")]
async fn tool_dependencies(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    require_license(&state)?;
    let name = resolve_tool_name(&state, &ctx.org_id, &id).await?;
    Ok(Json(dependency_value(&state, &ctx.org_id, &name).await?))
}

pub(crate) async fn dependency_value(
    state: &AppState,
    org_id: &Id,
    tool_name: &str,
) -> Result<Value> {
    let profiles = state
        .intelligence
        .repository
        .list_profiles(org_id)
        .await?
        .into_iter()
        .filter(|profile| profile.allowed_tools.iter().any(|tool| tool == tool_name))
        .map(|profile| {
            json!({
                "id": profile.id,
                "name": profile.name,
                "enabled": profile.enabled,
                "is_default": profile.is_default
            })
        })
        .collect::<Vec<_>>();
    let automations = state
        .intelligence
        .repository
        .list_automations(org_id)
        .await?
        .into_iter()
        .filter(|automation| {
            automation
                .allowed_tools
                .iter()
                .any(|tool| tool == tool_name)
        })
        .map(|automation| {
            json!({
                "id": automation.id,
                "name": automation.name,
                "enabled": automation.enabled
            })
        })
        .collect::<Vec<_>>();
    let total = profiles.len() + automations.len();
    Ok(json!({
        "tool_name": tool_name,
        "total": total,
        "agent_profiles": profiles,
        "automations": automations,
        "investigation_templates": []
    }))
}

#[derive(Debug, Deserialize)]
struct CallsQuery {
    #[serde(default = "default_call_limit")]
    limit: usize,
}

const fn default_call_limit() -> usize {
    100
}

#[permission("intelligence.use")]
async fn tool_calls(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Query(query): Query<CallsQuery>,
) -> Result<Json<Value>> {
    require_license(&state)?;
    let name = resolve_tool_name(&state, &ctx.org_id, &id).await?;
    let calls = state
        .intelligence
        .repository
        .list_tool_calls(&ctx.org_id, Some(&name), query.limit)
        .await?
        .into_iter()
        .map(|mut call| {
            call.input = redact_sensitive_value(&call.input);
            call
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({"calls": calls})))
}

#[derive(Debug, Deserialize)]
struct ToolTestRequest {
    #[serde(default)]
    arguments: Value,
    #[serde(default = "default_true")]
    dry_run: bool,
    #[serde(default)]
    validate_only: bool,
}

const fn default_true() -> bool {
    true
}

#[permission("intelligence.manage")]
async fn test_tool(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<ToolTestRequest>,
) -> Result<Json<Value>> {
    require_license(&state)?;
    let tools = managed_tools(&state, &ctx).await?;
    let tool = tools
        .into_iter()
        .find(|tool| {
            tool["id"].as_str() == Some(id.as_str()) || tool["name"].as_str() == Some(id.as_str())
        })
        .ok_or_else(|| Error::not_found(format!("tool `{id}` not found")))?;
    validate_schema(&tool["input_schema"], &request.arguments)?;
    let read_only = tool["capabilities"]["read_only"].as_bool().unwrap_or(false);
    if request.validate_only || !read_only {
        if !read_only && !request.dry_run {
            return Err(Error::forbidden(
                "write-capable tools can only be tested with dry_run=true",
            ));
        }
        return Ok(Json(json!({
            "success": true,
            "validated": true,
            "dry_run": true,
            "executed": false,
            "side_effects": false,
            "message": if read_only {
                "参数校验通过。"
            } else {
                "参数校验通过；写工具测试未执行真实操作。"
            },
            "request": redact_sensitive_value(&request.arguments)
        })));
    }
    if tool["execution_mode"] != "automatic" {
        return Ok(Json(json!({
            "success": true,
            "validated": true,
            "dry_run": request.dry_run,
            "executed": false,
            "side_effects": false,
            "message": "工具策略要求确认或审批；测试仅完成参数校验。"
        })));
    }
    let resolution = toolsets::resolve_toolsets(&state, &ctx.org_id).await?;
    let dispatcher = RealToolDispatcher::new(state.clone()).with_toolsets(resolution);
    let started = Instant::now();
    let result = dispatcher
        .dispatch(
            &ToolAuthContext {
                user_id: ctx.user_id.0.clone(),
                org_id: ctx.org_id.0.clone(),
                chat_id: None,
                investigation_id: None,
                execution_policy: Default::default(),
                query_generation_only: false,
            },
            ToolCall {
                name: tool["name"].as_str().unwrap_or(&id).to_string(),
                arguments: request.arguments.clone(),
            },
        )
        .await?;
    let duration_ms = i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);
    activity_audit::record(
        &state,
        &ctx,
        "intelligence.tool.tested",
        "intelligence_tool",
        &id,
        json!({
            "success": !result.is_error,
            "duration_ms": duration_ms,
            "dry_run": request.dry_run
        }),
    )
    .await;
    Ok(Json(json!({
        "success": !result.is_error,
        "validated": true,
        "dry_run": request.dry_run,
        "executed": true,
        "side_effects": false,
        "duration_ms": duration_ms,
        "request": redact_sensitive_value(&request.arguments),
        "response": result
    })))
}

#[permission("intelligence.use")]
async fn get_policy_defaults(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<ToolPolicyDefaults>> {
    require_license(&state)?;
    Ok(Json(
        state
            .intelligence
            .tool_control
            .get_policy_defaults(&ctx.org_id)
            .await?
            .unwrap_or_else(|| {
                ToolPolicyDefaults::system_defaults(ctx.org_id.clone(), ctx.user_id.clone())
            }),
    ))
}

#[derive(Debug, Deserialize)]
struct PolicyDefaultsRequest {
    risk_modes: Value,
    #[serde(default)]
    environment_overrides: Value,
}

#[permission("intelligence.manage")]
async fn update_policy_defaults(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<PolicyDefaultsRequest>,
) -> Result<Json<ToolPolicyDefaults>> {
    require_license(&state)?;
    validate_default_modes(&request.risk_modes)?;
    validate_environment_overrides(&request.environment_overrides)?;
    let existing = state
        .intelligence
        .tool_control
        .get_policy_defaults(&ctx.org_id)
        .await?;
    let now = TimestampMicros::now();
    let saved = state
        .intelligence
        .tool_control
        .upsert_policy_defaults(ToolPolicyDefaults {
            org_id: ctx.org_id.clone(),
            risk_modes: request.risk_modes,
            environment_overrides: request.environment_overrides,
            updated_by: ctx.user_id.clone(),
            created_at: existing
                .as_ref()
                .map(|defaults| defaults.created_at)
                .unwrap_or(now),
            updated_at: now,
        })
        .await?;
    activity_audit::record(
        &state,
        &ctx,
        "intelligence.tool.default_policy_updated",
        "intelligence_tool_policy",
        &ctx.org_id.0,
        json!({
            "risk_modes": saved.risk_modes,
            "environment_overrides": saved.environment_overrides
        }),
    )
    .await;
    Ok(Json(saved))
}

fn validate_default_modes(value: &Value) -> Result<()> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::invalid("risk_modes must be an object"))?;
    for risk in [
        RiskLevel::L0,
        RiskLevel::L1,
        RiskLevel::L2,
        RiskLevel::L3,
        RiskLevel::L4,
    ] {
        let key = format!("{risk:?}").to_ascii_lowercase();
        let mode = object
            .get(&key)
            .and_then(Value::as_str)
            .and_then(parse_execution_mode)
            .ok_or_else(|| Error::invalid(format!("risk_modes.{key} is invalid")))?;
        if !mode.allowed_for_risk(risk) {
            return Err(Error::invalid(format!(
                "risk_modes.{key} violates the hard risk floor"
            )));
        }
    }
    Ok(())
}

fn validate_environment_overrides(value: &Value) -> Result<()> {
    let environments = value
        .as_object()
        .ok_or_else(|| Error::invalid("environment_overrides must be an object"))?;
    for (environment, modes) in environments {
        if !matches!(
            environment.as_str(),
            "development" | "staging" | "production"
        ) {
            return Err(Error::invalid(format!(
                "unsupported policy environment `{environment}`"
            )));
        }
        if !modes.is_object() {
            return Err(Error::invalid(format!(
                "environment override `{environment}` must be an object"
            )));
        }
        for (risk_key, mode_value) in modes.as_object().into_iter().flatten() {
            let risk = match risk_key.as_str() {
                "l0" => RiskLevel::L0,
                "l1" => RiskLevel::L1,
                "l2" => RiskLevel::L2,
                "l3" => RiskLevel::L3,
                "l4" => RiskLevel::L4,
                _ => {
                    return Err(Error::invalid(format!(
                        "invalid risk `{risk_key}` in `{environment}`"
                    )));
                }
            };
            let mode = mode_value
                .as_str()
                .and_then(parse_execution_mode)
                .ok_or_else(|| Error::invalid("invalid environment execution mode"))?;
            if !mode.allowed_for_risk(risk) {
                return Err(Error::invalid(format!(
                    "{environment}.{risk_key} violates the hard risk floor"
                )));
            }
        }
    }
    Ok(())
}

fn validate_tool_environment_overrides(value: &Value, risk: RiskLevel) -> Result<()> {
    let environments = value
        .as_object()
        .ok_or_else(|| Error::invalid("tool environment_overrides must be an object"))?;
    for (environment, mode_value) in environments {
        if !matches!(
            environment.as_str(),
            "development" | "staging" | "production"
        ) {
            return Err(Error::invalid(format!(
                "unsupported tool policy environment `{environment}`"
            )));
        }
        let mode = mode_value
            .as_str()
            .and_then(parse_execution_mode)
            .ok_or_else(|| {
                Error::invalid(format!(
                    "tool environment override `{environment}` is invalid"
                ))
            })?;
        if !mode.allowed_for_risk(risk) {
            return Err(Error::invalid(format!(
                "{environment} override violates the hard {risk:?} floor"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn l4_default_cannot_be_automatic() {
        assert!(
            validate_default_modes(&json!({
                "l0": "automatic",
                "l1": "confirmation",
                "l2": "single_approval",
                "l3": "dual_approval",
                "l4": "automatic"
            }))
            .is_err()
        );
    }

    #[test]
    fn production_override_cannot_weaken_l3() {
        assert!(
            validate_environment_overrides(&json!({
                "production": {"l3": "automatic"}
            }))
            .is_err()
        );
    }

    #[test]
    fn tool_environment_override_cannot_weaken_l4() {
        assert!(
            validate_tool_environment_overrides(&json!({"production": "automatic"}), RiskLevel::L4)
                .is_err()
        );
    }
}
