// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent Model Gateway 配置路由。仅 Admin+。
//!
//! `GET    /api/v1/agent/settings/model-providers`                 list
//! `POST   /api/v1/agent/settings/model-providers`                 create
//! `PUT    /api/v1/agent/settings/model-providers/{id}`            update
//! `POST   /api/v1/agent/settings/model-providers/{id}/rotate-key` rotate key
//! `DELETE /api/v1/agent/settings/model-providers/{id}`            delete
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
    agent::{
        chat::Provider,
        usage::{Currency, validate_price},
    },
    api::{AppState, http::routes::activity_audit},
    app::iam::IamContext,
    domain::iam::permission,
    infra::persistence::repositories::agent::model_providers::{ModelProvider, ModelProviderInput},
    shared::{Error, Result, ids::Id},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/agent/settings/model-providers", get(list).post(create))
        .route(
            "/agent/settings/model-providers/{id}",
            axum::routing::put(update).delete(delete),
        )
        .route(
            "/agent/settings/model-providers/{id}/rotate-key",
            axum::routing::post(rotate_key),
        )
}

/// 多久没有收到模型服务的任何数据就判定失败；非流式调用要等整段回答生成完，所以给得宽。
const DEFAULT_TIMEOUT_MS: i64 = 120_000;
const MIN_TIMEOUT_MS: i64 = 1_000;
const MAX_TIMEOUT_MS: i64 = 600_000;

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
    pub price_currency: Currency,
    pub input_price_per_mtok: Option<f64>,
    pub cached_input_price_per_mtok: Option<f64>,
    pub output_price_per_mtok: Option<f64>,
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
        price_currency: p.price_currency,
        input_price_per_mtok: p.input_price_per_mtok,
        cached_input_price_per_mtok: p.cached_input_price_per_mtok,
        output_price_per_mtok: p.output_price_per_mtok,
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
    /// 单价的币种（ISO 4217），不填按美元。
    #[serde(default)]
    pub price_currency: Option<String>,
    /// 每百万输入 token 的价（有缓存的厂商是未命中缓存的价）；输入与输出都不填不计算成本。
    #[serde(default)]
    pub input_price_per_mtok: Option<f64>,
    /// 每百万命中缓存的输入 token 的价；不填时命中的输入也按输入价算。
    #[serde(default)]
    pub cached_input_price_per_mtok: Option<f64>,
    #[serde(default)]
    pub output_price_per_mtok: Option<f64>,
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
    #[serde(default)]
    pub price_currency: Option<String>,
    #[serde(default)]
    pub input_price_per_mtok: Option<f64>,
    #[serde(default)]
    pub cached_input_price_per_mtok: Option<f64>,
    #[serde(default)]
    pub output_price_per_mtok: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct RotateReq {
    pub api_key: String,
}

/// 校验 provider 类型串合法（openai|anthropic|openai_compatible）。
fn validate_provider(provider: &str) -> Result<()> {
    Provider::parse(provider).map(|_| ())
}

/// 超时缺省时用默认值；给了就必须在合理范围内。
fn timeout_or_default(timeout_ms: Option<i64>) -> Result<i64> {
    let timeout_ms = timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);

    if !(MIN_TIMEOUT_MS..=MAX_TIMEOUT_MS).contains(&timeout_ms) {
        return Err(Error::invalid(format!(
            "timeout_ms must be between {MIN_TIMEOUT_MS} and {MAX_TIMEOUT_MS}"
        )));
    }

    Ok(timeout_ms)
}

/// 请求里的单价，校验过的：币种不填按美元。
#[derive(Debug, PartialEq)]
struct Pricing {
    currency: Currency,
    input: Option<f64>,
    cached_input: Option<f64>,
    output: Option<f64>,
}

fn pricing(
    currency: Option<&str>,
    input: Option<f64>,
    cached_input: Option<f64>,
    output: Option<f64>,
) -> Result<Pricing> {
    validate_price(input, "input_price_per_mtok")?;
    validate_price(cached_input, "cached_input_price_per_mtok")?;
    validate_price(output, "output_price_per_mtok")?;

    Ok(Pricing {
        currency: currency
            .map(Currency::parse)
            .transpose()?
            .unwrap_or(Currency::USD),
        input,
        cached_input,
        output,
    })
}

/// 单价的币种得是组织备下的一种。
async fn known_currency(state: &AppState, ctx: &IamContext, currency: Currency) -> Result<()> {
    let currencies = state.agent.currencies.list(&ctx.org_id).await?;

    if currencies.iter().any(|known| known.code == currency) {
        return Ok(());
    }

    Err(Error::invalid(format!(
        "currency {currency} is not one of the organization's currencies; add it first"
    )))
}

#[permission("agent.use")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<ProviderResp>>> {
    let rows = state.agent.model_providers.list(&ctx.org_id).await?;
    Ok(Json(rows.into_iter().map(to_resp).collect()))
}

