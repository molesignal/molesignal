// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `GET / PUT /api/v1/agent/network-policy`：团队成员与个人建的连接（Skill 仓库、MCP 服务器）
//! 默认只能连公网，这里列出组织允许他们连的内网主机与网段。只有 `agent.manage` 能看能改。

use axum::{Extension, Json, Router, extract::State, routing::get};
use serde::Deserialize;
use serde_json::json;

use crate::{
    agent::network::MemberNetworkPolicy,
    api::{AppState, http::routes::activity_audit},
    app::iam::IamContext,
    domain::iam::permission,
    shared::{Result, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new().route("/agent/network-policy", get(get_policy).put(put_policy))
}

#[permission("agent.manage")]
async fn get_policy(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<MemberNetworkPolicy>> {
    Ok(Json(state.agent.network.get(&ctx.org_id).await?))
}

#[derive(Debug, Deserialize)]
struct PolicyRequest {
    #[serde(default)]
    private_hosts: Vec<String>,
    #[serde(default)]
    private_cidrs: Vec<String>,
}

#[permission("agent.manage")]
async fn put_policy(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<PolicyRequest>,
) -> Result<Json<MemberNetworkPolicy>> {
    let policy = MemberNetworkPolicy {
        private_hosts: request.private_hosts,
        private_cidrs: request.private_cidrs,
        updated_by: Some(ctx.user_id.clone()),
        updated_at: Some(TimestampMicros::now()),
    }
    .normalized()?;

    let saved = state.agent.network.put(&ctx.org_id, policy).await?;

    activity_audit::record(
        &state,
        &ctx,
        "agent.network_policy.updated",
        "agent_network_policy",
        &ctx.org_id.0,
        json!({
            "private_hosts": saved.private_hosts,
            "private_cidrs": saved.private_cidrs,
        }),
    )
    .await;

    Ok(Json(saved))
}
