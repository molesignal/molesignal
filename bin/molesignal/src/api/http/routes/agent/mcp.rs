// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 外部 MCP 服务器的接口。谁能看、能改、能停用按 `agent::scope` 判定：组织级的要
//! `agent.manage`，团队的由团队成员与管理员管理，个人的只有本人能改（管理员能停用、删除）。
//!
//! - `GET    /api/v1/agent/mcp-servers`                       看得到的服务器
//! - `POST   /api/v1/agent/mcp-servers`                       新建（`scope`、`team_id`）
//! - `POST   /api/v1/agent/mcp-servers/import`                粘贴其他客户端的配置导入
//! - `GET    /api/v1/agent/mcp-servers/{id}`                  服务器、工具与 prompt
//! - `PUT    /api/v1/agent/mcp-servers/{id}`                  修改、移交作用域
//! - `PATCH  /api/v1/agent/mcp-servers/{id}`                  启停
//! - `DELETE /api/v1/agent/mcp-servers/{id}`                  删除（工具都关掉、没有依赖之后）
//! - `POST   /api/v1/agent/mcp-servers/{id}/test|sync`        测试连接、同步工具与 prompt
//! - `GET    /api/v1/agent/mcp-servers/{id}/tools`            工具
//! - `PATCH  /api/v1/agent/mcp-servers/{id}/tools/{tool_id}`  启停一个工具
//! - `POST   /api/v1/agent/mcp-servers/{id}/enable-read-only` 启用全部只读工具
//! - `PUT | DELETE /api/v1/agent/mcp-servers/{id}/my-credential` 本人的成员凭据
//!
//! 凭据、敏感请求头与成员凭据只写不读：响应里只有是否已填与末四位，不进日志与审计。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{get, patch, post, put},
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::viewer::{requested_scope, viewer};
use crate::{
    api::{AppState, http::routes::activity_audit},
    app::{iam::IamContext, mcp::ServerDraft},
    domain::iam::permission,
    shared::{Error, Result, ids::Id},
};

mod import;
mod members;
mod synchronization;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/agent/mcp-servers", get(list_servers).post(create_server))
        .route("/agent/mcp-servers/import", post(import::import_servers))
        .route(
            "/agent/mcp-servers/{id}",
            get(get_server)
                .put(update_server)
                .patch(set_server_enabled)
                .delete(delete_server),
        )
        .route(
            "/agent/mcp-servers/{id}/test",
            post(synchronization::test_server),
        )
        .route(
            "/agent/mcp-servers/{id}/sync",
            post(synchronization::sync_server),
        )
        .route("/agent/mcp-servers/{id}/tools", get(list_server_tools))
        .route(
            "/agent/mcp-servers/{id}/tools/{tool_id}",
            patch(members::set_tool_enabled),
        )
        .route(
            "/agent/mcp-servers/{id}/enable-read-only",
            post(members::enable_read_only),
        )
        .route(
            "/agent/mcp-servers/{id}/my-credential",
            put(members::put_my_credential).delete(members::delete_my_credential),
        )
}

#[derive(Debug, Deserialize)]
struct ServerRequest {
    /// 不填是组织级（只有 `agent.manage` 能建）。
    #[serde(default = "organization")]
    scope: String,
    #[serde(default)]
    team_id: Option<String>,
    #[serde(flatten)]
    draft: ServerDraft,
}

fn organization() -> String {
    "org".into()
}

#[permission("agent.use")]
async fn list_servers(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    let viewer = viewer(&state, &ctx).await?;
    let servers = state.agent.mcp.list(&ctx.org_id, &viewer).await?;

    // 页面据此只在组织级、实例允许时提供 stdio。
    Ok(Json(json!({
        "servers": servers,
        "stdio_commands": state.agent.mcp.stdio_commands(),
    })))
}

#[permission("agent.use")]
async fn get_server(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let viewer = viewer(&state, &ctx).await?;
    let id = Id(id);
    let server = state.agent.mcp.view(&ctx.org_id, &viewer, &id).await?;

    let tools = state
        .agent
        .tool_control
        .list_mcp_tools(&ctx.org_id, Some(&id))
        .await?;

    let prompts = state.agent.mcp.prompts_of(&ctx.org_id, &id).await?;

    Ok(Json(
        json!({"server": server, "tools": tools, "prompts": prompts}),
    ))
}

