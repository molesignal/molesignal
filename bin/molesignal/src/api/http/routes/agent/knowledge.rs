// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 知识的 HTTP 路由（Mole Agent 的「知识」页签）。
//!
//! - `GET    /api/v1/agent/knowledge`          组织与各团队的知识概况
//! - `GET    /api/v1/agent/knowledge/files`    一份知识的文件（`?scope=org|team&team_id=`）
//! - `GET    /api/v1/agent/knowledge/file`     一个文件（再加 `&path=`）
//! - `PUT    /api/v1/agent/knowledge/file`     写一个文件
//! - `DELETE /api/v1/agent/knowledge/file`     删一个文件或一个目录
//! - `POST   /api/v1/agent/knowledge/move`     改名或移动
//! - `POST   /api/v1/agent/knowledge/upload`   上传文本文件或整个文件夹（multipart）
//!
//! 组织知识人人能读，持有 `agent.manage` 的人能改；团队知识由团队成员与管理员读写。

use std::collections::HashMap;

use axum::{
    Extension, Json, Router,
    extract::{DefaultBodyLimit, Query, State},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::viewer::{requested_scope, viewer};
use crate::{
    agent::{
        knowledge::{ENTRY_FILE, KnowledgeBase, MAX_FILE_BYTES, MAX_FILES, MAX_TOTAL_BYTES},
        scope::{Scope, Viewer},
    },
    api::{AppState, http::routes::activity_audit},
    app::iam::IamContext,
    domain::iam::permission,
    shared::{Error, Result},
};

mod upload;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/agent/knowledge", get(bases))
        .route("/agent/knowledge/files", get(files))
        .route(
            "/agent/knowledge/file",
            get(read_file).put(write_file).delete(delete_file),
        )
        .route("/agent/knowledge/move", post(move_file))
        .route(
            "/agent/knowledge/upload",
            post(upload::upload).layer(DefaultBodyLimit::max(MAX_TOTAL_BYTES + 256 * 1024)),
        )
}

