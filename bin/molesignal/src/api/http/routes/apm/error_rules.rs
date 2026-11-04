// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Rules for grouping errors: listed by whoever reads APM, set by whoever
//! manages it. A saved rule applies on this node at once and on the others
//! within half a minute, to the errors seen from then on.

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, put},
};
use serde::Deserialize;

use crate::{
    api::{AppState, http::routes::activity_audit},
    app::iam::IamContext,
    domain::{
        apm::{ERROR_RULES_MAX, ErrorRule, ErrorRuleAction},
        iam::permission,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/apm/error-rules", get(list_rules).post(create_rule))
        .route("/apm/error-rules/order", put(order_rules))
        .route(
            "/apm/error-rules/{id}",
            put(update_rule).delete(delete_rule),
        )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuleWrite {
    name: String,
    #[serde(default)]
    service_namespace: String,
    #[serde(default)]
    service_name: String,
    #[serde(default)]
    error_type: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    frame: String,
    #[serde(default)]
    endpoint: String,
    action: ErrorRuleAction,
    #[serde(default = "enabled")]
    enabled: bool,
}

fn enabled() -> bool {
    true
}

impl RuleWrite {
    fn into_rule(self, id: Id, position: u32, ctx: &IamContext) -> Result<ErrorRule> {
        let text = |value: String| value.trim().to_owned();

        let rule = ErrorRule {
            id,
            name: text(self.name),
            service_namespace: text(self.service_namespace),
            service_name: text(self.service_name),
            error_type: text(self.error_type),
            message: text(self.message),
            frame: text(self.frame),
            endpoint: text(self.endpoint),
            action: self.action,
            enabled: self.enabled,
            position,
            updated_by: ctx.principal_id().clone(),
            updated_at: TimestampMicros::now(),
        };

        rule.validate()?;

        Ok(rule)
    }
}

#[derive(Debug, Deserialize)]
struct OrderWrite {
    ids: Vec<String>,
}

#[permission(any("apm.error_rules.read", "sys.telemetry.read"))]
async fn list_rules(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<ErrorRule>>> {
    Ok(Json(
        state
            .telemetry
            .apm_catalog
            .list_error_rules(&ctx.org_id)
            .await?,
    ))
}

#[permission("apm.error_rules.manage")]
async fn create_rule(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<RuleWrite>,
) -> Result<(StatusCode, Json<ErrorRule>)> {
    let catalog = &state.telemetry.apm_catalog;
    let rules = catalog.list_error_rules(&ctx.org_id).await?;

    if rules.len() >= ERROR_RULES_MAX {
        return Err(Error::invalid(format!(
            "an organization keeps at most {ERROR_RULES_MAX} error rules"
        )));
    }

    // A new rule is tried last.
    let position = rules
        .iter()
        .map(|rule| rule.position.saturating_add(1))
        .max()
        .unwrap_or_default();
    let rule = request.into_rule(Id::new(), position, &ctx)?;

    catalog.save_error_rule(&ctx.org_id, &rule).await?;
    reapply(&state, &ctx.org_id).await;
    audit(&state, &ctx, "apm.error_rule.create", &rule).await;

    Ok((StatusCode::CREATED, Json(rule)))
}

#[permission("apm.error_rules.manage")]
async fn update_rule(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<RuleWrite>,
) -> Result<Json<ErrorRule>> {
    let catalog = &state.telemetry.apm_catalog;
    let id = Id::from_string(id);
    let current = catalog
        .list_error_rules(&ctx.org_id)
        .await?
        .into_iter()
        .find(|rule| rule.id == id)
        .ok_or_else(|| Error::not_found("APM error rule"))?;
    let rule = request.into_rule(id, current.position, &ctx)?;

    catalog.save_error_rule(&ctx.org_id, &rule).await?;
    reapply(&state, &ctx.org_id).await;
    audit(&state, &ctx, "apm.error_rule.update", &rule).await;

    Ok(Json(rule))
}

#[permission("apm.error_rules.manage")]
async fn delete_rule(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let id = Id::from_string(id);

    if !state
        .telemetry
        .apm_catalog
        .delete_error_rule(&ctx.org_id, &id)
        .await?
    {
        return Err(Error::not_found("APM error rule"));
    }

    reapply(&state, &ctx.org_id).await;

    activity_audit::record(
        &state,
        &ctx,
        "apm.error_rule.delete",
        "apm_error_rule",
        id.as_str(),
        serde_json::json!({}),
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}

#[permission("apm.error_rules.manage")]
async fn order_rules(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<OrderWrite>,
) -> Result<Json<Vec<ErrorRule>>> {
    if request.ids.len() > ERROR_RULES_MAX {
        return Err(Error::invalid(format!(
            "an organization keeps at most {ERROR_RULES_MAX} error rules"
        )));
    }

    let catalog = &state.telemetry.apm_catalog;
    let ids = request
        .ids
        .into_iter()
        .map(Id::from_string)
        .collect::<Vec<_>>();

    catalog.order_error_rules(&ctx.org_id, &ids).await?;

    let rules = catalog.list_error_rules(&ctx.org_id).await?;

    apply(&state, &ctx.org_id, rules.clone());

    activity_audit::record(
        &state,
        &ctx,
        "apm.error_rule.order",
        "apm_error_rule",
        "",
        serde_json::json!({
            "ids": ids.iter().map(Id::as_str).collect::<Vec<_>>(),
        }),
    )
    .await;

    Ok(Json(rules))
}

/// Puts the organization's rules, as now kept, where this node's errors are
/// fingerprinted. A failed read leaves it to the refresh.
async fn reapply(state: &AppState, org_id: &Id) {
    match state.telemetry.apm_catalog.list_error_rules(org_id).await {
        Ok(rules) => apply(state, org_id, rules),
        Err(error) => tracing::warn!(
            target: "molesignal::api::apm",
            error = %error,
            "APM error rules could not be read back after a change"
        ),
    }
}

fn apply(state: &AppState, org_id: &Id, rules: Vec<ErrorRule>) {
    if let Some(book) = state
        .telemetry
        .apm_runtime
        .as_ref()
        .and_then(|runtime| runtime.error_rules())
    {
        book.replace_org(org_id, rules);
    }
}

async fn audit(state: &AppState, ctx: &IamContext, action: &str, rule: &ErrorRule) {
    activity_audit::record(
        state,
        ctx,
        action,
        "apm_error_rule",
        rule.id.as_str(),
        serde_json::json!({
            "name": rule.name,
            "action": rule.action,
            "enabled": rule.enabled,
            "serviceNamespace": rule.service_namespace,
            "serviceName": rule.service_name,
        }),
    )
    .await;
}