#[permission("agent.use")]
async fn create_server(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<ServerRequest>,
) -> Result<Json<Value>> {
    let scope = requested_scope(&state, &ctx, &request.scope, request.team_id.as_deref()).await?;
    let viewer = viewer(&state, &ctx).await?;

    let server = state
        .agent
        .mcp
        .create(&ctx.org_id, &viewer, scope, request.draft)
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "agent.mcp_server.created",
        "agent_mcp_server",
        &server.id.0,
        json!({
            "name": server.name,
            "scope": server.scope.kind(),
            "transport": server.transport,
            "auth_type": server.auth_type,
            "credential_set": server.credential_set,
            "headers": server.headers.iter().map(|header| &header.name).collect::<Vec<_>>(),
        }),
    )
    .await;

    Ok(Json(json!(server)))
}

#[permission("agent.use")]
async fn update_server(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<ServerRequest>,
) -> Result<Json<Value>> {
    let scope = requested_scope(&state, &ctx, &request.scope, request.team_id.as_deref()).await?;
    let viewer = viewer(&state, &ctx).await?;
    let credential_rotated = request
        .draft
        .credential
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());

    let (before, server) = state
        .agent
        .mcp
        .update(&ctx.org_id, &viewer, &Id(id), scope, request.draft)
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "agent.mcp_server.updated",
        "agent_mcp_server",
        &server.id.0,
        json!({
            "name": server.name,
            "scope": server.scope.kind(),
            "moved_from": (before.scope != server.scope).then(|| before.scope.kind()),
            "transport": server.transport,
            "enabled": server.enabled,
            "credential_rotated": credential_rotated,
            "headers": server.headers.iter().map(|header| &header.name).collect::<Vec<_>>(),
        }),
    )
    .await;

    Ok(Json(json!(server)))
}

#[derive(Debug, Deserialize)]
struct EnabledRequest {
    enabled: bool,
}

#[permission("agent.use")]
async fn set_server_enabled(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<EnabledRequest>,
) -> Result<Json<Value>> {
    let viewer = viewer(&state, &ctx).await?;

    let server = state
        .agent
        .mcp
        .set_enabled(&ctx.org_id, &viewer, &Id(id), request.enabled)
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        if server.enabled {
            "agent.mcp_server.enabled"
        } else {
            "agent.mcp_server.disabled"
        },
        "agent_mcp_server",
        &server.id.0,
        json!({"name": server.name, "scope": server.scope.kind()}),
    )
    .await;

    Ok(Json(json!(server)))
}

#[permission("agent.use")]
async fn delete_server(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let viewer = viewer(&state, &ctx).await?;
    let id = Id(id);
    let server = state
        .agent
        .mcp
        .moderatable(&ctx.org_id, &viewer, &id)
        .await?;

    let tools = state
        .agent
        .tool_control
        .list_mcp_tools(&ctx.org_id, Some(&id))
        .await?;

    let enabled = tools.iter().filter(|tool| tool.enabled).count();

    if enabled > 0 {
        return Err(Error::conflict(format!(
            "MCP Server has {enabled} enabled tools; disable them before removal"
        )));
    }

    for tool in &tools {
        let dependencies =
            super::tools_control::dependency_value(&state, &ctx.org_id, &tool.name).await?;
        let count = dependencies["total"].as_u64().unwrap_or_default();

        if count > 0 {
            return Err(Error::conflict(format!(
                "MCP tool `{}` has {count} active dependencies; remove or replace them before deleting the server",
                tool.name
            )));
        }
    }

    state
        .agent
        .tool_control
        .delete_mcp_server(&ctx.org_id, &id)
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "agent.mcp_server.deleted",
        "agent_mcp_server",
        &id.0,
        json!({"name": server.name, "scope": server.scope.kind(), "tool_count": tools.len()}),
    )
    .await;

    Ok(Json(json!({"deleted": true})))
}

#[permission("agent.use")]
async fn list_server_tools(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let viewer = viewer(&state, &ctx).await?;
    let id = Id(id);

    state.agent.mcp.visible(&ctx.org_id, &viewer, &id).await?;

    Ok(Json(json!({
        "tools": state.agent.tool_control
            .list_mcp_tools(&ctx.org_id, Some(&id)).await?
    })))
}
