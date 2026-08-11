// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Saved views ：每个 org 共享的 SQL/PromQL 保存查询。
//!
//! 读走 `SavedViewRead`（Viewer+），写走 `SavedViewWrite`（Editor+）。
//! org 隔离在 repository 的 `WHERE org_id = $1` 谓词上强制——跨 org 的 id
//! 在 `get` 上落 NotFound、在 `update`/`delete` 上是 no-op。

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use serde::Deserialize;

use crate::{
    api::{AppState, http::middleware::ProtectedResource},
    app::iam::IamContext,
    domain::{
        iam::{permission, resource_permission},
        query::QueryLanguage,
        saved_view::SavedView,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/saved_views", get(list).post(create))
        .route("/saved_views/{id}", get(get_one).put(update).delete(delete))
}

#[async_trait::async_trait]
impl ProtectedResource for SavedView {
    type Id = Id;

    async fn load(state: &AppState, id: Self::Id) -> Result<Self> {
        state.platform.saved_view.get_by_id(&id).await
    }

    fn organization_id(&self) -> &Id {
        &self.org_id
    }

    fn resource_type(&self) -> &str {
        "saved_view"
    }

    fn resource_id(&self) -> &str {
        self.id.as_str()
    }
}

#[derive(Deserialize)]
struct ListQuery {
    /// `?pinned=true` 只返回置顶的视图（dashboard 侧边栏快捷入口用）。
    #[serde(default)]
    pinned: bool,
}

#[derive(Deserialize)]
struct WriteReq {
    name: String,
    language: QueryLanguage,
    statement: String,
    time_range_secs: u32,
    #[serde(default)]
    stream: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    pinned: bool,
}

/// `name` / `stream` columns are VARCHAR(255); guard here so over-length
/// input is a clean 400 instead of a Postgres truncation surfacing as 500.
const MAX_NAME_LEN: usize = 255;
/// Upper bound on the look-back window (~1 year). Also keeps `time_range_secs`
/// well inside i32 so the `as i32` store cast never wraps negative.
const MAX_RANGE_SECS: u32 = 366 * 24 * 3600;

fn validate(req: &WriteReq) -> Result<()> {
    if req.name.trim().is_empty() {
        return Err(Error::invalid("name cannot be empty"));
    }
    if req.name.chars().count() > MAX_NAME_LEN {
        return Err(Error::invalid("name must be at most 255 characters"));
    }
    if req.statement.trim().is_empty() {
        return Err(Error::invalid("statement cannot be empty"));
    }
    if let Some(stream) = &req.stream
        && stream.chars().count() > MAX_NAME_LEN
    {
        return Err(Error::invalid("stream must be at most 255 characters"));
    }
    if req.time_range_secs == 0 || req.time_range_secs > MAX_RANGE_SECS {
        return Err(Error::invalid(
            "time_range_secs must be between 1 and 31622400",
        ));
    }
    Ok(())
}

#[permission("saved_views.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<SavedView>>> {
    Ok(Json(
        state
            .platform
            .saved_view
            .list(&ctx.org_id, q.pinned)
            .await?,
    ))
}

#[resource_permission(
    action = "saved_views.read",
    resource = SavedView,
    id = Id::from_string(id),
    bind = view
)]
async fn get_one(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<SavedView>> {
    Ok(Json(view))
}

#[permission("saved_views.create")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<WriteReq>,
) -> Result<Json<SavedView>> {
    validate(&req)?;
    let now = TimestampMicros::now();
    let view = SavedView {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        owner_user_id: ctx.user_id.clone(),
        name: req.name,
        language: req.language,
        statement: req.statement,
        time_range_secs: req.time_range_secs,
        stream: req.stream,
        tags: req.tags,
        pinned: req.pinned,
        created_at: now,
        updated_at: now,
    };
    Ok(Json(state.platform.saved_view.create(view).await?))
}

#[resource_permission(
    action = "saved_views.edit",
    resource = SavedView,
    id = Id::from_string(id),
    bind = existing
)]
async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<WriteReq>,
) -> Result<Json<SavedView>> {
    validate(&req)?;
    let view = SavedView {
        id: existing.id,
        org_id: existing.org_id,
        owner_user_id: existing.owner_user_id,
        name: req.name,
        language: req.language,
        statement: req.statement,
        time_range_secs: req.time_range_secs,
        stream: req.stream,
        tags: req.tags,
        pinned: req.pinned,
        created_at: existing.created_at,
        updated_at: TimestampMicros::now(),
    };
    Ok(Json(state.platform.saved_view.update(view).await?))
}

#[resource_permission(
    action = "saved_views.delete",
    resource = SavedView,
    id = Id::from_string(id),
    bind = view
)]
async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<&'static str> {
    state
        .platform
        .saved_view
        .delete(&view.org_id, &view.id)
        .await?;
    Ok("deleted")
}
