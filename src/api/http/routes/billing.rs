// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Billing 控制面：平台级 Stripe 配置管理 + webhook 入口。
//!
//! - `GET    /billing/settings`       读取（masked，不回吐密钥）—— org.billing.read
//! - `PUT    /billing/settings`       更新 enabled / 容差 / 密钥 —— org.billing.manage
//! - `POST   /billing/stripe/webhook` Stripe webhook（**无 JWT**，HMAC 验签）
//!
//! 配置为平台级单例（单 Stripe 账户）；当前由组织级 IAM 管理能力守门。
//! 守护。webhook 路径在 `auth_layer` 白名单放行，靠 `Stripe-Signature` 验签鉴别。

use std::sync::atomic::Ordering;

use axum::{
    Extension, Json, Router,
    body::Bytes,
    extract::State,
    http::HeaderMap,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    api::AppState,
    app::iam::IamContext,
    cloud_marketplace::{
        MARKETPLACE_FEATURE,
        stripe::{
            STRIPE_PROVIDER, parse_subscription_event, stripe_status_to_state,
            verify_stripe_signature,
        },
    },
    domain::{
        billing::{
            DEFAULT_TRIAL_DAYS, NotifyStage, Trial, TrialState, evaluate_trial_state,
            trial_days_remaining, trial_ends_after,
        },
        iam::permission,
    },
    infra::persistence::repositories::{
        billing_settings::BillingSettingsInput, marketplace::MarketplaceSubscription,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/billing/settings", get(get_settings).put(put_settings))
        .route("/billing/trial", get(get_trial))
        .route("/billing/stripe/webhook", post(stripe_webhook))
}

#[derive(Debug, Serialize)]
struct TrialResp {
    /// none（非 SaaS / 无试用）| active | expired | converted。
    state: String,
    started_at_micros: i64,
    ends_at_micros: i64,
    days_remaining: i64,
}

/// 当前 org 的试用状态。计费启用且既无试用记录又无有效订阅时，惰性开启一段默认试用。
async fn get_trial(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<TrialResp>> {
    let now = TimestampMicros::now();
    let billing_on = state.platform.billing_enabled.load(Ordering::Relaxed)
        || state.platform.license.has_feature(MARKETPLACE_FEATURE);
    let subs = state.platform.marketplace.list(&ctx.org_id).await?;
    let has_active_sub = subs.iter().any(|s| s.state == "active");

    let mut trial = state.platform.trials.get(&ctx.org_id).await?;
    if trial.is_none() && billing_on && !has_active_sub {
        let created = Trial {
            org_id: ctx.org_id.clone(),
            started_at: now,
            ends_at: trial_ends_after(now, DEFAULT_TRIAL_DAYS),
            state: TrialState::Active,
            notified_stage: NotifyStage::None,
            created_at: now,
            updated_at: now,
        };
        trial = Some(state.platform.trials.create_if_absent(created).await?);
    }

    let resp = match trial {
        Some(t) => {
            let st = evaluate_trial_state(t.ends_at, now, has_active_sub);
            TrialResp {
                state: st.as_str().to_string(),
                started_at_micros: t.started_at.0,
                ends_at_micros: t.ends_at.0,
                days_remaining: trial_days_remaining(t.ends_at, now),
            }
        }
        None => TrialResp {
            state: "none".to_string(),
            started_at_micros: 0,
            ends_at_micros: 0,
            days_remaining: 0,
        },
    };
    Ok(Json(resp))
}

#[derive(Debug, Serialize)]
struct SettingsResp {
    enabled: bool,
    signature_tolerance_secs: i64,
    /// 仅暴露"是否已设置"，绝不回吐密钥明文。
    webhook_secret_set: bool,
    api_key_set: bool,
    updated_at_micros: i64,
}

#[derive(Debug, Deserialize)]
struct SettingsReq {
    enabled: bool,
    #[serde(default = "default_tolerance")]
    signature_tolerance_secs: i64,
    /// `null`/缺省 = 保持原值；`""` = 清空；非空 = 替换。
    #[serde(default)]
    webhook_secret: Option<String>,
    #[serde(default)]
    api_key: Option<String>,
}

fn default_tolerance() -> i64 {
    300
}

#[permission("org.billing.read")]
async fn get_settings(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<SettingsResp>> {
    let cfg = state.platform.billing_settings.get().await?;
    Ok(Json(SettingsResp {
        enabled: cfg.enabled,
        signature_tolerance_secs: cfg.signature_tolerance_secs,
        webhook_secret_set: cfg.webhook_secret_set,
        api_key_set: cfg.api_key_set,
        updated_at_micros: cfg.updated_at.0,
    }))
}

#[permission("org.billing.manage")]
async fn put_settings(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<SettingsReq>,
) -> Result<Json<SettingsResp>> {
    if req.signature_tolerance_secs <= 0 {
        return Err(Error::invalid("signature_tolerance_secs must be positive"));
    }
    let saved = state
        .platform
        .billing_settings
        .set(BillingSettingsInput {
            enabled: req.enabled,
            signature_tolerance_secs: req.signature_tolerance_secs,
            webhook_secret: req.webhook_secret,
            api_key: req.api_key,
        })
        .await?;
    // 刷新热路径缓存：ingest gate 读这个 atomic 决定是否查订阅态。
    state
        .platform
        .billing_enabled
        .store(saved.enabled, Ordering::Relaxed);
    Ok(Json(SettingsResp {
        enabled: saved.enabled,
        signature_tolerance_secs: saved.signature_tolerance_secs,
        webhook_secret_set: saved.webhook_secret_set,
        api_key_set: saved.api_key_set,
        updated_at_micros: saved.updated_at.0,
    }))
}

/// Stripe webhook：验签 → 解析订阅事件 → upsert 到 marketplace_subscriptions(provider=stripe)。
/// 无 JWT（auth_layer 白名单）；以 `Stripe-Signature` HMAC 鉴别。
async fn stripe_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>> {
    let cfg = state.platform.billing_settings.get().await?;
    if !cfg.enabled {
        // 未启用：不暴露内部状态，按未找到处理。
        return Err(Error::not_found("billing webhook not enabled"));
    }
    let sig = headers
        .get("Stripe-Signature")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::unauthorized("missing Stripe-Signature"))?;
    let now_unix = chrono::Utc::now().timestamp();
    verify_stripe_signature(
        &body,
        sig,
        &cfg.webhook_secret,
        now_unix,
        cfg.signature_tolerance_secs,
    )?;

    // 非订阅事件：验签通过即应答 200，不动状态。
    let Some(ev) = parse_subscription_event(&body)? else {
        return Ok(Json(json!({ "received": true, "ignored": true })));
    };

    let now = TimestampMicros::now();
    let existing = state
        .platform
        .marketplace
        .find_by_external(STRIPE_PROVIDER, &ev.subscription_id)
        .await?;
    // org 绑定：优先 metadata.org_id，其次沿用已有记录；都没有则拒绝（无法归属）。
    let org_id = match (ev.org_id.as_deref(), existing.as_ref()) {
        (Some(o), _) => Id(o.to_string()),
        (None, Some(s)) => s.org_id.clone(),
        (None, None) => {
            return Err(Error::invalid(
                "stripe subscription missing metadata.org_id (cannot map to organization)",
            ));
        }
    };
    let new_state = stripe_status_to_state(&ev.status);
    let id = existing
        .as_ref()
        .map(|s| s.id.clone())
        .unwrap_or_else(Id::new);
    let created_at = existing.as_ref().map(|s| s.created_at).unwrap_or(now);
    let row = MarketplaceSubscription {
        id,
        org_id,
        provider: STRIPE_PROVIDER.to_string(),
        external_id: ev.subscription_id,
        state: new_state.as_str().to_string(),
        plan_id: ev.price_id,
        metadata: json!({ "customer_id": ev.customer_id, "event_type": ev.event_type }),
        created_at,
        updated_at: now,
    };
    let saved = state.platform.marketplace.upsert_by_external(row).await?;
    // 订阅状态变了：撤销该 org 的门禁缓存，让 ingest gate 立刻看到新状态（不等 TTL）。
    state
        .platform
        .billing_state_cache
        .invalidate(&saved.org_id)
        .await;
    Ok(Json(json!({ "received": true, "state": saved.state })))
}
