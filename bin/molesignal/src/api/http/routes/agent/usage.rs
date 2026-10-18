// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 用量与组织预算。
//!
//! - `GET /api/v1/agent/usage?days=30` 今天的预算状况、预算设置与最近几天按来源的用量
//! - `PUT /api/v1/agent/usage/budget`  保存组织的每日上限与预算用完时对话的处理（仅管理员）

use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    routing::{get, put},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    agent::usage::{DailyUsage, InteractiveBudgetMode},
    api::{AppState, http::routes::activity_audit},
    app::iam::IamContext,
    domain::iam::permission,
    shared::Result,
};

/// 不指定时回看的天数。
const DEFAULT_DAYS: i64 = 30;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/agent/usage", get(get_usage))
        .route("/agent/usage/budget", put(put_budget))
}

#[derive(Debug, Deserialize)]
struct UsageQuery {
    #[serde(default)]
    days: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct BudgetRequest {
    /// 每日 token 上限；`null` 跟随实例默认值，0 表示不限。
    daily_token_limit: Option<i64>,
    #[serde(default)]
    interactive_mode: InteractiveBudgetMode,
}

fn day_row(row: &DailyUsage) -> Value {
    json!({
        "day_start_micros": row.day.start().0,
        "origin": row.origin,
        "calls": row.calls,
        "unpriced_calls": row.unpriced_calls,
        "prompt_tokens": row.prompt_tokens,
        "completion_tokens": row.completion_tokens,
        "cost": row.cost,
        "currency": row.currency,
    })
}

async fn usage_view(state: &AppState, ctx: &IamContext, days: i64) -> Result<Value> {
    let status = state.agent.usage.status(&ctx.org_id).await;
    let budget = state.agent.usage.budget_settings(&ctx.org_id).await?;
    let history = state.agent.usage.history(&ctx.org_id, days).await?;

    // 有模型服务填了单价时才谈得上成本；否则界面不显示成本。
    let cost_known = state
        .agent
        .model_providers
        .list(&ctx.org_id)
        .await?
        .iter()
        .any(|provider| provider.prices().is_some());

    Ok(json!({
        "status": status,
        "budget": budget,
        "cost_known": cost_known,
        "days": history.iter().map(day_row).collect::<Vec<_>>(),
    }))
}

#[permission("agent.use")]
async fn get_usage(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<UsageQuery>,
) -> Result<Json<Value>> {
    let days = query.days.unwrap_or(DEFAULT_DAYS);

    Ok(Json(usage_view(&state, &ctx, days).await?))
}

#[permission("agent.manage")]
async fn put_budget(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<BudgetRequest>,
) -> Result<Json<Value>> {
    let saved = state
        .agent
        .usage
        .put_budget(
            &ctx.org_id,
            request.daily_token_limit,
            request.interactive_mode,
            ctx.principal_id(),
        )
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "agent.usage_budget.updated",
        "agent_usage_budget",
        &ctx.org_id.0,
        json!({
            "daily_token_limit": saved.org_limit,
            "interactive_mode": saved.interactive_mode,
        }),
    )
    .await;

    Ok(Json(usage_view(&state, &ctx, DEFAULT_DAYS).await?))
}
