// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Annotation HTTP routes（spec annotations）。

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};

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
        .route(
            "/annotations/{id}",
            get(get_one).put(update).patch(update).delete(delete),
        )
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

#[derive(Debug, Deserialize)]
pub struct UpdateReq {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Patch<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub time_start_micros: Option<i64>,
    #[serde(default)]
    pub time_end_micros: Option<i64>,
    #[serde(default)]
    pub dashboard_id: Patch<String>,
    #[serde(default)]
    pub stream_name: Patch<String>,
}

#[derive(Debug, Default)]
pub enum Patch<T> {
    #[default]
    Missing,
    Value(Option<T>),
}

impl<'de, T: DeserializeOwned> Deserialize<'de> for Patch<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        Option::<T>::deserialize(deserializer).map(Self::Value)
    }
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
    let mut a = Annotation {
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
    normalize_annotation(&mut a)?;
    let a = state.storage.annotations.create(a).await?;
    Ok(Json(to_resp(a)))
}

#[permission("dashboards.edit")]
async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<UpdateReq>,
) -> Result<Json<Resp>> {
    let mut annotation = state
        .storage
        .annotations
        .get(&ctx.org_id, &Id(id))
        .await
        .map_err(|_| Error::not_found("annotation not found"))?;
    if let Some(title) = req.title {
        annotation.title = title;
    }
    if let Patch::Value(description) = req.description {
        annotation.description = description;
    }
    if let Some(tags) = req.tags {
        annotation.tags = tags;
    }
    if let Some(time_start) = req.time_start_micros {
        annotation.time_start = TimestampMicros(time_start);
    }
    if let Some(time_end) = req.time_end_micros {
        annotation.time_end = TimestampMicros(time_end);
    }
    if let Patch::Value(dashboard_id) = req.dashboard_id {
        annotation.dashboard_id = dashboard_id.map(Id);
    }
    if let Patch::Value(stream_name) = req.stream_name {
        annotation.stream_name = stream_name;
    }
    normalize_annotation(&mut annotation)?;
    let saved = state.storage.annotations.update(annotation).await?;
    Ok(Json(to_resp(saved)))
}

fn normalize_annotation(annotation: &mut Annotation) -> Result<()> {
    annotation.title = annotation.title.trim().to_string();
    validate_annotation(
        &annotation.title,
        annotation.time_start.0,
        annotation.time_end.0,
    )?;
    annotation.tags = annotation
        .tags
        .drain(..)
        .map(|tag| tag.trim().to_string())
        .filter(|tag| !tag.is_empty())
        .collect();
    annotation.tags.sort();
    annotation.tags.dedup();
    if annotation.tags.len() > 64 || annotation.tags.iter().any(|tag| tag.len() > 128) {
        return Err(Error::invalid(
            "annotations support at most 64 tags of 128 bytes each",
        ));
    }
    Ok(())
}

fn validate_annotation(title: &str, start: i64, end: i64) -> Result<()> {
    let title = title.trim();
    if title.is_empty() || title.chars().count() > 500 {
        return Err(Error::invalid(
            "title must contain between 1 and 500 characters",
        ));
    }
    if end < start {
        return Err(Error::invalid(
            "time_end_micros must be greater than or equal to time_start_micros",
        ));
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_patch_distinguishes_omitted_null_and_value() {
        let omitted: UpdateReq = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(matches!(omitted.description, Patch::Missing));

        let cleared: UpdateReq =
            serde_json::from_value(serde_json::json!({"description": null})).unwrap();
        assert!(matches!(cleared.description, Patch::Value(None)));

        let updated: UpdateReq =
            serde_json::from_value(serde_json::json!({"description": "note"})).unwrap();
        assert!(matches!(
            updated.description,
            Patch::Value(Some(value)) if value == "note"
        ));
    }
}