#[permission("agent.manage")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<CreateReq>,
) -> Result<Json<ProviderResp>> {
    validate_provider(&req.provider)?;

    if req.name.trim().is_empty() || req.default_model.trim().is_empty() {
        return Err(Error::invalid("name and default_model are required"));
    }

    let prices = pricing(
        req.price_currency.as_deref(),
        req.input_price_per_mtok,
        req.cached_input_price_per_mtok,
        req.output_price_per_mtok,
    )?;
    known_currency(&state, &ctx, prices.currency).await?;
    let id = Id::new();

    let input = ModelProviderInput {
        id: id.clone(),
        org_id: ctx.org_id.clone(),
        provider: req.provider.clone(),
        name: req.name.clone(),
        base_url: req.base_url.clone(),
        default_model: req.default_model.clone(),
        enabled: req.enabled,
        timeout_ms: timeout_or_default(req.timeout_ms)?,
        max_tokens: req.max_tokens,
        price_currency: prices.currency,
        input_price_per_mtok: prices.input,
        cached_input_price_per_mtok: prices.cached_input,
        output_price_per_mtok: prices.output,
    };

    let api_key = req.api_key.as_deref().filter(|k| !k.is_empty());
    let saved = state.agent.model_providers.create(input, api_key).await?;

    // 审计：含 masked 元数据，不含明文/密文。
    activity_audit::record(
        &state,
        &ctx,
        "agent.model_provider.created",
        "agent_model_provider",
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

#[permission("agent.manage")]
async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<UpdateReq>,
) -> Result<Json<ProviderResp>> {
    validate_provider(&req.provider)?;
    let prices = pricing(
        req.price_currency.as_deref(),
        req.input_price_per_mtok,
        req.cached_input_price_per_mtok,
        req.output_price_per_mtok,
    )?;
    known_currency(&state, &ctx, prices.currency).await?;

    let input = ModelProviderInput {
        id: Id(id.clone()),
        org_id: ctx.org_id.clone(),
        provider: req.provider.clone(),
        name: req.name.clone(),
        base_url: req.base_url.clone(),
        default_model: req.default_model.clone(),
        enabled: req.enabled,
        timeout_ms: timeout_or_default(req.timeout_ms)?,
        max_tokens: req.max_tokens,
        price_currency: prices.currency,
        input_price_per_mtok: prices.input,
        cached_input_price_per_mtok: prices.cached_input,
        output_price_per_mtok: prices.output,
    };

    let saved = state.agent.model_providers.update(input).await?;

    activity_audit::record(
        &state,
        &ctx,
        "agent.model_provider.updated",
        "agent_model_provider",
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

#[permission("agent.manage")]
async fn rotate_key(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<RotateReq>,
) -> Result<Json<ProviderResp>> {
    if req.api_key.trim().is_empty() {
        return Err(Error::invalid("api_key is required"));
    }

    let saved = state
        .agent
        .model_providers
        .rotate_key(&ctx.org_id, &Id(id.clone()), &req.api_key)
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "agent.model_provider.key_rotated",
        "agent_model_provider",
        &saved.id.0,
        json!({ "key_last4": saved.key_last4 }),
    )
    .await;

    Ok(Json(to_resp(saved)))
}

#[permission("agent.manage")]
async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    state
        .agent
        .model_providers
        .delete(&ctx.org_id, &Id(id.clone()))
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "agent.model_provider.deleted",
        "agent_model_provider",
        &id,
        json!({}),
    )
    .await;

    Ok(Json(json!({ "deleted": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_defaults_to_two_minutes_and_must_be_reasonable() {
        assert_eq!(timeout_or_default(None).unwrap(), 120_000);
        assert_eq!(timeout_or_default(Some(45_000)).unwrap(), 45_000);
        assert!(timeout_or_default(Some(500)).is_err());
        assert!(timeout_or_default(Some(3_600_000)).is_err());
    }

    #[test]
    fn prices_are_optional_but_must_be_plausible() {
        assert_eq!(
            pricing(None, None, None, None).unwrap().currency,
            Currency::USD
        );
        assert!(pricing(None, Some(2.5), None, Some(10.0)).is_ok());
        assert!(pricing(None, Some(-1.0), None, None).is_err());
        assert!(pricing(None, None, Some(f64::NAN), None).is_err());
        assert!(pricing(None, None, None, Some(f64::INFINITY)).is_err());
    }

    #[test]
    fn a_currency_is_a_three_letter_code() {
        let cny = pricing(Some("CNY"), Some(2.0), Some(0.5), Some(8.0)).unwrap();
        assert_eq!(cny.currency.as_str(), "CNY");
        assert_eq!(cny.cached_input, Some(0.5));
        assert!(pricing(Some("rmb"), None, None, None).is_err());
    }
}
