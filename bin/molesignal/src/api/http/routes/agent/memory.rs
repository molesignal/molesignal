// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 记忆的 HTTP 路由（「知识」页签的「记忆」视图，以及对话里的撤销）。
//!
//! - `GET    /api/v1/agent/memories`              个人或某个团队的记忆（`?scope=personal|team&team_id=`）
//! - `PATCH  /api/v1/agent/memories/{id}`         改一条
//! - `DELETE /api/v1/agent/memories/{id}`         删一条（也用来撤销一次「记住」）
//! - `POST   /api/v1/agent/memories/{id}/restore` 撤销一次「忘掉」
//! - `GET|PUT /api/v1/agent/memory-settings`      组织的「会话结束后提炼」开关（改要 `agent.manage`）
//!
//! 个人记忆只有本人能看、能改；团队记忆由团队成员与管理员看和改。记忆只由对话写下，这里不新建。

use std::collections::HashMap;

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    routing::{get, patch, post},
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::viewer::{requested_scope, viewer};
use crate::{
    agent::{
        memory::{Memory, MemoryDraft, MemoryKind},
        scope::Scope,
    },
    api::{AppState, http::routes::activity_audit},
    app::iam::IamContext,
    domain::iam::permission,
    shared::{Error, Result, ids::Id},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/agent/memories", get(list_memories))
        .route(
            "/agent/memories/{id}",
            patch(update_memory).delete(delete_memory),
        )
        .route("/agent/memories/{id}/restore", post(restore_memory))
        .route(
            "/agent/memory-settings",
            get(memory_settings).put(set_memory_settings),
        )
}

/// 这个人能不能看、能不能改这个作用域的记忆。
async fn allowed(state: &AppState, ctx: &IamContext, scope: &Scope) -> Result<bool> {
    match scope {
        Scope::Personal { owner_id } => Ok(*owner_id == ctx.user_id),
        Scope::Team { .. } => Ok(viewer(state, ctx).await?.can_edit(scope)),
        Scope::Org => Ok(false),
    }
}

async fn owned_memory(state: &AppState, ctx: &IamContext, id: &str) -> Result<Memory> {
    let memory = state
        .agent
        .memory
        .get(&ctx.org_id, &Id::from_string(id))
        .await?;

    if !allowed(state, ctx, &memory.scope).await? {
        return Err(Error::not_found(format!("memory `{id}` not found")));
    }

    Ok(memory)
}

fn memory_json(memory: &Memory) -> Value {
    json!({
        "id": memory.id.0,
        "scope": memory.scope.kind(),
        "team_id": memory.scope.team_id().map(|id| id.0.clone()),
        "kind": memory.kind,
        "name": memory.name,
        "description": memory.description,
        "body": memory.body,
        "source_chat_id": memory.source_chat_id.as_ref().map(|id| id.0.clone()),
        "created_by": memory.created_by.0,
        "updated_by": memory.updated_by.0,
        "created_at_micros": memory.created_at.0,
        "updated_at_micros": memory.updated_at.0,
        "forgotten_at_micros": memory.forgotten_at.map(|at| at.0),
    })
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    team_id: Option<String>,
}