#[derive(Debug, Deserialize)]
struct ScopeQuery {
    scope: String,
    #[serde(default)]
    team_id: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

/// 请求指的那份知识，以及发请求的人能不能改它。看不到的团队与不存在一样回 404。
pub(super) async fn knowledge_scope(
    state: &AppState,
    ctx: &IamContext,
    kind: &str,
    team_id: Option<&str>,
) -> Result<(Scope, Viewer)> {
    if kind == "personal" {
        return Err(Error::invalid(
            "knowledge belongs to the organization or a team",
        ));
    }

    let scope = requested_scope(state, ctx, kind, team_id).await?;
    let viewer = viewer(state, ctx).await?;

    if !viewer.can_see(&scope) {
        return Err(Error::not_found("knowledge not found"));
    }

    Ok((scope, viewer))
}

fn editable(viewer: &Viewer, scope: &Scope) -> Result<()> {
    if viewer.can_edit(scope) {
        return Ok(());
    }

    Err(Error::forbidden(match scope {
        Scope::Org => "the organization's knowledge is changed by holders of agent.manage",
        _ => "a team's knowledge is changed by its members",
    }))
}

fn base_json(
    base: Option<&KnowledgeBase>,
    scope: &Scope,
    team_name: Option<&str>,
    can_edit: bool,
) -> Value {
    json!({
        "scope": scope.kind(),
        "team_id": scope.team_id().map(|id| id.0.clone()),
        "team_name": team_name,
        "files": base.map_or(0, |base| base.files),
        "bytes": base.map_or(0, |base| base.bytes),
        "entry": base.is_some_and(|base| base.entry_updated_at.is_some()),
        "entry_updated_at_micros": base.and_then(|base| base.entry_updated_at.map(|at| at.0)),
        "changed_since_entry": base.map_or(0, |base| base.changed_since_entry),
        "updated_at_micros": base.and_then(|base| base.updated_at.map(|at| at.0)),
        "can_edit": can_edit,
    })
}

/// 组织的一份，加上能看到的每个团队一份（还没有文件的也列出来）。
#[permission("agent.use")]
async fn bases(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    let viewer = viewer(&state, &ctx).await?;
    let stored = state.agent.knowledge.bases(&ctx.org_id).await?;

    let by_key = stored
        .iter()
        .map(|base| (base.scope.key(), base))
        .collect::<HashMap<_, _>>();

    let mut list = vec![base_json(
        by_key.get(&Scope::Org.key()).copied(),
        &Scope::Org,
        None,
        viewer.can_edit(&Scope::Org),
    )];

    let mut teams = state.iam.teams.list(&ctx.org_id).await?;
    teams.sort_by_key(|team| team.name.to_lowercase());

    for team in teams {
        let scope = Scope::Team { team_id: team.id };

        if viewer.can_see(&scope) {
            list.push(base_json(
                by_key.get(&scope.key()).copied(),
                &scope,
                Some(&team.name),
                viewer.can_edit(&scope),
            ));
        }
    }

    Ok(Json(json!({
        "bases": list,
        "entry_file": ENTRY_FILE,
        "limits": {
            "max_file_bytes": MAX_FILE_BYTES,
            "max_total_bytes": MAX_TOTAL_BYTES,
            "max_files": MAX_FILES,
        },
    })))
}

#[permission("agent.use")]
async fn files(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<ScopeQuery>,
) -> Result<Json<Value>> {
    let (scope, viewer) =
        knowledge_scope(&state, &ctx, &query.scope, query.team_id.as_deref()).await?;

    let files = state.agent.knowledge.list(&ctx.org_id, &scope).await?;

    Ok(Json(json!({
        "files": files
            .iter()
            .map(|file| json!({
                "path": file.path,
                "size_bytes": file.size_bytes,
                "updated_by": file.updated_by.0,
                "updated_at_micros": file.updated_at.0,
            }))
            .collect::<Vec<_>>(),
        "can_edit": viewer.can_edit(&scope),
    })))
}

#[permission("agent.use")]
async fn read_file(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<ScopeQuery>,
) -> Result<Json<Value>> {
    let (scope, _) = knowledge_scope(&state, &ctx, &query.scope, query.team_id.as_deref()).await?;
    let path = query.path.as_deref().unwrap_or_default();

    let file = state
        .agent
        .knowledge
        .get(&ctx.org_id, &scope, path)
        .await?
        .ok_or_else(|| Error::not_found(format!("knowledge file `{path}` not found")))?;

    Ok(Json(json!({
        "path": file.path,
        "content": file.content,
        "size_bytes": file.size_bytes,
        "updated_by": file.updated_by.0,
        "updated_at_micros": file.updated_at.0,
    })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteReq {
    scope: String,
    #[serde(default)]
    team_id: Option<String>,
    path: String,
    content: String,
}

#[permission("agent.use")]
async fn write_file(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<WriteReq>,
) -> Result<Json<Value>> {
    let (scope, viewer) = knowledge_scope(&state, &ctx, &req.scope, req.team_id.as_deref()).await?;
    editable(&viewer, &scope)?;

    state
        .agent
        .knowledge
        .put(
            &ctx.org_id,
            &scope,
            &[(req.path.clone(), req.content)],
            &ctx.user_id,
        )
        .await?;

    audit(&state, &ctx, &scope, "written", json!({ "path": req.path })).await;
    Ok(Json(json!({ "written": true })))
}

#[permission("agent.use")]
async fn delete_file(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<ScopeQuery>,
) -> Result<Json<Value>> {
    let (scope, viewer) =
        knowledge_scope(&state, &ctx, &query.scope, query.team_id.as_deref()).await?;
    editable(&viewer, &scope)?;

    let path = query.path.unwrap_or_default();
    let deleted = state
        .agent
        .knowledge
        .delete(&ctx.org_id, &scope, &path)
        .await?;

    audit(&state, &ctx, &scope, "deleted", json!({ "path": path })).await;
    Ok(Json(json!({ "deleted": deleted })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MoveReq {
    scope: String,
    #[serde(default)]
    team_id: Option<String>,
    from: String,
    to: String,
}

#[permission("agent.use")]
async fn move_file(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<MoveReq>,
) -> Result<Json<Value>> {
    let (scope, viewer) = knowledge_scope(&state, &ctx, &req.scope, req.team_id.as_deref()).await?;
    editable(&viewer, &scope)?;

    state
        .agent
        .knowledge
        .rename(&ctx.org_id, &scope, &req.from, &req.to, &ctx.user_id)
        .await?;

    audit(
        &state,
        &ctx,
        &scope,
        "moved",
        json!({ "from": req.from, "to": req.to }),
    )
    .await;
    Ok(Json(json!({ "moved": true })))
}

pub(super) async fn audit(
    state: &AppState,
    ctx: &IamContext,
    scope: &Scope,
    action: &str,
    mut detail: Value,
) {
    detail["action"] = json!(action);

    activity_audit::record(
        state,
        ctx,
        "agent.knowledge.changed",
        "agent_knowledge",
        &scope.key(),
        detail,
    )
    .await;
}
