// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Annotation HTTP routes（spec annotations）。

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use serde::{Deserialize, Serialize};

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::permission,
    infra::persistence::repositories::annotations::{Annotation, AnnotationFilter},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/annotations", get(list).post(create))
        .route("/annotations/{id}", get(get_one).delete(delete))
}

#[derive(Debug, Deserialize)]
pub struct ListParams {
    #[serde(default)]
    pub from: Option<i64>,
    #[serde(default)]
    pub to: Option<i64>,
    #[serde(default)]
    pub dashboard_id: Option<String>,
    #[serde(default)]
    pub stream: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateReq {
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub time_start_micros: i64,
    pub time_end_micros: i64,
    #[serde(default)]
    pub dashboard_id: Option<String>,
    #[serde(default)]
    pub stream_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Resp {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub time_start_micros: i64,
    pub time_end_micros: i64,
    pub dashboard_id: Option<String>,
    pub stream_name: Option<String>,
    pub created_by: String,
    pub created_at_micros: i64,
}

fn to_resp(a: Annotation) -> Resp {
    Resp {
        id: a.id.0,
        title: a.title,
        description: a.description,
        tags: a.tags,
        time_start_micros: a.time_start.0,
        time_end_micros: a.time_end.0,
        dashboard_id: a.dashboard_id.map(|i| i.0),
        stream_name: a.stream_name,
        created_by: a.created_by.0,
        created_at_micros: a.created_at.0,
    }
}

#[permission("dashboards.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(p): Query<ListParams>,
) -> Result<Json<Vec<Resp>>> {
    let f = AnnotationFilter {
        dashboard_id: p.dashboard_id.as_deref(),
        stream_name: p.stream.as_deref(),
        tag: p.tag.as_deref(),
        from_micros: p.from,
        to_micros: p.to,
    };
    Ok(Json(
        state
            .storage
            .annotations
            .list(&ctx.org_id, f)
            .await?
            .into_iter()
            .map(to_resp)
            .collect(),
    ))
}

#[permission("dashboards.read")]
async fn get_one(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Resp>> {
    // 跨 org 返 404（spec 要求，不泄漏存在性）
    match state.storage.annotations.get(&ctx.org_id, &Id(id)).await {
        Ok(a) => Ok(Json(to_resp(a))),
        Err(_) => Err(Error::not_found("annotation not found")),
    }
}

#[permission("dashboards.edit")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<CreateReq>,
) -> Result<Json<Resp>> {
    if req.time_end_micros < req.time_start_micros {
        return Err(Error::invalid("time_end_micros must >= time_start_micros"));
    }
    let a = Annotation {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        title: req.title,
        description: req.description,
        tags: req.tags,
        time_start: TimestampMicros(req.time_start_micros),
        time_end: TimestampMicros(req.time_end_micros),
        dashboard_id: req.dashboard_id.map(Id),
        stream_name: req.stream_name,
        created_by: ctx.user_id.clone(),
        created_at: TimestampMicros::now(),
    };
    let a = state.storage.annotations.create(a).await?;
    Ok(Json(to_resp(a)))
}

#[permission("dashboards.edit")]
async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    state
        .storage
        .annotations
        .delete(&ctx.org_id, &Id(id))
        .await?;
    Ok(Json(serde_json::json!({"deleted": true})))
}
