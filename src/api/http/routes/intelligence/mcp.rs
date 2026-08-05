// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! MCP Server 控制面与受限 Streamable HTTP runtime。
//!
//! 安全边界：
//! - 仅管理员可 CRUD / 测试 / 同步；
//! - 任意 stdio 命令、Unix Socket 与旧 SSE transport 没有执行入口；
//! - HTTP 目标在每次请求前做域名、CIDR 与私网解析校验，禁用重定向；
//! - 凭据由 repository 即时解密，只进入请求 header，不进入响应、日志或审计；
//! - 同步发现的工具默认关闭，风险下限取 MCP annotations 的保守推断；
//! - 模型提供的租户、用户与审批身份字段在转发前移除。

use std::collections::HashMap;

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use ipnet::IpNet;
use reqwest::header::HeaderName;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    api::{AppState, http::routes::activity_audit},
    app::iam::IamContext,
    domain::iam::permission,
    intelligence::{
        FEATURE,
        tool_control::{McpServer, McpServerInput},
    },
    shared::{Error, Result, ids::Id},
};

mod execution;
mod runtime;
mod schema;
mod synchronization;
pub(crate) use execution::execute_tool;
pub(crate) use schema::validate_schema;

const MCP_PROTOCOL_VERSION: &str = "2025-03-26";
const DEFAULT_TIMEOUT_MS: i64 = 10_000;
const MAX_TIMEOUT_MS: i64 = 120_000;
const DEFAULT_MAX_RESPONSE_BYTES: i64 = 1_048_576;
const MAX_RESPONSE_BYTES: i64 = 16 * 1_048_576;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/intelligence/mcp-servers",
            get(list_servers).post(create_server),
        )
        .route(
            "/intelligence/mcp-servers/{id}",
            get(get_server).put(update_server).delete(delete_server),
        )
        .route(
            "/intelligence/mcp-servers/{id}/test",
            post(synchronization::test_server),
        )
        .route(
            "/intelligence/mcp-servers/{id}/sync",
            post(synchronization::sync_server),
        )
        .route(
            "/intelligence/mcp-servers/{id}/tools",
            get(list_server_tools),
        )
}

