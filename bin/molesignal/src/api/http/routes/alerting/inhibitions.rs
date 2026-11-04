// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 告警抑制：有更严重（或更上游）的事件在时，压住次要事件的通知。
//!
//! - `GET    /api/v1/alerts/inhibit-rules`              列表，带 `inhibiting`（此刻压着几个事件）
//! - `POST   /api/v1/alerts/inhibit-rules`              新建
//! - `POST   /api/v1/alerts/inhibit-rules/preview`      这条规则（不用先存）现在会压住哪些事件
//! - `GET    /api/v1/alerts/inhibit-rules/{id}`         详情
//! - `PUT    /api/v1/alerts/inhibit-rules/{id}`         修改
//! - `DELETE /api/v1/alerts/inhibit-rules/{id}`         删除
//! - `POST   /api/v1/alerts/incidents/{id}/inhibition/lift`  照常通知：这个事件不再被抑制
//!
//! 看规则要 `alerts.inhibitions.read`，改要 `alerts.inhibitions.manage`；放开一个事件要对它有
//! `alerts.incidents.update`。规则随联邦同步，各集群按自己的事件判定。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    api::{
        AppState,
        http::{
            federation::{delete_payload, emit_cud},
            routes::activity_audit,
        },
    },
    app::{alerting::Actor, iam::IamContext},
    domain::{
        alerting::{
            incident::Incident,
            inhibit::{
                DEFAULT_LINGER_SECS, DEFAULT_WAIT_SECS, InhibitRule, InhibitSource, InhibitTarget,
            },
            timeline::ActionVia,
        },
        federation::{CudAction, ResourceKind},
        iam::{permission, resource_permission},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/alerts/inhibit-rules", get(list).post(create))
        .route("/alerts/inhibit-rules/preview", post(preview))
        .route(
            "/alerts/inhibit-rules/{id}",
            get(get_one).put(update).delete(remove),
        )
        .route("/alerts/incidents/{id}/inhibition/lift", post(lift))
}

fn yes() -> bool {
    true
}

fn wait_secs() -> u32 {
    DEFAULT_WAIT_SECS
}

fn linger_secs() -> u32 {
    DEFAULT_LINGER_SECS
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RuleReq {
    #[serde(default)]
    name: String,
    #[serde(default = "yes")]
    enabled: bool,
    #[serde(default)]
    source: InhibitSource,
    #[serde(default)]
    target: InhibitTarget,
    #[serde(default)]
    equal: Vec<String>,
    #[serde(default = "yes")]
    more_severe_only: bool,
    #[serde(default = "wait_secs")]
    wait_secs: u32,
    #[serde(default = "linger_secs")]
    linger_secs: u32,
    #[serde(default)]
    comment: String,
}

impl RuleReq {
    fn into_rule(
        self,
        id: Id,
        org_id: Id,
        created_by: Option<Id>,
        created_at: TimestampMicros,
    ) -> Result<InhibitRule> {
        let mut rule = InhibitRule {
            id,
            org_id,
            name: self.name,
            enabled: self.enabled,
            source: self.source,
            target: self.target,
            equal: self.equal,
            more_severe_only: self.more_severe_only,
            wait_secs: self.wait_secs,
            linger_secs: self.linger_secs,
            comment: self.comment.trim().to_string(),
            created_by,
            created_at,
            updated_at: TimestampMicros::now(),
        };

        rule.validate()?;
        Ok(rule)
    }
}

/// 规则本身，加上此刻压着几个事件。
fn rule_view(rule: &InhibitRule, inhibiting: usize) -> Value {
    let mut body = serde_json::to_value(rule).unwrap_or(Value::Null);

    if let Some(fields) = body.as_object_mut() {
        fields.insert("inhibiting".into(), json!(inhibiting));
    }

    body
}

async fn audit(state: &AppState, ctx: &IamContext, action: &str, rule: &InhibitRule) {
    activity_audit::record(
        state,
        ctx,
        action,
        "inhibit_rule",
        &rule.id.0,
        json!({
            "name": rule.name,
            "enabled": rule.enabled,
            "equal": rule.equal,
            "moreSevereOnly": rule.more_severe_only,
        }),
    )
    .await;
}

/// 本组织的一条规则；别的组织的当作不存在。
async fn owned_rule(state: &AppState, ctx: &IamContext, id: &str) -> Result<InhibitRule> {
    let rule = state
        .alerting
        .inhibitions
        .rules()
        .get(&Id::from_string(id))
        .await?;

    if rule.org_id != ctx.org_id {
        return Err(Error::not_found("inhibit rule not found"));
    }

    Ok(rule)
}

#[permission("alerts.inhibitions.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    let inhibitions = &state.alerting.inhibitions;
    let rules = inhibitions.rules().list(&ctx.org_id).await?;
    let counts = inhibitions.counts(&ctx.org_id).await?;

    let rows = rules
        .iter()
        .map(|rule| rule_view(rule, counts.get(&rule.id).copied().unwrap_or(0)))
        .collect();

    Ok(Json(Value::Array(rows)))
}

