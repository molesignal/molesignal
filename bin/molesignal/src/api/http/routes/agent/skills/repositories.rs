// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Skill 仓库的接口。新建与修改后立刻同步一次，结果随响应返回。
//!
//! - `GET    /api/v1/agent/skill-repositories`
//! - `POST   /api/v1/agent/skill-repositories`
//! - `PUT    /api/v1/agent/skill-repositories/{id}`
//! - `DELETE /api/v1/agent/skill-repositories/{id}`
//! - `POST   /api/v1/agent/skill-repositories/{id}/sync`
//!
//! 访问令牌只写不读：响应里只有是否已设与末四位；修改时留空表示不变。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{get, post, put},
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::super::viewer::{requested_scope, viewer};
use crate::{
    agent::skills::{RepoHost, TokenChange},
    api::{AppState, http::routes::activity_audit},
    app::{iam::IamContext, skills::RepositoryInput},
    domain::iam::permission,
    shared::{Result, ids::Id},
};

pub(super) fn routes() -> axum::Router<AppState> {
    Router::new()
        .route(
            "/agent/skill-repositories",
            get(list_repositories).post(create_repository),
        )
        .route(
            "/agent/skill-repositories/{id}",
            put(update_repository).delete(delete_repository),
        )
        .route("/agent/skill-repositories/{id}/sync", post(sync_repository))
}

#[derive(Debug, Deserialize)]
struct RepositoryRequest {
    #[serde(default = "personal")]
    scope: String,
    #[serde(default)]
    team_id: Option<String>,
    url: String,
    /// 不填时由地址认出。
    #[serde(default)]
    host: Option<String>,
    #[serde(default)]
    archive_url: Option<String>,
    #[serde(default)]
    git_ref: String,
    #[serde(default)]
    dir: String,
    #[serde(default)]
    auto_sync: bool,
    /// 只写；不填表示不变。
    #[serde(default)]
    token: Option<String>,
    /// 清除已保存的令牌。
    #[serde(default)]
    clear_token: bool,
}

fn personal() -> String {
    "personal".into()
}

async fn input(
    state: &AppState,
    ctx: &IamContext,
    request: RepositoryRequest,
) -> Result<RepositoryInput> {
    let scope = requested_scope(state, ctx, &request.scope, request.team_id.as_deref()).await?;

    let host = request
        .host
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "auto")
        .map(RepoHost::parse)
        .transpose()?;

    let token = match (
        request
            .token
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        request.clear_token,
    ) {
        (Some(token), _) => TokenChange::Set(token),
        (None, true) => TokenChange::Clear,
        (None, false) => TokenChange::Keep,
    };

    Ok(RepositoryInput {
        scope,
        url: request.url,
        host,
        archive_url: request.archive_url,
        git_ref: request.git_ref,
        dir: request.dir,
        auto_sync: request.auto_sync,
        token,
    })
}

#[permission("agent.use")]
async fn list_repositories(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    let viewer = viewer(&state, &ctx).await?;

    let repositories = state
        .agent
        .skills
        .list_repositories(&ctx.org_id, &viewer)
        .await?;

    let repositories = repositories
        .into_iter()
        .map(|repository| {
            let editable = viewer.can_edit(&repository.scope);
            let mut value = json!(repository);
            value["editable"] = json!(editable);
            value
        })
        .collect::<Vec<_>>();

    Ok(Json(json!({ "repositories": repositories })))
}

#[permission("agent.use")]
async fn create_repository(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<RepositoryRequest>,
) -> Result<Json<Value>> {
    let input = input(&state, &ctx, request).await?;
    let viewer = viewer(&state, &ctx).await?;

    let (repository, report) = state
        .agent
        .skills
        .add_repository(&ctx.org_id, &viewer, input)
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "agent.skill_repository.created",
        "agent_skill_repository",
        &repository.id.0,
        json!({
            "url": repository.url,
            "ref": repository.git_ref,
            "scope": repository.scope.kind(),
            "status": report.status,
            "imported": report.imported,
        }),
    )
    .await;

    Ok(Json(json!({ "repository": repository, "sync": report })))
}

#[permission("agent.use")]
async fn update_repository(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<RepositoryRequest>,
) -> Result<Json<Value>> {
    let input = input(&state, &ctx, request).await?;
    let viewer = viewer(&state, &ctx).await?;

    let (repository, report) = state
        .agent
        .skills
        .update_repository(&ctx.org_id, &viewer, &Id(id), input)
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "agent.skill_repository.updated",
        "agent_skill_repository",
        &repository.id.0,
        json!({
            "url": repository.url,
            "ref": repository.git_ref,
            "scope": repository.scope.kind(),
            "status": report.status,
        }),
    )
    .await;

    Ok(Json(json!({ "repository": repository, "sync": report })))
}

#[permission("agent.use")]
async fn delete_repository(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let viewer = viewer(&state, &ctx).await?;

    let repository = state
        .agent
        .skills
        .delete_repository(&ctx.org_id, &viewer, &Id(id))
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "agent.skill_repository.deleted",
        "agent_skill_repository",
        &repository.id.0,
        json!({ "url": repository.url }),
    )
    .await;

    Ok(Json(json!({ "deleted": true })))
}

#[permission("agent.use")]
async fn sync_repository(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let viewer = viewer(&state, &ctx).await?;
    let id = Id(id);

    let report = state
        .agent
        .skills
        .sync_repository(&ctx.org_id, &viewer, &id)
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "agent.skill_repository.synced",
        "agent_skill_repository",
        &id.0,
        json!({
            "status": report.status,
            "imported": report.imported,
            "updated": report.updated,
            "removed": report.removed,
            "commit": report.commit,
        }),
    )
    .await;

    Ok(Json(json!({ "sync": report })))
}
