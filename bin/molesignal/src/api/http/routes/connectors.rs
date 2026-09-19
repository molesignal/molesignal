// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Connectors CRUD。
//!
//! `pipelines.read` 可读，`pipelines.edit` 可写。`config_json` 中敏感字段在 list/get 响应做浅 mask
//! （`access_key`/`secret_key`/`push_token`/`webhook_url` 等）。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::permission,
    infra::connectors::{Connector, ConnectorKind},
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/connectors", get(list).post(create))
        .route("/connectors/{id}", get(get_one).put(update).delete(delete))
}

#[derive(Debug, Deserialize)]
pub struct CreateReq {
    pub name: String,
    pub kind: String, // "aws_cloudwatch_logs" 等
    pub config_json: Value,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateReq {
    pub name: String,
    pub kind: String,
    pub config_json: Value,
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize)]
pub struct ConnectorResp {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub config_json: Value,
    pub enabled: bool,
    pub last_run_at_micros: Option<i64>,
    pub created_at_micros: i64,
    pub updated_at_micros: i64,
}

const SENSITIVE_KEYS: &[&str] = &[
    "access_key",
    "secret_key",
    "push_token",
    "webhook_url",
    "client_secret",
    "password",
    "sasl_password",
];

fn mask_config(mut v: Value) -> Value {
    if let Value::Object(ref mut m) = v {
        for k in SENSITIVE_KEYS {
            if m.contains_key(*k) {
                m.insert((*k).to_string(), Value::String("***".to_string()));
            }
        }
    }
    v
}

fn to_resp(c: Connector) -> ConnectorResp {
    ConnectorResp {
        id: c.id.0,
        name: c.name,
        kind: c.kind,
        config_json: mask_config(c.config_json),
        enabled: c.enabled,
        last_run_at_micros: c.last_run_at.map(|t| t.0),
        created_at_micros: c.created_at.0,
        updated_at_micros: c.updated_at.0,
    }
}

#[permission("pipelines.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<ConnectorResp>>> {
    Ok(Json(
        state
            .storage
            .connectors
            .list(&ctx.org_id)
            .await?
            .into_iter()
            .map(to_resp)
            .collect(),
    ))
}

#[permission("pipelines.read")]
async fn get_one(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<ConnectorResp>> {
    let c = state.storage.connectors.get(&ctx.org_id, &Id(id)).await?;
    Ok(Json(to_resp(c)))
}

#[permission("pipelines.edit")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<CreateReq>,
) -> Result<Json<ConnectorResp>> {
    let _kind = req.kind.parse::<ConnectorKind>()?;
    let now = TimestampMicros::now();
    let c = Connector {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        name: req.name,
        kind: req.kind,
        config_json: req.config_json,
        enabled: req.enabled,
        last_run_at: None,
        created_at: now,
        updated_at: now,
    };
    let c = state.storage.connectors.create(c).await?;
    Ok(Json(to_resp(c)))
}

#[permission("pipelines.edit")]
async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<UpdateReq>,
) -> Result<Json<ConnectorResp>> {
    let _kind = req.kind.parse::<ConnectorKind>()?;
    let existing = state.storage.connectors.get(&ctx.org_id, &Id(id)).await?;
    let c = Connector {
        id: existing.id,
        org_id: ctx.org_id.clone(),
        name: req.name,
        kind: req.kind,
        config_json: req.config_json,
        enabled: req.enabled,
        last_run_at: existing.last_run_at,
        created_at: existing.created_at,
        updated_at: TimestampMicros::now(),
    };
    let c = state.storage.connectors.update(c).await?;
    Ok(Json(to_resp(c)))
}

#[permission("pipelines.edit")]
async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    state
        .storage
        .connectors
        .delete(&ctx.org_id, &Id(id))
        .await?;
    Ok(Json(serde_json::json!({"deleted": true})))
}
