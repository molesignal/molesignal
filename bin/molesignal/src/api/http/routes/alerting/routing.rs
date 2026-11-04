// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 路由接口：新事件交给哪个团队、走哪条升级策略。
//!
//! - `GET    /api/v1/alerts/routing/rules`             路由规则，按顺序（`alerts.routing.read`）
//! - `POST   /api/v1/alerts/routing/rules`             新建，排在最后（`alerts.routing.manage`）
//! - `PUT    /api/v1/alerts/routing/rules/order`       按给出的 id 重排（`alerts.routing.manage`）
//! - `PUT    /api/v1/alerts/routing/rules/{id}`        修改（`alerts.routing.manage`）
//! - `DELETE /api/v1/alerts/routing/rules/{id}`        删除（`alerts.routing.manage`）
//! - `GET    /api/v1/alerts/routing/teams`             团队、它们的值班策略和负责的服务（`alerts.routing.read`）
//! - `PUT    /api/v1/alerts/routing/teams/{team_id}`   设置或去掉一个团队的值班策略（`alerts.routing.manage`）
//! - `POST   /api/v1/alerts/routing/test`              一组标签会怎么分派（`alerts.routing.read`）

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{get, post, put},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    api::{AppState, http::routes::activity_audit},
    app::{
        alerting::routing::{RoutingProbe, RuleWrite},
        iam::IamContext,
    },
    domain::{alerting::routing::RoutingRule, iam::permission},
    shared::{Result, ids::Id},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/alerts/routing/rules", get(list_rules).post(create_rule))
        .route("/alerts/routing/rules/order", put(reorder_rules))
        .route(
            "/alerts/routing/rules/{id}",
            put(update_rule).delete(delete_rule),
        )
        .route("/alerts/routing/teams", get(list_teams))
        .route("/alerts/routing/teams/{teamId}", put(set_team_policy))
        .route("/alerts/routing/test", post(test_routing))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OrderReq {
    ids: Vec<Id>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TeamPolicyReq {
    /// `null` 去掉这个团队的值班策略。
    escalation_policy_id: Option<Id>,
}

async fn audit_rule(state: &AppState, ctx: &IamContext, action: &str, rule: &RoutingRule) {
    activity_audit::record(
        state,
        ctx,
        action,
        "alert_routing_rule",
        &rule.id.0,
        json!({ "name": rule.name, "target": rule.target }),
    )
    .await;
}

#[permission("alerts.routing.read")]
async fn list_rules(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<RoutingRule>>> {
    Ok(Json(state.alerting.routing.rules(&ctx.org_id).await?))
}

#[permission("alerts.routing.manage")]
async fn create_rule(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(write): Json<RuleWrite>,
) -> Result<Json<RoutingRule>> {
    let rule = state
        .alerting
        .routing
        .create_rule(&ctx.org_id, &ctx.user_id, write)
        .await?;

    audit_rule(&state, &ctx, "alerts.routing.rule_created", &rule).await;
    Ok(Json(rule))
}

#[permission("alerts.routing.manage")]
async fn update_rule(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(write): Json<RuleWrite>,
) -> Result<Json<RoutingRule>> {
    let rule = state
        .alerting
        .routing
        .update_rule(&ctx.org_id, &Id(id), write)
        .await?;

    audit_rule(&state, &ctx, "alerts.routing.rule_updated", &rule).await;
    Ok(Json(rule))
}

#[permission("alerts.routing.manage")]
async fn delete_rule(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let rule = state
        .alerting
        .routing
        .delete_rule(&ctx.org_id, &Id(id))
        .await?;

    audit_rule(&state, &ctx, "alerts.routing.rule_deleted", &rule).await;
    Ok(Json(json!({ "deleted": rule.id })))
}

#[permission("alerts.routing.manage")]
async fn reorder_rules(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<OrderReq>,
) -> Result<Json<Vec<RoutingRule>>> {
    let rules = state
        .alerting
        .routing
        .reorder(&ctx.org_id, &req.ids)
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "alerts.routing.rules_reordered",
        "alert_routing_rule",
        "",
        json!({ "order": rules.iter().map(|rule| &rule.id).collect::<Vec<_>>() }),
    )
    .await;

    Ok(Json(rules))
}

#[permission("alerts.routing.read")]
async fn list_teams(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    let teams = state.alerting.routing.teams(&ctx.org_id).await?;
    Ok(Json(serde_json::to_value(teams).unwrap_or(Value::Null)))
}

#[permission("alerts.routing.manage")]
async fn set_team_policy(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(team_id): Path<String>,
    Json(req): Json<TeamPolicyReq>,
) -> Result<Json<Value>> {
    let team_id = Id(team_id);
    let policy = state
        .alerting
        .routing
        .set_team_policy(
            &ctx.org_id,
            &ctx.user_id,
            &team_id,
            req.escalation_policy_id,
        )
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "alerts.routing.team_policy_set",
        "team",
        &team_id.0,
        json!({ "escalationPolicyId": policy.as_ref().map(|policy| &policy.escalation_policy_id) }),
    )
    .await;

    Ok(Json(json!({
        "teamId": team_id,
        "escalationPolicyId": policy.map(|policy| policy.escalation_policy_id),
    })))
}

#[permission("alerts.routing.read")]
async fn test_routing(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(probe): Json<RoutingProbe>,
) -> Result<Json<Value>> {
    let explanation = state.alerting.routing.explain(&ctx.org_id, probe).await?;
    Ok(Json(
        serde_json::to_value(explanation).unwrap_or(Value::Null),
    ))
}