#[permission("alerts.inhibitions.manage")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<RuleReq>,
) -> Result<Json<Value>> {
    let rule = req.into_rule(
        Id::new(),
        ctx.org_id.clone(),
        Some(ctx.user_id.clone()),
        TimestampMicros::now(),
    )?;

    let saved = state.alerting.inhibitions.rules().create(rule).await?;

    emit_cud(
        &state,
        &saved.org_id,
        ResourceKind::InhibitRule,
        CudAction::Created,
        &saved.id.0,
        &saved,
    )
    .await;

    audit(&state, &ctx, "alerts.inhibit_rule.created", &saved).await;
    Ok(Json(rule_view(&saved, 0)))
}

/// 还没存的规则也能试：名字不要求。
#[permission("alerts.inhibitions.read")]
async fn preview(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(mut req): Json<RuleReq>,
) -> Result<Json<Value>> {
    if req.name.trim().is_empty() {
        req.name = "preview".into();
    }

    let now = TimestampMicros::now();
    let rule = req.into_rule(Id::new(), ctx.org_id.clone(), None, now)?;
    let pairs = state.alerting.inhibitions.preview(&rule, now).await?;

    Ok(Json(json!({ "pairs": pairs })))
}

#[permission("alerts.inhibitions.read")]
async fn get_one(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let rule = owned_rule(&state, &ctx, &id).await?;
    let counts = state.alerting.inhibitions.counts(&ctx.org_id).await?;

    Ok(Json(rule_view(
        &rule,
        counts.get(&rule.id).copied().unwrap_or(0),
    )))
}

#[permission("alerts.inhibitions.manage")]
async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<RuleReq>,
) -> Result<Json<Value>> {
    let existing = owned_rule(&state, &ctx, &id).await?;

    let rule = req.into_rule(
        existing.id,
        existing.org_id,
        existing.created_by,
        existing.created_at,
    )?;

    let saved = state.alerting.inhibitions.rules().update(rule).await?;

    emit_cud(
        &state,
        &saved.org_id,
        ResourceKind::InhibitRule,
        CudAction::Updated,
        &saved.id.0,
        &saved,
    )
    .await;

    audit(&state, &ctx, "alerts.inhibit_rule.updated", &saved).await;

    let counts = state.alerting.inhibitions.counts(&ctx.org_id).await?;

    Ok(Json(rule_view(
        &saved,
        counts.get(&saved.id).copied().unwrap_or(0),
    )))
}

#[permission("alerts.inhibitions.manage")]
async fn remove(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let existing = owned_rule(&state, &ctx, &id).await?;

    state
        .alerting
        .inhibitions
        .rules()
        .delete(&existing.id)
        .await?;

    emit_cud(
        &state,
        &existing.org_id,
        ResourceKind::InhibitRule,
        CudAction::Deleted,
        &existing.id.0,
        &delete_payload(&existing.id.0),
    )
    .await;

    audit(&state, &ctx, "alerts.inhibit_rule.deleted", &existing).await;
    Ok(Json(json!({ "deleted": true })))
}

#[resource_permission(
    action = "alerts.incidents.update",
    resource = Incident,
    id = Id::from_string(id),
    bind = incident
)]
async fn lift(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let actor = Actor::new(&ctx.org_id, ctx.principal_id(), ActionVia::Web);

    let lifted = state
        .alerting
        .actions
        .lift_inhibition(&actor, &incident.id)
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "incident.inhibition_lifted",
        "incident",
        &lifted.id.0,
        json!({ "summary": lifted.summary }),
    )
    .await;

    Ok(Json(serde_json::to_value(&lifted).unwrap_or(Value::Null)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_takes_the_default_buffers_and_refuses_what_it_does_not_know() {
        let req: RuleReq = serde_json::from_value(json!({
            "name": "same service", "equal": ["service"]
        }))
        .expect("defaults");

        assert!(req.enabled);
        assert!(req.more_severe_only);
        assert_eq!(req.wait_secs, DEFAULT_WAIT_SECS);
        assert_eq!(req.linger_secs, DEFAULT_LINGER_SECS);

        assert!(serde_json::from_value::<RuleReq>(json!({ "name": "x", "matchers": [] })).is_err());
    }

    #[test]
    fn a_rule_without_a_source_or_shared_labels_is_refused() {
        let req: RuleReq = serde_json::from_value(json!({ "name": "everything" })).expect("parse");

        assert!(
            req.into_rule(
                Id::from_string("r"),
                Id::from_string("o"),
                None,
                TimestampMicros(1),
            )
            .is_err()
        );
    }

    #[test]
    fn the_view_says_how_many_it_holds_back() {
        let rule = InhibitRule {
            id: Id::from_string("r"),
            org_id: Id::from_string("o"),
            name: "n".into(),
            enabled: true,
            source: InhibitSource::default(),
            target: InhibitTarget::default(),
            equal: vec!["service".into()],
            more_severe_only: true,
            wait_secs: 0,
            linger_secs: 0,
            comment: String::new(),
            created_by: None,
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
        };

        assert_eq!(rule_view(&rule, 3)["inhibiting"], 3);
    }
}
