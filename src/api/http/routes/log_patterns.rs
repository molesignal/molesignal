// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Log Patterns HTTP routes（spec log-patterns）。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::{Deserialize, Serialize};

use crate::{
    api::{
        AppState,
        http::federation::{delete_payload, emit_cud},
    },
    app::iam::IamContext,
    domain::{
        federation::{CudAction, ResourceKind},
        iam::permission,
    },
    infra::persistence::repositories::log_patterns::{LogPattern, compile_check},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/log_patterns", get(list).post(create))
        .route(
            "/log_patterns/{id}",
            get(get_one).put(update).delete(delete),
        )
}

#[derive(Debug, Deserialize)]
pub struct CreateReq {
    pub name: String,
    pub regex: String,
    #[serde(default)]
    pub capture_groups: Vec<String>,
    pub category: String,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub stream_filter: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateReq {
    pub name: String,
    pub regex: String,
    #[serde(default)]
    pub capture_groups: Vec<String>,
    pub category: String,
    pub priority: i32,
    #[serde(default)]
    pub stream_filter: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Resp {
    pub id: String,
    pub name: String,
    pub regex: String,
    pub capture_groups: Vec<String>,
    pub category: String,
    pub priority: i32,
    pub stream_filter: Option<String>,
    pub created_at_micros: i64,
    pub updated_at_micros: i64,
}

fn to_resp(p: LogPattern) -> Resp {
    Resp {
        id: p.id.0,
        name: p.name,
        regex: p.regex,
        capture_groups: p.capture_groups,
        category: p.category,
        priority: p.priority,
        stream_filter: p.stream_filter,
        created_at_micros: p.created_at.0,
        updated_at_micros: p.updated_at.0,
    }
}

#[permission("streams.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<Resp>>> {
    Ok(Json(
        state
            .storage
            .log_patterns
            .list(&ctx.org_id)
            .await?
            .into_iter()
            .map(to_resp)
            .collect(),
    ))
}

#[permission("streams.read")]
async fn get_one(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Resp>> {
    let p = state.storage.log_patterns.get(&ctx.org_id, &Id(id)).await?;
    Ok(Json(to_resp(p)))
}

#[permission("streams.configure")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<CreateReq>,
) -> Result<Json<Resp>> {
    if req.name.is_empty() || req.category.is_empty() {
        return Err(Error::invalid("name and category must not be empty"));
    }
    compile_check(&req.regex)?;
    let now = TimestampMicros::now();
    let p = LogPattern {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        name: req.name,
        regex: req.regex,
        capture_groups: req.capture_groups,
        category: req.category,
        priority: req.priority,
        stream_filter: req.stream_filter,
        created_at: now,
        updated_at: now,
    };
    let p = state.storage.log_patterns.create(p).await?;
    emit_cud(
        &state,
        &ctx.org_id,
        ResourceKind::LogPattern,
        CudAction::Created,
        &p.id.0,
        &p,
    )
    .await;
    Ok(Json(to_resp(p)))
}

#[permission("streams.configure")]
async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<UpdateReq>,
) -> Result<Json<Resp>> {
    compile_check(&req.regex)?;
    let existing = state.storage.log_patterns.get(&ctx.org_id, &Id(id)).await?;
    let p = LogPattern {
        id: existing.id,
        org_id: ctx.org_id.clone(),
        name: req.name,
        regex: req.regex,
        capture_groups: req.capture_groups,
        category: req.category,
        priority: req.priority,
        stream_filter: req.stream_filter,
        created_at: existing.created_at,
        updated_at: TimestampMicros::now(),
    };
    let p = state.storage.log_patterns.update(p).await?;
    emit_cud(
        &state,
        &ctx.org_id,
        ResourceKind::LogPattern,
        CudAction::Updated,
        &p.id.0,
        &p,
    )
    .await;
    Ok(Json(to_resp(p)))
}

#[permission("streams.configure")]
async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    state
        .storage
        .log_patterns
        .delete(&ctx.org_id, &Id(id.clone()))
        .await?;
    emit_cud(
        &state,
        &ctx.org_id,
        ResourceKind::LogPattern,
        CudAction::Deleted,
        &id,
        &delete_payload(&id),
    )
    .await;
    Ok(Json(serde_json::json!({"deleted": true})))
}
