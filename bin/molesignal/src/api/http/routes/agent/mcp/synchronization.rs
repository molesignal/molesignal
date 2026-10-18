// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 测试连接与同步。能改这台服务器的人才能做；要成员凭据的服务器用操作人自己的凭据。

use axum::{
    Extension, Json,
    extract::{Path, State},
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::super::viewer::viewer;
use crate::{
    api::{AppState, http::routes::activity_audit},
    app::iam::IamContext,
    domain::iam::permission,
    shared::{Result, ids::Id},
};

#[derive(Debug, Deserialize)]
pub(super) struct SyncRequest {
    #[serde(default)]
    selected_tools: Vec<String>,
}

#[permission("agent.use")]
pub(super) async fn test_server(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let viewer = viewer(&state, &ctx).await?;
    let id = Id(id);
    let outcome = state.agent.mcp.test(&ctx.org_id, &viewer, &id).await?;

    activity_audit::record(
        &state,
        &ctx,
        "agent.mcp_server.tested",
        "agent_mcp_server",
        &id.0,
        match &outcome.error {
            None => json!({"success": true, "discovered_tools": outcome.tools.len()}),
            Some(error) => json!({"success": false, "error": error}),
        },
    )
    .await;

    Ok(Json(json!({
        "success": outcome.error.is_none(),
        "server": outcome.server,
        "error": outcome.error,
        "discovered_tools": outcome.tools,
    })))
}

#[permission("agent.use")]
pub(super) async fn sync_server(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<SyncRequest>,
) -> Result<Json<Value>> {
    let viewer = viewer(&state, &ctx).await?;
    let id = Id(id);

    let outcome = state
        .agent
        .mcp
        .sync(&ctx.org_id, &viewer, &id, &request.selected_tools)
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "agent.mcp_server.synced",
        "agent_mcp_server",
        &id.0,
        json!({
            "tools": outcome.tools.len(),
            "prompts": outcome.prompts,
            "new_tools_default_enabled": false
        }),
    )
    .await;

    Ok(Json(json!({
        "server": outcome.server,
        "tools": outcome.tools,
        "prompt_count": outcome.prompts,
    })))
}