fn require_license(state: &AppState) -> Result<()> {
    if !state.platform.license.has_feature(FEATURE) {
        return Err(Error::forbidden(format!("{FEATURE} feature not licensed")));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct McpServerRequest {
    name: String,
    #[serde(default = "default_transport")]
    transport: String,
    endpoint_url: Option<String>,
    command_template: Option<String>,
    #[serde(default = "default_auth_type")]
    auth_type: String,
    auth_header: Option<String>,
    /// write-only；响应结构中没有此字段。
    credential: Option<String>,
    #[serde(default = "default_true")]
    private_only: bool,
    #[serde(default)]
    allowed_domains: Vec<String>,
    #[serde(default)]
    allowed_cidrs: Vec<String>,
    #[serde(default)]
    follow_redirects: bool,
    #[serde(default = "default_true")]
    tls_verify: bool,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: i64,
    #[serde(default = "default_max_response_bytes")]
    max_response_bytes: i64,
    #[serde(default = "default_true")]
    enabled: bool,
}

fn default_transport() -> String {
    "streamable_http".into()
}

fn default_auth_type() -> String {
    "none".into()
}

const fn default_true() -> bool {
    true
}

const fn default_timeout_ms() -> i64 {
    DEFAULT_TIMEOUT_MS
}

const fn default_max_response_bytes() -> i64 {
    DEFAULT_MAX_RESPONSE_BYTES
}

fn normalize_server_request(
    request: McpServerRequest,
    id: Id,
    org_id: Id,
    created_by: Id,
) -> Result<(McpServerInput, Option<String>)> {
    let name = request.name.trim();
    if name.is_empty() || name.chars().count() > 255 {
        return Err(Error::invalid(
            "MCP Server name must contain 1 to 255 characters",
        ));
    }
    if !matches!(
        request.transport.as_str(),
        "streamable_http" | "sse" | "stdio" | "unix_socket"
    ) {
        return Err(Error::invalid("unsupported MCP transport"));
    }
    if request.transport == "streamable_http" {
        let endpoint = request
            .endpoint_url
            .as_deref()
            .ok_or_else(|| Error::invalid("Endpoint URL is required"))?;
        runtime::validate_endpoint_shape(endpoint)?;
    } else if request.transport == "stdio" && request.command_template.is_none() {
        return Err(Error::invalid(
            "stdio MCP requires an administrator-provided command template",
        ));
    }
    if request.follow_redirects {
        return Err(Error::invalid(
            "MCP redirects are blocked by the server security policy",
        ));
    }
    if !matches!(
        request.auth_type.as_str(),
        "none" | "bearer_token" | "api_key" | "oauth" | "mtls" | "internal_service_identity"
    ) {
        return Err(Error::invalid("unsupported MCP authentication type"));
    }
    let credential = request
        .credential
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if request.auth_type != "none" && credential.is_none() {
        // 更新时允许留空以保留旧 secret；handler 会根据现有元数据补充校验。
    }
    if !(1_000..=MAX_TIMEOUT_MS).contains(&request.timeout_ms) {
        return Err(Error::invalid(
            "MCP timeout_ms must be between 1000 and 120000",
        ));
    }
    if !(1_024..=MAX_RESPONSE_BYTES).contains(&request.max_response_bytes) {
        return Err(Error::invalid(
            "MCP max_response_bytes must be between 1024 and 16777216",
        ));
    }
    let allowed_domains = request
        .allowed_domains
        .into_iter()
        .map(|value| value.trim().trim_start_matches('.').to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    for domain in &allowed_domains {
        if domain.contains('/') || domain.contains(':') || domain.contains(char::is_whitespace) {
            return Err(Error::invalid(format!(
                "invalid allowed MCP domain `{domain}`"
            )));
        }
    }
    for cidr in &request.allowed_cidrs {
        cidr.parse::<IpNet>()
            .map_err(|_| Error::invalid(format!("invalid MCP CIDR `{cidr}`")))?;
    }
    let auth_header = request
        .auth_header
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());
    if auth_header.is_some()
        && !matches!(
            request.auth_type.as_str(),
            "api_key" | "internal_service_identity"
        )
    {
        return Err(Error::invalid(
            "a custom authentication header is only valid for API key or internal identity auth",
        ));
    }
    if let Some(header) = auth_header.as_deref() {
        let parsed = HeaderName::from_bytes(header.as_bytes())
            .map_err(|_| Error::invalid("invalid MCP authentication header"))?;
        if !parsed.as_str().starts_with("x-") {
            return Err(Error::invalid(
                "custom MCP authentication headers must use an X- prefix",
            ));
        }
    }
    Ok((
        McpServerInput {
            id,
            org_id,
            name: name.to_string(),
            transport: request.transport,
            endpoint_url: request
                .endpoint_url
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            command_template: request
                .command_template
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            auth_type: request.auth_type,
            auth_header,
            private_only: request.private_only,
            allowed_domains,
            allowed_cidrs: request.allowed_cidrs,
            follow_redirects: false,
            tls_verify: request.tls_verify,
            timeout_ms: request.timeout_ms,
            max_response_bytes: request.max_response_bytes,
            enabled: request.enabled,
            created_by,
        },
        credential,
    ))
}

#[permission("intelligence.use")]
async fn list_servers(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    require_license(&state)?;
    let servers = state
        .intelligence
        .tool_control
        .list_mcp_servers(&ctx.org_id)
        .await?;
    let tools = state
        .intelligence
        .tool_control
        .list_mcp_tools(&ctx.org_id, None)
        .await?;
    let mut counts = HashMap::<String, usize>::new();
    for tool in tools {
        *counts.entry(tool.server_id.0).or_default() += 1;
    }
    Ok(Json(json!({
        "servers": servers.into_iter().map(|server| {
            let tool_count = counts.get(&server.id.0).copied().unwrap_or_default();
            let mut value = serde_json::to_value(server).unwrap_or_else(|_| json!({}));
            value["tool_count"] = json!(tool_count);
            value
        }).collect::<Vec<_>>()
    })))
}

#[permission("intelligence.use")]
async fn get_server(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    require_license(&state)?;
    let id = Id(id);
    let server = state
        .intelligence
        .tool_control
        .get_mcp_server(&ctx.org_id, &id)
        .await?;
    let tools = state
        .intelligence
        .tool_control
        .list_mcp_tools(&ctx.org_id, Some(&id))
        .await?;
    Ok(Json(json!({"server": server, "tools": tools})))
}

#[permission("intelligence.manage")]
async fn create_server(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<McpServerRequest>,
) -> Result<Json<McpServer>> {
    require_license(&state)?;
    let (input, credential) =
        normalize_server_request(request, Id::new(), ctx.org_id.clone(), ctx.user_id.clone())?;
    if input.auth_type != "none" && credential.is_none() {
        return Err(Error::invalid(
            "credential is required for the selected authentication type",
        ));
    }
    let server = state
        .intelligence
        .tool_control
        .create_mcp_server(input, credential.as_deref())
        .await?;
    activity_audit::record(
        &state,
        &ctx,
        "intelligence.mcp_server.created",
        "intelligence_mcp_server",
        &server.id.0,
        json!({
            "name": server.name,
            "transport": server.transport,
            "auth_type": server.auth_type,
            "credential_set": server.credential_set
        }),
    )
    .await;
    Ok(Json(server))
}

#[permission("intelligence.manage")]
async fn update_server(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<McpServerRequest>,
) -> Result<Json<McpServer>> {
    require_license(&state)?;
    let id = Id(id);
    let existing = state
        .intelligence
        .tool_control
        .get_mcp_server(&ctx.org_id, &id)
        .await?;
    let (input, credential) =
        normalize_server_request(request, id, ctx.org_id.clone(), existing.created_by)?;
    if input.auth_type != "none" && credential.is_none() && !existing.credential_set {
        return Err(Error::invalid(
            "credential is required for the selected authentication type",
        ));
    }
    let server = state
        .intelligence
        .tool_control
        .update_mcp_server(input, credential.as_deref())
        .await?;
    activity_audit::record(
        &state,
        &ctx,
        "intelligence.mcp_server.updated",
        "intelligence_mcp_server",
        &server.id.0,
        json!({
            "name": server.name,
            "transport": server.transport,
            "enabled": server.enabled,
            "credential_rotated": credential.is_some()
        }),
    )
    .await;
    Ok(Json(server))
}

#[permission("intelligence.manage")]
async fn delete_server(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    require_license(&state)?;
    let id = Id(id);
    let tools = state
        .intelligence
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
        .intelligence
        .tool_control
        .delete_mcp_server(&ctx.org_id, &id)
        .await?;
    activity_audit::record(
        &state,
        &ctx,
        "intelligence.mcp_server.deleted",
        "intelligence_mcp_server",
        &id.0,
        json!({"tool_count": tools.len()}),
    )
    .await;
    Ok(Json(json!({"deleted": true})))
}

#[permission("intelligence.manage")]
async fn list_server_tools(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    require_license(&state)?;
    let id = Id(id);
    let _ = state
        .intelligence
        .tool_control
        .get_mcp_server(&ctx.org_id, &id)
        .await?;
    Ok(Json(json!({
        "tools": state.intelligence.tool_control
            .list_mcp_tools(&ctx.org_id, Some(&id)).await?
    })))
}
