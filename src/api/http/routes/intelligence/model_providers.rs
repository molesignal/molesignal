// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Intelligence Model Gateway 配置路由。仅 Admin+。
//!
//! `GET    /api/v1/intelligence/settings/model-providers`                 list
//! `POST   /api/v1/intelligence/settings/model-providers`                 create
//! `PUT    /api/v1/intelligence/settings/model-providers/{id}`            update
//! `POST   /api/v1/intelligence/settings/model-providers/{id}/rotate-key` rotate key
//! `DELETE /api/v1/intelligence/settings/model-providers/{id}`            delete
//!
//! API key 永不回显：响应只含 `key_last4` + `key_set`；审计 payload 不含明文/密文。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    api::{AppState, http::routes::activity_audit},
    app::iam::IamContext,
    domain::iam::permission,
    infra::persistence::repositories::intelligence::model_providers::{
        ModelProvider, ModelProviderInput,
    },
    intelligence::chat::Provider,
    shared::{Error, Result, ids::Id},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/intelligence/settings/model-providers",
            get(list).post(create),
        )
        .route(
            "/intelligence/settings/model-providers/{id}",
            axum::routing::put(update).delete(delete),
        )
        .route(
            "/intelligence/settings/model-providers/{id}/rotate-key",
            axum::routing::post(rotate_key),
        )
}

fn require_license(state: &AppState) -> Result<()> {
    if !state
        .platform
        .license
        .has_feature(crate::intelligence::FEATURE)
    {
        return Err(Error::forbidden("intelligence feature not licensed"));
    }
    Ok(())
}

const DEFAULT_TIMEOUT_MS: i64 = 30_000;

#[derive(Debug, Serialize)]
pub struct ProviderResp {
    pub id: String,
    pub provider: String,
    pub name: String,
    pub base_url: Option<String>,
    pub default_model: String,
    pub enabled: bool,
    pub timeout_ms: i64,
    pub max_tokens: Option<i64>,
    pub key_last4: Option<String>,
    pub key_set: bool,
    pub created_at_micros: i64,
    pub updated_at_micros: i64,
}

fn to_resp(p: ModelProvider) -> ProviderResp {
    ProviderResp {
        id: p.id.0,
        provider: p.provider,
        name: p.name,
        base_url: p.base_url,
        default_model: p.default_model,
        enabled: p.enabled,
        timeout_ms: p.timeout_ms,
        max_tokens: p.max_tokens,
        key_last4: p.key_last4,
        key_set: p.key_set,
        created_at_micros: p.created_at.0,
        updated_at_micros: p.updated_at.0,
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateReq {
    pub provider: String,
    pub name: String,
    #[serde(default)]
    pub base_url: Option<String>,
    pub default_model: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub timeout_ms: Option<i64>,
    #[serde(default)]
    pub max_tokens: Option<i64>,
    /// write-only：明文 API key；seal 后落库，绝不回显。
    #[serde(default)]
    pub api_key: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct UpdateReq {
    pub provider: String,
    pub name: String,
    #[serde(default)]
    pub base_url: Option<String>,
    pub default_model: String,
    pub enabled: bool,
    #[serde(default)]
    pub timeout_ms: Option<i64>,
    #[serde(default)]
    pub max_tokens: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct RotateReq {
    pub api_key: String,
}

/// 校验 provider 类型串合法（openai|anthropic|openai_compatible）。
fn validate_provider(provider: &str) -> Result<()> {
    Provider::parse(provider).map(|_| ())
}

#[permission("intelligence.use")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<ProviderResp>>> {
    require_license(&state)?;
    let rows = state.intelligence.model_providers.list(&ctx.org_id).await?;
    Ok(Json(rows.into_iter().map(to_resp).collect()))
}

#[permission("intelligence.manage")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<CreateReq>,
) -> Result<Json<ProviderResp>> {
    require_license(&state)?;
    validate_provider(&req.provider)?;
    if req.name.trim().is_empty() || req.default_model.trim().is_empty() {
        return Err(Error::invalid("name and default_model are required"));
    }
    let id = Id::new();
    let input = ModelProviderInput {
        id: id.clone(),
        org_id: ctx.org_id.clone(),
        provider: req.provider.clone(),
        name: req.name.clone(),
        base_url: req.base_url.clone(),
        default_model: req.default_model.clone(),
        enabled: req.enabled,
        timeout_ms: req.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS),
        max_tokens: req.max_tokens,
    };
    let api_key = req.api_key.as_deref().filter(|k| !k.is_empty());
    let saved = state
        .intelligence
        .model_providers
        .create(input, api_key)
        .await?;
    // 审计：含 masked 元数据，不含明文/密文。
    activity_audit::record(
        &state,
        &ctx,
        "intelligence.model_provider.created",
        "intelligence_model_provider",
        &saved.id.0,
        json!({
            "provider": saved.provider,
            "name": saved.name,
            "default_model": saved.default_model,
            "enabled": saved.enabled,
            "key_last4": saved.key_last4,
            "key_set": saved.key_set,
        }),
    )
    .await;
    Ok(Json(to_resp(saved)))
}

#[permission("intelligence.manage")]
async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<UpdateReq>,
) -> Result<Json<ProviderResp>> {
    require_license(&state)?;
    validate_provider(&req.provider)?;
    let input = ModelProviderInput {
        id: Id(id.clone()),
        org_id: ctx.org_id.clone(),
        provider: req.provider.clone(),
        name: req.name.clone(),
        base_url: req.base_url.clone(),
        default_model: req.default_model.clone(),
        enabled: req.enabled,
        timeout_ms: req.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS),
        max_tokens: req.max_tokens,
    };
    let saved = state.intelligence.model_providers.update(input).await?;
    activity_audit::record(
        &state,
        &ctx,
        "intelligence.model_provider.updated",
        "intelligence_model_provider",
        &saved.id.0,
        json!({
            "provider": saved.provider,
            "name": saved.name,
            "default_model": saved.default_model,
            "enabled": saved.enabled,
        }),
    )
    .await;
    Ok(Json(to_resp(saved)))
}

#[permission("intelligence.manage")]
async fn rotate_key(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<RotateReq>,
) -> Result<Json<ProviderResp>> {
    require_license(&state)?;
    if req.api_key.trim().is_empty() {
        return Err(Error::invalid("api_key is required"));
    }
    let saved = state
        .intelligence
        .model_providers
        .rotate_key(&ctx.org_id, &Id(id.clone()), &req.api_key)
        .await?;
    activity_audit::record(
        &state,
        &ctx,
        "intelligence.model_provider.key_rotated",
        "intelligence_model_provider",
        &saved.id.0,
        json!({ "key_last4": saved.key_last4 }),
    )
    .await;
    Ok(Json(to_resp(saved)))
}

#[permission("intelligence.manage")]
async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    require_license(&state)?;
    state
        .intelligence
        .model_providers
        .delete(&ctx.org_id, &Id(id.clone()))
        .await?;
    activity_audit::record(
        &state,
        &ctx,
        "intelligence.model_provider.deleted",
        "intelligence_model_provider",
        &id,
        json!({}),
    )
    .await;
    Ok(Json(json!({ "deleted": true })))
}