#[permission("agent.use")]
async fn list_memories(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>> {
    let kind = query.scope.as_deref().unwrap_or("personal");

    if kind == "org" {
        return Err(Error::invalid("memories belong to a person or a team"));
    }

    let scope = requested_scope(&state, &ctx, kind, query.team_id.as_deref()).await?;

    if !allowed(&state, &ctx, &scope).await? {
        return Err(Error::not_found("memories not found"));
    }

    let memories = state.agent.memory.list(&ctx.org_id, &scope).await?;
    let titles = source_titles(&state, &ctx, &scope, &memories).await;

    let rows = memories
        .iter()
        .map(|memory| {
            let mut row = memory_json(memory);
            let title = memory
                .source_chat_id
                .as_ref()
                .and_then(|id| titles.get(&id.0));

            row["source_chat_title"] = json!(title);
            row
        })
        .collect::<Vec<_>>();

    Ok(Json(json!({ "memories": rows })))
}

/// 记忆出处的会话名，按会话 ID：只有还在、这个作用域的会话（个人记忆是本人的个人会话，团队
/// 记忆是这个团队的会话）。读不出时当作没有，记忆照样列出。
async fn source_titles(
    state: &AppState,
    ctx: &IamContext,
    scope: &Scope,
    memories: &[Memory],
) -> HashMap<String, String> {
    let mut ids: Vec<Id> = memories
        .iter()
        .filter_map(|memory| memory.source_chat_id.clone())
        .collect();

    ids.sort_by(|a, b| a.0.cmp(&b.0));
    ids.dedup_by(|a, b| a.0 == b.0);

    let chats = state
        .agent
        .chats
        .get_chats(&ctx.org_id, &ids)
        .await
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "the chats memories came from could not be read");
            Vec::new()
        });

    chats
        .into_iter()
        .filter(|chat| match scope {
            Scope::Personal { owner_id } => chat.team_id.is_none() && chat.user_id == *owner_id,
            Scope::Team { team_id } => chat.team_id.as_deref() == Some(team_id.0.as_str()),
            Scope::Org => false,
        })
        .map(|chat| (chat.id.0, chat.title))
        .collect()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateReq {
    kind: MemoryKind,
    name: String,
    description: String,
    body: String,
}

#[permission("agent.use")]
async fn update_memory(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<UpdateReq>,
) -> Result<Json<Value>> {
    let memory = owned_memory(&state, &ctx, &id).await?;

    let draft = MemoryDraft {
        kind: req.kind,
        name: req.name,
        description: req.description,
        body: req.body,
    };

    let memory = state
        .agent
        .memory
        .update(&ctx.org_id, &memory.id, &draft, &ctx.user_id)
        .await?;

    audit(&state, &ctx, &memory, "updated").await;
    Ok(Json(memory_json(&memory)))
}

#[permission("agent.use")]
async fn delete_memory(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let memory = owned_memory(&state, &ctx, &id).await?;
    let deleted = state.agent.memory.delete(&ctx.org_id, &memory.id).await?;

    audit(&state, &ctx, &memory, "deleted").await;
    Ok(Json(json!({ "deleted": deleted })))
}

#[permission("agent.use")]
async fn restore_memory(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let memory = owned_memory(&state, &ctx, &id).await?;

    let memory = state
        .agent
        .memory
        .restore(&ctx.org_id, &memory.id, &ctx.user_id)
        .await?;

    audit(&state, &ctx, &memory, "restored").await;
    Ok(Json(memory_json(&memory)))
}

async fn audit(state: &AppState, ctx: &IamContext, memory: &Memory, action: &str) {
    activity_audit::record(
        state,
        ctx,
        "agent.memory.changed",
        "agent_memory",
        &memory.id.0,
        json!({ "action": action, "scope": memory.scope.key(), "name": memory.name }),
    )
    .await;
}

#[permission("agent.use")]
async fn memory_settings(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    let settings = state.agent.memory.settings(&ctx.org_id).await?;

    Ok(Json(json!({
        "distill_after_chat": settings.distill_after_chat,
        "can_edit": ctx.has_permission("agent.manage"),
        "updated_at_micros": settings.updated_at.map(|at| at.0),
    })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsReq {
    distill_after_chat: bool,
}

#[permission("agent.manage")]
async fn set_memory_settings(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<SettingsReq>,
) -> Result<Json<Value>> {
    let settings = state
        .agent
        .memory
        .set_settings(&ctx.org_id, req.distill_after_chat, &ctx.user_id)
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "agent.memory.settings_changed",
        "agent_memory_settings",
        &ctx.org_id.0,
        json!({ "distill_after_chat": settings.distill_after_chat }),
    )
    .await;

    Ok(Json(json!({
        "distill_after_chat": settings.distill_after_chat,
        "can_edit": true,
        "updated_at_micros": settings.updated_at.map(|at| at.0),
    })))
}
