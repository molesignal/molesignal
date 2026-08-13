// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{Value, json};

use super::super::OperationOutcome;
use crate::{
    agent::model::ApprovalRequest,
    api::{
        AppState,
        http::{
            federation::{delete_payload, emit_cud},
            middleware::Permission,
        },
    },
    app::iam::IamContext,
    domain::{
        alerting::{
            anomaly::{AnomalyParams, MAX_ANOMALY_LOOKBACK_DAYS, SUPPORTED_DETECTORS},
            incident::Severity,
            rule::{
                AlertQuery, AlertRule, AlertRuleKind, AlertTrigger, RuleState, SeverityThreshold,
            },
        },
        federation::{CudAction, ResourceKind},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) async fn execute(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    match approval.action.as_str() {
        "create_alert_rule" => create(state, ctx, approval).await,
        "update_alert_rule" => update(state, ctx, approval).await,
        "delete_alert_rule" => delete(state, ctx, approval).await,
        "trigger_alert_rule" => trigger(state, ctx, approval).await,
        _ => unreachable!("alert-rule operation received unrelated action"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleWrite {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    kind: AlertRuleKind,
    query: AlertQuery,
    trigger: AlertTrigger,
    #[serde(default)]
    thresholds: Vec<SeverityThreshold>,
    #[serde(default)]
    severity: Option<Severity>,
    #[serde(default)]
    anomaly_params: Option<AnomalyParams>,
    #[serde(default)]
    escalation_policy_id: Option<Id>,
    #[serde(default)]
    labels: BTreeMap<String, String>,
    #[serde(default)]
    annotations: BTreeMap<String, String>,
}

fn default_true() -> bool {
    true
}

async fn create(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    let request: RuleWrite = parse(&approval.parameters)?;
    validate(&request)?;
    let now = TimestampMicros::now();
    let rule = state
        .alerting
        .service
        .create_rule(AlertRule {
            id: Id(approval.target.clone()),
            org_id: ctx.org_id.clone(),
            name: request.name,
            description: request.description,
            enabled: request.enabled,
            kind: request.kind,
            query: request.query,
            trigger: request.trigger,
            anomaly_params: normalize_anomaly(request.kind, request.anomaly_params)?,
            thresholds: request.thresholds,
            severity: request.severity,
            escalation_policy_id: request
                .escalation_policy_id
                .unwrap_or_else(|| Id::from_string("default")),
            labels: request.labels,
            annotations: request.annotations,
            last_eval_at: None,
            last_state: RuleState::Healthy,
            created_at: now,
            updated_at: now,
        })
        .await?;
    emit_rule_cud(state, &rule, CudAction::Created).await;
    outcome("alert rule created", rule)
}

async fn update(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    let request: RuleWrite = parse(&approval.parameters)?;
    validate(&request)?;
    let existing = load_authorized(state, ctx, &approval.target).await?;
    let rule = state
        .alerting
        .service
        .update_rule(AlertRule {
            id: existing.id,
            org_id: existing.org_id,
            name: request.name,
            description: request.description,
            enabled: request.enabled,
            kind: request.kind,
            query: request.query,
            trigger: request.trigger,
            anomaly_params: normalize_anomaly(request.kind, request.anomaly_params)?,
            thresholds: request.thresholds,
            severity: request.severity,
            escalation_policy_id: request
                .escalation_policy_id
                .unwrap_or(existing.escalation_policy_id),
            labels: request.labels,
            annotations: request.annotations,
            last_eval_at: existing.last_eval_at,
            last_state: existing.last_state,
            created_at: existing.created_at,
            updated_at: TimestampMicros::now(),
        })
        .await?;
    emit_rule_cud(state, &rule, CudAction::Updated).await;
    outcome("alert rule updated", rule)
}

async fn delete(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    require_no_parameters(approval)?;
    let rule = load_authorized(state, ctx, &approval.target).await?;
    state.alerting.service.delete_rule(&rule.id).await?;
    emit_cud(
        state,
        &rule.org_id,
        ResourceKind::AlertRule,
        CudAction::Deleted,
        &rule.id.0,
        &delete_payload(&rule.id.0),
    )
    .await;
    Ok(OperationOutcome {
        summary: "alert rule deleted".into(),
        verification: json!({"verified": true, "rule_id": rule.id, "deleted": true}),
    })
}

async fn trigger(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    require_no_parameters(approval)?;
    let rule = load_authorized(state, ctx, &approval.target).await?;
    let incident = state
        .alerting
        .evaluator
        .trigger_rule(&rule, &approval.requested_by, TimestampMicros::now())
        .await?;
    Ok(OperationOutcome {
        summary: format!("manually triggered alert rule {}", rule.id.0),
        verification: json!({
            "verified": true,
            "rule_id": rule.id,
            "incident": incident,
        }),
    })
}

async fn load_authorized(state: &AppState, ctx: &IamContext, id: &str) -> Result<AlertRule> {
    let rule = state.alerting.service.get_rule(&Id(id.to_string())).await?;
    if rule.org_id != ctx.org_id {
        return Err(Error::not_found("alert rule not found"));
    }
    Permission::require_resource(
        state,
        ctx,
        "alerts.manage",
        &rule.org_id,
        "alert",
        &rule.id.0,
    )
    .await?;
    Ok(rule)
}

fn validate(request: &RuleWrite) -> Result<()> {
    if request.name.trim().is_empty() {
        return Err(Error::invalid("name cannot be empty"));
    }
    if request.query.statement.trim().is_empty() {
        return Err(Error::invalid("query.statement cannot be empty"));
    }
    let stream = request
        .query
        .stream
        .as_ref()
        .ok_or_else(|| Error::invalid("query.stream is required"))?;
    if stream.name.trim().is_empty() {
        return Err(Error::invalid("query.stream.name cannot be empty"));
    }
    normalize_anomaly(request.kind, request.anomaly_params.clone()).map(|_| ())
}

fn normalize_anomaly(
    kind: AlertRuleKind,
    parameters: Option<AnomalyParams>,
) -> Result<Option<AnomalyParams>> {
    if kind != AlertRuleKind::Anomaly {
        return Ok(None);
    }
    let parameters = parameters
        .ok_or_else(|| Error::invalid("anomaly_params is required when kind = anomaly"))?;
    if !SUPPORTED_DETECTORS.contains(&parameters.algorithm.as_str()) {
        return Err(Error::invalid(format!(
            "anomaly detector not yet supported: {}",
            parameters.algorithm
        )));
    }
    if parameters.lookback_days < 1 || parameters.lookback_days > MAX_ANOMALY_LOOKBACK_DAYS {
        return Err(Error::invalid(format!(
            "anomaly lookback_days must be between 1 and {MAX_ANOMALY_LOOKBACK_DAYS}"
        )));
    }
    if !parameters.k.is_finite() || parameters.k <= 0.0 {
        return Err(Error::invalid("anomaly k must be a positive number"));
    }
    if parameters.algorithm == "ewma"
        && (!parameters.alpha.is_finite() || parameters.alpha <= 0.0 || parameters.alpha > 1.0)
    {
        return Err(Error::invalid("anomaly alpha must be in (0, 1] for ewma"));
    }
    if parameters.weekly_seasonality && parameters.lookback_days < 7 {
        return Err(Error::invalid(
            "weekly_seasonality requires lookback_days >= 7",
        ));
    }
    Ok(Some(parameters))
}

async fn emit_rule_cud(state: &AppState, rule: &AlertRule, action: CudAction) {
    emit_cud(
        state,
        &rule.org_id,
        ResourceKind::AlertRule,
        action,
        &rule.id.0,
        rule,
    )
    .await;
}

fn outcome(summary: &str, rule: AlertRule) -> Result<OperationOutcome> {
    Ok(OperationOutcome {
        summary: summary.into(),
        verification: json!({"verified": true, "alert_rule": rule}),
    })
}

fn parse<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T> {
    serde_json::from_value(value.clone())
        .map_err(|error| Error::invalid(format!("invalid operation parameters: {error}")))
}

fn require_no_parameters(approval: &ApprovalRequest) -> Result<()> {
    if approval
        .parameters
        .as_object()
        .is_some_and(serde_json::Map::is_empty)
    {
        Ok(())
    } else {
        Err(Error::invalid(
            "trigger_alert_rule does not accept operation parameters",
        ))
    }
}
