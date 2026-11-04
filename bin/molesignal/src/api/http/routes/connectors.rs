// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Connectors CRUD。
//!
//! `pipelines.read` 可读，`pipelines.edit` 可写。`config_json` 中敏感字段在响应里做浅 mask
//! （`access_key`/`secret_key`/`push_token` 等，见 [`crate::infra::connectors::config`]），响应另带
//! `secret_keys` 列出已存有值的敏感字段。更新时把 mask 原样传回表示「保留已存的值」。
//! 保存前按 kind 校验配置，缺地址字段或凭据不成对时返回 400。
//!
//! `POST /connectors/test` 在保存前试连一份配置（带 `id` 时 mask 指向该 connector 已存的密钥），
//! 需要 `pipelines.edit`：它用已存的密钥，也替调用方向外连接。`GET /connectors/aws-external-id`
//! 给出本组织扮演 AWS 角色时用的外部 ID，供写进角色的信任策略。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::permission,
    infra::connectors::{
        Connector, ConnectorKind, aws,
        check::{self, CheckOutcome},
        config,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/connectors", get(list).post(create))
        .route("/connectors/test", post(test))
        .route("/connectors/aws-external-id", get(aws_external_id))
        .route("/connectors/{id}", get(get_one).put(update).delete(delete))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateReq {
    pub name: String,
    pub kind: String, // "aws_cloudwatch_logs" 等
    pub config_json: Value,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateReq {
    pub name: String,
    pub kind: String,
    pub config_json: Value,
    pub enabled: bool,
}

/// Settings to try. With `id`, a masked secret stands for the one that connector holds.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestReq {
    pub kind: String,
    pub config_json: Value,
    #[serde(default)]
    pub id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AwsExternalIdResp {
    pub external_id: String,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorResp {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub config_json: Value,
    /// Secrets in `config_json` that hold a value. Each reads as the mask there; an update that
    /// sends the mask back keeps the stored value.
    pub secret_keys: Vec<String>,
    pub enabled: bool,
    pub last_run_at: Option<i64>,
    /// How a pull source's reading goes: `position`, `last_error`, `last_warning`.
    pub read_state: Value,
    pub created_at: i64,
    pub updated_at: i64,
}

fn to_resp(c: Connector) -> ConnectorResp {
    ConnectorResp {
        id: c.id.0,
        name: c.name,
        kind: c.kind,
        secret_keys: config::stored_secret_keys(&c.config_json),
        config_json: config::mask_secrets(c.config_json),
        enabled: c.enabled,
        last_run_at: c.last_run_at.map(|t| t.0),
        read_state: c.read_state,
        created_at: c.created_at.0,
        updated_at: c.updated_at.0,
    }
}

#[permission("connectors.read")]
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

#[permission("connectors.read")]
async fn get_one(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<ConnectorResp>> {
    let c = state.storage.connectors.get(&ctx.org_id, &Id(id)).await?;
    Ok(Json(to_resp(c)))
}

#[permission("connectors.manage")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<CreateReq>,
) -> Result<Json<ConnectorResp>> {
    let kind = req.kind.parse::<ConnectorKind>()?;
    // Nothing is stored yet, so a mask in a new connector stands for no secret at all.
    let config_json = config::keep_stored_secrets(req.config_json, None);
    config::validate(kind, &config_json)?;
    let config_json = config::with_derived(kind, config_json);
    let now = TimestampMicros::now();

    let c = Connector {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        name: req.name,
        kind: req.kind,
        config_json,
        enabled: req.enabled,
        last_run_at: None,
        read_state: Value::Null,
        created_at: now,
        updated_at: now,
    };

    let c = state.storage.connectors.create(c).await?;
    Ok(Json(to_resp(c)))
}

#[permission("connectors.manage")]
async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<UpdateReq>,
) -> Result<Json<ConnectorResp>> {
    let kind = req.kind.parse::<ConnectorKind>()?;
    let existing = state.storage.connectors.get(&ctx.org_id, &Id(id)).await?;
    // A secret sent back masked is the one already stored. Another kind's secrets are not carried over.
    let stored = (existing.kind == req.kind).then_some(&existing.config_json);
    let config_json = config::keep_stored_secrets(req.config_json, stored);
    config::validate(kind, &config_json)?;
    let config_json = config::with_derived(kind, config_json);

    let c = Connector {
        id: existing.id,
        org_id: ctx.org_id.clone(),
        name: req.name,
        kind: req.kind,
        config_json,
        enabled: req.enabled,
        last_run_at: existing.last_run_at,
        read_state: existing.read_state,
        created_at: existing.created_at,
        updated_at: TimestampMicros::now(),
    };

    let c = state.storage.connectors.update(c).await?;
    Ok(Json(to_resp(c)))
}

#[permission("connectors.manage")]
async fn test(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<TestReq>,
) -> Result<Json<CheckOutcome>> {
    let kind = req.kind.parse::<ConnectorKind>()?;

    let existing = match req.id {
        Some(id) => Some(state.storage.connectors.get(&ctx.org_id, &Id(id)).await?),
        None => None,
    };

    let stored = existing
        .as_ref()
        .filter(|saved| saved.kind == req.kind)
        .map(|saved| &saved.config_json);
    let config_json = config::keep_stored_secrets(req.config_json, stored);
    config::validate(kind, &config_json)?;
    let now = TimestampMicros::now();

    // The settings as they would be saved; a saved connector keeps its id, which names its
    // sessions with AWS.
    let connector = Connector {
        id: existing.map(|saved| saved.id).unwrap_or_else(Id::new),
        org_id: ctx.org_id.clone(),
        name: String::new(),
        kind: req.kind,
        config_json,
        enabled: true,
        last_run_at: None,
        read_state: Value::Null,
        created_at: now,
        updated_at: now,
    };

    Ok(Json(check::check(&connector).await))
}

#[permission("connectors.read")]
async fn aws_external_id(Extension(ctx): Extension<IamContext>) -> Result<Json<AwsExternalIdResp>> {
    Ok(Json(AwsExternalIdResp {
        external_id: aws::external_id(&ctx.org_id),
    }))
}

#[permission("connectors.manage")]
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
