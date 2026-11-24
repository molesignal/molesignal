// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Changes: `GET /changes` lists every change of a stretch of time a reader may read (reported
//! changes, annotations, versions APM first saw, alert rule edits); the other routes keep the
//! annotations people put on a time range. Pipelines report their changes through
//! `POST /apm/deployments`, webhooks that cannot be shaped like it through
//! `POST /api/v1/intake/changes` ([`intake`]).

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use domain::iam::{IamContext, permission};
use kernel::{
    Error, Result,
    ids::Id,
    time::{TimeRange, TimestampMicros},
};
use postgres::repositories::annotations::Annotation;
use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};

use crate::{
    api::AppState,
    app::changes::{ChangeEntry, ChangeFeed, ChangeFeedKind, ChangeFeedQuery, MOST_CHANGES},
};

mod intake;

pub use intake::{CHANGE_INTAKE_PATH, public_routes as intake_routes};

/// How far back a list reaches when it names no start.
const DEFAULT_SPAN_MICROS: i64 = 86_400_000_000;
/// The most services a list may name.
const MOST_SERVICES: usize = 50;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/changes", get(list).post(create))
        .route(
            "/changes/{id}",
            get(get_one).put(update).patch(update).delete(delete),
        )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListParams {
    #[serde(default)]
    pub from: Option<i64>,
    #[serde(default)]
    pub to: Option<i64>,
    /// Comma-separated service names.
    #[serde(default)]
    pub services: Option<String>,
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default)]
    pub environment: Option<String>,
    /// Comma-separated kinds; every kind the reader may read when left out.
    #[serde(default)]
    pub kinds: Option<String>,
    #[serde(default)]
    pub dashboard_id: Option<String>,
    #[serde(default)]
    pub stream: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

/// The parts of a comma-separated value, trimmed, the empty ones left out.
fn listed(value: Option<&str>) -> Vec<String> {
    value
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Where changes are read from.
pub(crate) fn change_feed(state: &AppState) -> ChangeFeed {
    ChangeFeed {
        catalog: state.telemetry.apm_catalog.clone(),
        annotations: state.storage.annotations.clone(),
        apm: state.telemetry.apm_query.clone(),
        rules: state.alerting.service.rules.clone(),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateReq {
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub time_start: i64,
    pub time_end: i64,
    #[serde(default)]
    pub dashboard_id: Option<String>,
    #[serde(default)]
    pub stream_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateReq {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Patch<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub time_start: Option<i64>,
    #[serde(default)]
    pub time_end: Option<i64>,
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
#[serde(rename_all = "camelCase")]
pub struct Resp {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub time_start: i64,
    pub time_end: i64,
    pub dashboard_id: Option<String>,
    pub stream_name: Option<String>,
    pub created_by: String,
    pub created_at: i64,
}

fn to_resp(a: Annotation) -> Resp {
    Resp {
        id: a.id.0,
        title: a.title,
        description: a.description,
        tags: a.tags,
        time_start: a.time_start.0,
        time_end: a.time_end.0,
        dashboard_id: a.dashboard_id.map(|i| i.0),
        stream_name: a.stream_name,
        created_by: a.created_by.0,
        created_at: a.created_at.0,
    }
}

/// Every change of the range the caller may read, the latest first. Each kind asks its own
/// permission; one the caller lacks is left out rather than refused.
#[permission(any(
    "apm.deployments.read",
    "apm.services.read",
    "dashboards.annotations.read",
    "alerts.rules.read",
    "sys.telemetry.read"
))]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(p): Query<ListParams>,
) -> Result<Json<Vec<ChangeEntry>>> {
    let to = p.to.unwrap_or_else(|| TimestampMicros::now().0);
    let from = p.from.unwrap_or(to - DEFAULT_SPAN_MICROS);

    if from > to {
        return Err(Error::invalid("from is after to"));
    }

    let services = listed(p.services.as_deref());

    if services.len() > MOST_SERVICES {
        return Err(Error::invalid(format!(
            "name at most {MOST_SERVICES} services"
        )));
    }

    let kinds = listed(p.kinds.as_deref())
        .iter()
        .map(|kind| {
            ChangeFeedKind::parse(kind)
                .ok_or_else(|| Error::invalid(format!("unknown change kind `{kind}`")))
        })
        .collect::<Result<Vec<_>>>()?;

    let query = ChangeFeedQuery {
        range: TimeRange::new(TimestampMicros(from), TimestampMicros(to)),
        services,
        namespace: p.namespace.filter(|value| !value.trim().is_empty()),
        environment: p.environment.filter(|value| !value.trim().is_empty()),
        kinds,
        dashboard_id: p.dashboard_id,
        stream: p.stream,
        limit: p.limit.unwrap_or(MOST_CHANGES).clamp(1, MOST_CHANGES),
    };

    Ok(Json(
        change_feed(&state)
            .read(&ctx.org_id, &query, |permission| {
                ctx.has_permission(permission)
            })
            .await?,
    ))
}

#[permission("dashboards.annotations.read")]
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

#[permission("dashboards.annotations.manage")]
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
        time_start: TimestampMicros(req.time_start),
        time_end: TimestampMicros(req.time_end),
        dashboard_id: req.dashboard_id.map(Id),
        stream_name: req.stream_name,
        created_by: ctx.user_id.clone(),
        created_at: TimestampMicros::now(),
    };

    normalize_annotation(&mut a)?;
    let a = state.storage.annotations.create(a).await?;
    Ok(Json(to_resp(a)))
}

#[permission("dashboards.annotations.manage")]
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

    if let Some(time_start) = req.time_start {
        annotation.time_start = TimestampMicros(time_start);
    }

    if let Some(time_end) = req.time_end {
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
            "timeEnd must be greater than or equal to timeStart",
        ));
    }

    Ok(())
}

#[permission("dashboards.annotations.manage")]
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
