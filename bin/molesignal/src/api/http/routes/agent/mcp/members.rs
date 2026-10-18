// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 作用域里的成员自己能做的事：启停服务器的工具（工具策略仍由 `agent.manage` 在「工具」里
//! 管），一次启用全部只读工具，存取本人的成员凭据。

use axum::{
    Extension, Json,
    extract::{Path, State},
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::super::viewer::viewer;
use crate::{
    agent::{
        model::RiskLevel,
        tool_control::{ManagedToolStatus, McpTool},
    },
    api::{AppState, http::routes::activity_audit},
    app::iam::IamContext,
    domain::iam::permission,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[derive(Debug, Deserialize)]
pub(super) struct ToolEnabledRequest {
    enabled: bool,
}

fn switched(tool: McpTool, enabled: bool) -> Result<McpTool> {
    if enabled
        && (tool.status == ManagedToolStatus::Unavailable || tool.unavailable_diagnostic.is_some())
    {
        return Err(Error::conflict(
            "this tool is unavailable; synchronize the server before enabling it",
        ));
    }

    Ok(McpTool {
        enabled,
        status: if enabled {
            ManagedToolStatus::Healthy
        } else {
            ManagedToolStatus::Disabled
        },
        updated_at: TimestampMicros::now(),
        ..tool
    })
}

/// 只读、风险下限为 L0、schema 可用、还没启用的工具。
fn enables_as_read_only(tool: &McpTool) -> bool {
    !tool.enabled
        && tool.minimum_risk == RiskLevel::L0
        && tool.capabilities["read_only"].as_bool().unwrap_or(false)
        && tool.status != ManagedToolStatus::Unavailable
        && tool.unavailable_diagnostic.is_none()
}

#[permission("agent.use")]
pub(super) async fn set_tool_enabled(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path((id, tool_id)): Path<(String, String)>,
    Json(request): Json<ToolEnabledRequest>,
) -> Result<Json<Value>> {
    let viewer = viewer(&state, &ctx).await?;
    let id = Id(id);
    let server = state.agent.mcp.editable(&ctx.org_id, &viewer, &id).await?;

    let tool = state
        .agent
        .tool_control
        .list_mcp_tools(&ctx.org_id, Some(&id))
        .await?
        .into_iter()
        .find(|tool| tool.id.0 == tool_id)
        .ok_or_else(|| Error::not_found(format!("MCP tool `{tool_id}` not found")))?;

    let tool = state
        .agent
        .tool_control
        .update_mcp_tool_policy(switched(tool, request.enabled)?)
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        if tool.enabled {
            "agent.mcp_tool.enabled"
        } else {
            "agent.mcp_tool.disabled"
        },
        "agent_mcp_server",
        &server.id.0,
        json!({"tool": tool.name, "server": server.name}),
    )
    .await;

    Ok(Json(json!(tool)))
}

#[permission("agent.use")]
pub(super) async fn enable_read_only(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let viewer = viewer(&state, &ctx).await?;
    let id = Id(id);
    let server = state.agent.mcp.editable(&ctx.org_id, &viewer, &id).await?;

    let tools = state
        .agent
        .tool_control
        .list_mcp_tools(&ctx.org_id, Some(&id))
        .await?;

    let mut enabled = Vec::new();

    for tool in tools.into_iter().filter(enables_as_read_only) {
        let tool = state
            .agent
            .tool_control
            .update_mcp_tool_policy(switched(tool, true)?)
            .await?;

        enabled.push(tool.name);
    }

    activity_audit::record(
        &state,
        &ctx,
        "agent.mcp_server.read_only_tools_enabled",
        "agent_mcp_server",
        &server.id.0,
        json!({"server": server.name, "tools": enabled}),
    )
    .await;

    Ok(Json(json!({
        "enabled": enabled.len(),
        "tools": state.agent.tool_control.list_mcp_tools(&ctx.org_id, Some(&id)).await?,
    })))
}

#[derive(Debug, Deserialize)]
pub(super) struct CredentialRequest {
    value: String,
}

#[permission("agent.use")]
pub(super) async fn put_my_credential(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<CredentialRequest>,
) -> Result<Json<Value>> {
    let viewer = viewer(&state, &ctx).await?;

    let (server, credential) = state
        .agent
        .mcp
        .put_my_credential(&ctx.org_id, &viewer, &Id(id), &request.value)
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "agent.mcp_server.member_credential_saved",
        "agent_mcp_server",
        &server.id.0,
        json!({"server": server.name}),
    )
    .await;

    Ok(Json(json!({ "credential": credential })))
}

#[permission("agent.use")]
pub(super) async fn delete_my_credential(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let viewer = viewer(&state, &ctx).await?;

    let (server, removed) = state
        .agent
        .mcp
        .delete_my_credential(&ctx.org_id, &viewer, &Id(id))
        .await?;

    if removed {
        activity_audit::record(
            &state,
            &ctx,
            "agent.mcp_server.member_credential_removed",
            "agent_mcp_server",
            &server.id.0,
            json!({"server": server.name}),
        )
        .await;
    }

    Ok(Json(json!({ "deleted": removed })))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::agent::tool_control::ToolExecutionMode;

    fn tool(read_only: bool, risk: RiskLevel, status: ManagedToolStatus) -> McpTool {
        McpTool {
            id: Id::from_string("tool"),
            org_id: Id::from_string("org"),
            server_id: Id::from_string("server"),
            remote_name: "search".into(),
            name: "mcp__github__search".into(),
            display_name: "search".into(),
            description: String::new(),
            input_schema: json!({"type": "object"}),
            schema_hash: "hash".into(),
            schema_dialect: "dialect".into(),
            schema_synced_at: TimestampMicros(1),
            unavailable_diagnostic: None,
            output_schema: None,
            minimum_risk: risk,
            risk,
            execution_mode: ToolExecutionMode::Automatic,
            capabilities: json!({"read_only": read_only}),
            tags: Vec::new(),
            enabled: false,
            status,
            version: None,
            last_synced_at: TimestampMicros(1),
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
        }
    }

    #[test]
    fn only_safe_read_only_tools_are_switched_on_in_bulk() {
        assert!(enables_as_read_only(&tool(
            true,
            RiskLevel::L0,
            ManagedToolStatus::Disabled
        )));
        assert!(!enables_as_read_only(&tool(
            false,
            RiskLevel::L0,
            ManagedToolStatus::Disabled
        )));
        assert!(!enables_as_read_only(&tool(
            true,
            RiskLevel::L2,
            ManagedToolStatus::Disabled
        )));
        assert!(!enables_as_read_only(&tool(
            true,
            RiskLevel::L0,
            ManagedToolStatus::Unavailable
        )));
    }

    #[test]
    fn an_unavailable_tool_cannot_be_switched_on() {
        assert!(
            switched(
                tool(true, RiskLevel::L0, ManagedToolStatus::Unavailable),
                true
            )
            .is_err()
        );
        assert!(
            switched(
                tool(true, RiskLevel::L0, ManagedToolStatus::Unavailable),
                false
            )
            .is_ok()
        );

        let on = switched(tool(true, RiskLevel::L0, ManagedToolStatus::Disabled), true).unwrap();
        assert!(on.enabled);
        assert_eq!(on.status, ManagedToolStatus::Healthy);
    }
}
