// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 模型服务单价可选的币种。
//!
//! `GET    /api/v1/agent/settings/currencies`         组织的币种，各被几个模型服务用着
//! `PUT    /api/v1/agent/settings/currencies/{code}`  添加一种，或改它的名称与符号
//! `DELETE /api/v1/agent/settings/currencies/{code}`  删掉一种；还有模型服务用着时不能删

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{get, put},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    agent::{currencies::PriceCurrency, usage::Currency},
    api::{AppState, http::routes::activity_audit},
    app::iam::IamContext,
    domain::iam::permission,
    infra::persistence::repositories::agent::model_providers::ModelProvider,
    shared::{Error, Result},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/agent/settings/currencies", get(list))
        .route(
            "/agent/settings/currencies/{code}",
            put(save).delete(remove),
        )
}

#[derive(Debug, Serialize)]
struct CurrencyResp {
    code: Currency,
    name: Option<String>,
    symbol: Option<String>,
    /// 用这种币种计价的模型服务。
    used_by: Vec<String>,
    updated_at_micros: i64,
}

#[derive(Debug, Deserialize)]
struct SaveReq {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    symbol: Option<String>,
}

/// 用这种币种计价的模型服务的名称。
fn users(providers: &[ModelProvider], code: Currency) -> Vec<String> {
    providers
        .iter()
        .filter(|provider| provider.price_currency == code)
        .map(|provider| provider.name.clone())
        .collect()
}

fn resp(currency: PriceCurrency, providers: &[ModelProvider]) -> CurrencyResp {
    CurrencyResp {
        used_by: users(providers, currency.code),
        code: currency.code,
        name: currency.name,
        symbol: currency.symbol,
        updated_at_micros: currency.updated_at.0,
    }
}

#[permission("agent.use")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    let currencies = state.agent.currencies.list(&ctx.org_id).await?;
    let providers = state.agent.model_providers.list(&ctx.org_id).await?;

    Ok(Json(json!({
        "currencies": currencies
            .into_iter()
            .map(|currency| resp(currency, &providers))
            .collect::<Vec<_>>(),
    })))
}

#[permission("agent.manage")]
async fn save(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(code): Path<String>,
    Json(req): Json<SaveReq>,
) -> Result<Json<CurrencyResp>> {
    let currency = PriceCurrency::new(
        Currency::parse(&code)?,
        req.name.as_deref(),
        req.symbol.as_deref(),
    )?;

    let saved = state.agent.currencies.put(&ctx.org_id, &currency).await?;
    let providers = state.agent.model_providers.list(&ctx.org_id).await?;

    activity_audit::record(
        &state,
        &ctx,
        "agent.currency.saved",
        "agent_currency",
        saved.code.as_str(),
        json!({ "name": saved.name, "symbol": saved.symbol }),
    )
    .await;

    Ok(Json(resp(saved, &providers)))
}

#[permission("agent.manage")]
async fn remove(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(code): Path<String>,
) -> Result<Json<Value>> {
    let code = Currency::parse(&code)?;
    let providers = state.agent.model_providers.list(&ctx.org_id).await?;
    let used_by = users(&providers, code);

    if !used_by.is_empty() {
        return Err(Error::conflict(format!(
            "currency {code} is used by model providers {}; price them in another currency first",
            used_by.join(", ")
        )));
    }

    let deleted = state.agent.currencies.delete(&ctx.org_id, code).await?;

    if deleted {
        activity_audit::record(
            &state,
            &ctx,
            "agent.currency.deleted",
            "agent_currency",
            code.as_str(),
            json!({}),
        )
        .await;
    }

    Ok(Json(json!({ "deleted": deleted })))
}
