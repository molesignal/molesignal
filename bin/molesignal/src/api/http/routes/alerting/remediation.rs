// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 修复动作：
//!
//! - `GET/POST /alerts/remediation/actions`、`PUT/DELETE /alerts/remediation/actions/{id}`：定义动作
//!   （看要 `alerts.remediation.read`，改要 `alerts.remediation.manage`）。请求头的值和签名密钥不出库：保存时留空的请求头与
//!   没传的签名密钥保持原样，传空串清掉签名密钥。
//! - Kubernetes 集群的登记在 `remediation_clusters`。
//! - `GET /alerts/incidents/{id}/remediations`：这个事件能用的动作与发起过的；
//! - `POST /alerts/incidents/{id}/remediations`：发起（要 `alerts.incidents.remediate`），建一个审批请求，
//!   批准后执行。

use std::collections::BTreeMap;

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{get, put},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::RiskLevel;

use crate::{
    agent::model::ApprovalStatus,
    api::{AppState, http::routes::activity_audit},
    app::{
        iam::IamContext,
        tools::dashboard::{CreateApprovalRequest, create_agent_approval},
    },
    domain::{
        alerting::{
            incident::Incident,
            remediation::{
                RemediationAction, RemediationParameter, RemediationRisk, RemediationRunStatus,
                RemediationTarget, VerifyBy,
            },
        },
        iam::{permission, resource_permission},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/alerts/remediation/actions",
            get(list_actions).post(create_action),
        )
        .route(
            "/alerts/remediation/actions/{id}",
            put(update_action).delete(delete_action),
        )
        .route(
            "/alerts/incidents/{id}/remediations",
            get(incident_remediations).post(request_remediation),
        )
}

/// 页面填的一个动作；ID、组织、谁建的与时间由这里定。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActionReq {
    name: String,
    #[serde(default)]
    description: String,
    target: RemediationTarget,
    #[serde(default)]
    parameters: Vec<RemediationParameter>,
    #[serde(default)]
    services: Vec<String>,
    #[serde(default)]
    team_ids: Vec<Id>,
    #[serde(default)]
    risk: RemediationRisk,
    #[serde(default)]
    verify_after_secs: u32,
    #[serde(default)]
    verify_by: VerifyBy,
    #[serde(default = "enabled_by_default")]
    enabled: bool,
}

fn enabled_by_default() -> bool {
    true
}

impl ActionReq {
    fn into_action(self, id: Id, org_id: Id, now: TimestampMicros) -> RemediationAction {
        RemediationAction {
            id,
            org_id,
            name: self.name,
            description: self.description,
            target: self.target,
            parameters: self.parameters,
            services: self
                .services
                .into_iter()
                .map(|service| service.trim().to_string())
                .filter(|service| !service.is_empty())
                .collect(),
            team_ids: self.team_ids,
            risk: self.risk,
            verify_after_secs: self.verify_after_secs,
            verify_by: self.verify_by,
            enabled: self.enabled,
            secrets_set: Vec::new(),
            created_by: None,
            updated_by: None,
            created_at: now,
            updated_at: now,
        }
    }
}

#[permission("alerts.remediation.read")]
async fn list_actions(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    let actions = state
        .alerting
        .remediation
        .repository()
        .list_actions(&ctx.org_id)
        .await?;

    Ok(Json(json!({ "actions": actions })))
}

#[permission("alerts.remediation.manage")]
async fn create_action(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<ActionReq>,
) -> Result<Json<Value>> {
    let mut action = req.into_action(Id::new(), ctx.org_id.clone(), TimestampMicros::now());

    action.created_by = Some(ctx.user_id.clone());
    action.updated_by = Some(ctx.user_id.clone());

    check_cluster(&state, &ctx.org_id, &action).await?;
    action.validate()?;

    let saved = state
        .alerting
        .remediation
        .repository()
        .save_action(action)
        .await?;

    audit(&state, &ctx, "remediation_action.created", &saved).await;
    Ok(Json(public_action(saved)))
}

#[permission("alerts.remediation.manage")]
async fn update_action(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<ActionReq>,
) -> Result<Json<Value>> {
    let repository = state.alerting.remediation.repository();
    let existing = repository
        .get_action(&ctx.org_id, &Id::from_string(id))
        .await?
        .ok_or_else(|| Error::not_found("remediation action not found"))?;

    let mut action = req.into_action(
        existing.id.clone(),
        existing.org_id.clone(),
        TimestampMicros::now(),
    );

    action.created_by = existing.created_by.clone();
    action.created_at = existing.created_at;
    action.updated_by = Some(ctx.user_id.clone());

    keep_secrets(&mut action.target, &existing.target);
    check_cluster(&state, &ctx.org_id, &action).await?;
    action.validate()?;

    let saved = repository.save_action(action).await?;

    audit(&state, &ctx, "remediation_action.updated", &saved).await;
    Ok(Json(public_action(saved)))
}

#[permission("alerts.remediation.manage")]
async fn delete_action(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let id = Id::from_string(id);

    if !state
        .alerting
        .remediation
        .repository()
        .delete_action(&ctx.org_id, &id)
        .await?
    {
        return Err(Error::not_found("remediation action not found"));
    }

    activity_audit::record(
        &state,
        &ctx,
        "remediation_action.deleted",
        "remediation_action",
        &id.0,
        json!({}),
    )
    .await;

    Ok(Json(json!({ "deleted": true })))
}

