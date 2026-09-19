// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 工具白名单 CRUD。
//!
//! 路径统一位于 `/agent/settings/toolsets`。
//!
//! 读取要求 `agent.use`，写入要求 `agent.manage`。
//! OSS /  切换由 bootstrap 阶段注入不同 repo 完成。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::permission,
    infra::persistence::repositories::agent::toolsets::AgentToolset,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/agent/settings/toolsets", get(list).post(create))
        .route(
            "/agent/settings/toolsets/{id}",
            axum::routing::delete(delete),
        )
}

#[derive(Debug, Deserialize)]
pub struct CreateReq {
    pub name: String,
    #[serde(default)]
    pub schema: Value,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

#[permission("agent.use")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<AgentToolset>>> {
    let rows = state.agent.toolsets.list(&ctx.org_id).await?;
    Ok(Json(rows))
}

#[permission("agent.manage")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<CreateReq>,
) -> Result<Json<AgentToolset>> {
    if req.name.trim().is_empty() {
        return Err(Error::invalid("name must not be empty"));
    }
    super::validate_toolset_schema(&req.schema)?;
    let now = TimestampMicros::now();
    let t = AgentToolset {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        name: req.name,
        schema: req.schema,
        enabled: req.enabled,
        created_at: now,
        updated_at: now,
    };
    let saved = state.agent.toolsets.create(t).await?;
    Ok(Json(saved))
}

#[permission("agent.manage")]
async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    state.agent.toolsets.delete(&ctx.org_id, &Id(id)).await?;
    Ok(Json(serde_json::json!({"deleted": true})))
}
