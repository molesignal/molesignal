// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Cloud marketplace webhook 接收（付费版独占）。
//!
//! - `POST /api/v1/marketplace/aws-webhook` ：解析 AWS SNS notification → 状态机
//! - `POST /api/v1/marketplace/azure-webhook` ：解析 Azure webhook → 状态机
//! - `GET /api/v1/marketplace/subscriptions` ：列出本 org 的订阅
//!
//! `cfg=` + `license.has_feature("marketplace")`；OSS 完全不编译。

use axum::{
    Extension, Json, Router,
    extract::State,
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    api::AppState,
    app::iam::IamContext,
    cloud_marketplace::{MARKETPLACE_FEATURE, aws_action_to_state, azure_action_to_state},
    domain::iam::permission,
    infra::persistence::repositories::marketplace::MarketplaceSubscription,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/marketplace/aws-webhook", post(aws_webhook))
        .route("/marketplace/azure-webhook", post(azure_webhook))
        .route("/marketplace/subscriptions", get(list))
}

fn require_license(state: &AppState) -> Result<()> {
    if !state.platform.license.has_feature(MARKETPLACE_FEATURE) {
        return Err(Error::forbidden(format!(
            "{MARKETPLACE_FEATURE} feature not licensed"
        )));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct AwsWebhookBody {
    /// AWS Marketplace 通知字段（简化）
    pub customer_identifier: String,
    pub action: String, // subscribe-success | unsubscribe-pending | …
    #[serde(default)]
    pub product_code: Option<String>,
    #[serde(default)]
    pub org_id: Option<String>,
}

async fn aws_webhook(
    State(state): State<AppState>,
    Json(body): Json<AwsWebhookBody>,
) -> Result<Json<Value>> {
    require_license(&state)?;
    let new_state = aws_action_to_state(&body.action)
        .map_err(|e| Error::invalid(format!("AWS marketplace action: {e}")))?;
    upsert_subscription(
        &state,
        "aws",
        &body.customer_identifier,
        body.org_id.as_deref(),
        body.product_code.as_deref(),
        new_state.as_str(),
    )
    .await
}

#[derive(Debug, Deserialize)]
pub struct AzureWebhookBody {
    pub subscription_id: String,
    pub action: String,
    #[serde(default)]
    pub plan_id: Option<String>,
    #[serde(default)]
    pub org_id: Option<String>,
}

async fn azure_webhook(
    State(state): State<AppState>,
    Json(body): Json<AzureWebhookBody>,
) -> Result<Json<Value>> {
    require_license(&state)?;
    let new_state = azure_action_to_state(&body.action)
        .map_err(|e| Error::invalid(format!("Azure marketplace action: {e}")))?;
    upsert_subscription(
        &state,
        "azure",
        &body.subscription_id,
        body.org_id.as_deref(),
        body.plan_id.as_deref(),
        new_state.as_str(),
    )
    .await
}

async fn upsert_subscription(
    state: &AppState,
    provider: &str,
    external_id: &str,
    org_id_override: Option<&str>,
    plan_id: Option<&str>,
    new_state: &str,
) -> Result<Json<Value>> {
    let now = TimestampMicros::now();
    let existing = state
        .platform
        .marketplace
        .find_by_external(provider, external_id)
        .await?;
    let org_id = match (org_id_override, existing.as_ref()) {
        (Some(o), _) => Id(o.to_string()),
        (None, Some(s)) => s.org_id.clone(),
        // 没指定 org → 用首个 org（marketplace webhook 通常需配套 org 绑定流程，留 follow-up）
        (None, None) => {
            return Err(Error::invalid(
                "org_id required for new marketplace subscription",
            ));
        }
    };
    let id = existing
        .as_ref()
        .map(|s| s.id.clone())
        .unwrap_or_else(Id::new);
    let created_at = existing.as_ref().map(|s| s.created_at).unwrap_or(now);
    let row = MarketplaceSubscription {
        id,
        org_id,
        provider: provider.to_string(),
        external_id: external_id.to_string(),
        state: new_state.to_string(),
        plan_id: plan_id.map(String::from),
        metadata: Value::Null,
        created_at,
        updated_at: now,
    };
    let saved = state.platform.marketplace.upsert_by_external(row).await?;
    // 订阅状态变更后撤销门禁缓存（热路径），让 ingest gate 立即生效。
    state
        .platform
        .billing_state_cache
        .invalidate(&saved.org_id)
        .await;
    Ok(Json(serde_json::json!({
        "id": saved.id.0,
        "state": saved.state,
    })))
}

#[permission("org.billing.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    require_license(&state)?;
    let subs = state.platform.marketplace.list(&ctx.org_id).await?;
    Ok(Json(serde_json::json!({"subscriptions": subs})))
}