/// 请求头留空的、没传签名密钥的，用原来的。
fn keep_secrets(target: &mut RemediationTarget, existing: &RemediationTarget) {
    let (RemediationTarget::Http(http), RemediationTarget::Http(old)) = (target, existing) else {
        return;
    };

    for (name, value) in &mut http.headers {
        if value.is_empty()
            && let Some(kept) = old.headers.get(name)
        {
            value.clone_from(kept);
        }
    }

    match &http.signing_secret {
        None => http.signing_secret.clone_from(&old.signing_secret),
        Some(secret) if secret.is_empty() => http.signing_secret = None,
        Some(_) => {}
    }
}

/// Kubernetes 动作用的集群在本组织。
async fn check_cluster(state: &AppState, org_id: &Id, action: &RemediationAction) -> Result<()> {
    let RemediationTarget::Kubernetes(kubernetes) = &action.target else {
        return Ok(());
    };

    state
        .alerting
        .remediation
        .repository()
        .get_cluster(org_id, &kubernetes.cluster_id)
        .await?
        .map(|_| ())
        .ok_or_else(|| Error::invalid("the Kubernetes cluster does not exist in this organization"))
}

/// 给页面的动作：秘密的值不出库（列表里读出的本来就没有，保存后读出的要去掉）。
fn public_action(mut action: RemediationAction) -> Value {
    if let RemediationTarget::Http(http) = &mut action.target {
        for value in http.headers.values_mut() {
            value.clear();
        }

        http.signing_secret = None;
    }

    serde_json::to_value(action).unwrap_or(Value::Null)
}

async fn audit(state: &AppState, ctx: &IamContext, what: &str, action: &RemediationAction) {
    activity_audit::record(
        state,
        ctx,
        what,
        "remediation_action",
        &action.id.0,
        json!({
            "name": action.name,
            "kind": match action.target {
                RemediationTarget::Http(_) => "http",
                RemediationTarget::Kubernetes(_) => "kubernetes",
            },
            "risk": action.risk,
            "enabled": action.enabled,
        }),
    )
    .await;
}

#[resource_permission(
    action = "alerts.incidents.read",
    resource = Incident,
    id = Id::from_string(id),
    bind = incident
)]
async fn incident_remediations(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let service = &state.alerting.remediation;
    let actions = service.applicable(&incident).await?;
    let mut runs = service
        .repository()
        .list_runs_for_incident(&incident.org_id, &incident.id)
        .await?;

    // 还在等审批的：审批被驳回、过期或撤销了就记成没执行。
    for run in &mut runs {
        if run.status != RemediationRunStatus::AwaitingApproval {
            continue;
        }

        let Ok(approval) = state
            .agent
            .repository
            .get_approval(&incident.org_id, &run.approval_id)
            .await
        else {
            continue;
        };

        let expired = approval
            .expires_at
            .is_some_and(|expires| expires.0 <= TimestampMicros::now().0)
            && matches!(approval.status, ApprovalStatus::Pending);

        let why = match approval.status {
            ApprovalStatus::Rejected => Some("approval rejected"),
            ApprovalStatus::Expired => Some("approval expired"),
            ApprovalStatus::Cancelled => Some("approval cancelled"),
            _ if expired => Some("approval expired"),
            _ => None,
        };

        if let Some(why) = why {
            *run = service.decline(run.clone(), why).await?;
        }
    }

    let actions: Vec<Value> = actions.into_iter().map(public_action).collect();

    Ok(Json(json!({ "actions": actions, "runs": runs })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunReq {
    action_id: Id,
    #[serde(default)]
    inputs: BTreeMap<String, String>,
    #[serde(default)]
    reason: String,
}

#[resource_permission(
    action = "alerts.incidents.remediate",
    resource = Incident,
    id = Id::from_string(id),
    bind = incident
)]
async fn request_remediation(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<RunReq>,
) -> Result<Json<Value>> {
    let service = &state.alerting.remediation;
    let (action, inputs) = service
        .prepare(&incident, &req.action_id, &req.inputs)
        .await?;

    let reason = match req.reason.trim() {
        "" => format!("Run `{}` on incident: {}", action.name, incident.summary),
        reason => reason.to_string(),
    };

    let approval = create_agent_approval(
        &state.tools,
        &ctx,
        CreateApprovalRequest {
            investigation_id: None,
            action: crate::api::http::routes::agent::control::remediation_operation::ACTION.into(),
            target: incident.id.to_string(),
            parameters: json!({
                "actionId": action.id,
                "action": action.name,
                "inputs": inputs,
            }),
            reason: reason.chars().take(2_000).collect(),
            impact: impact(&action, &inputs),
            expires_at: None,
            required_approvals_override: None,
            risk_override: Some(match action.risk {
                RemediationRisk::L2 => RiskLevel::L2,
                RemediationRisk::L3 => RiskLevel::L3,
            }),
        },
    )
    .await?;

    let run = service
        .record_request(
            &incident,
            &action,
            inputs,
            approval.id.clone(),
            ctx.principal_id(),
        )
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "remediation.requested",
        "incident",
        &incident.id.0,
        json!({ "action": action.name, "approvalId": approval.id, "runId": run.id }),
    )
    .await;

    Ok(Json(json!({ "run": run, "approval": approval })))
}

/// 审批上写的影响：会调哪个地址，或在哪个集群上对哪个工作负载做什么。
fn impact(action: &RemediationAction, inputs: &BTreeMap<String, String>) -> String {
    let what = match &action.target {
        RemediationTarget::Http(http) => format!("HTTP {} {}", http.method.as_str(), http.url),
        RemediationTarget::Kubernetes(kubernetes) => format!(
            "Kubernetes {:?} {:?} {}/{}",
            kubernetes.operation,
            kubernetes.workload_kind,
            kubernetes.namespace,
            kubernetes.workload_name
        ),
    };

    let given = inputs
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(", ");

    let text = if given.is_empty() {
        what
    } else {
        format!("{what} ({given})")
    };

    text.chars().take(2_000).collect()
}
