// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Approval requests for operations that change things: what a request carries, and how one is
//! created. A remediation action covered by a pre-approval is recorded already approved; a request
//! that waits for reviewers tells the people who can approve.

use agent::model::{ApprovalRequest, ApprovalStatus};
use domain::iam::IamContext;
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::RiskLevel;

use super::{
    ToolRuntime,
    dashboard::{operation_permission_allowed, operation_policy},
};
use crate::app::notify::AgentApprovalNotice;

const DEFAULT_APPROVAL_TTL_MICROS: i64 = 60 * 60 * 1_000_000;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateApprovalRequest {
    pub investigation_id: Option<Id>,
    pub action: String,
    pub target: String,
    #[serde(default)]
    pub parameters: Value,
    pub reason: String,
    pub impact: String,
    pub expires_at: Option<i64>,
    #[serde(skip)]
    pub required_approvals_override: Option<i32>,
    /// The risk to use instead of the operation's own (a remediation action has its L2 or L3).
    #[serde(skip)]
    pub risk_override: Option<RiskLevel>,
}

pub(crate) async fn create_agent_approval(
    runtime: &ToolRuntime,
    auth: &IamContext,
    request: CreateApprovalRequest,
) -> Result<ApprovalRequest> {
    let (risk, required_permission) = operation_policy(&request.action)?;
    let risk = request.risk_override.unwrap_or(risk);

    if request.action == "create_api_token"
        && auth.principal_type() != domain::iam::access::IamPrincipalType::User
    {
        return Err(Error::forbidden(
            "personal API tokens can only be requested by an authenticated user",
        ));
    }

    if !operation_permission_allowed(auth, &request.action, required_permission) {
        return Err(Error::forbidden(format!(
            "operation '{}' requires permission '{}'",
            request.action, required_permission
        )));
    }

    if request.target.trim().is_empty() {
        return Err(Error::invalid("operation target cannot be empty"));
    }

    if request.reason.trim().is_empty() || request.reason.chars().count() > 2_000 {
        return Err(Error::invalid(
            "operation reason must contain 1 to 2000 characters",
        ));
    }

    if request.impact.trim().is_empty() || request.impact.chars().count() > 2_000 {
        return Err(Error::invalid(
            "operation impact must contain 1 to 2000 characters",
        ));
    }

    let draft_expiry = if request.action == "create_dashboard" {
        let expected_hash = request
            .parameters
            .get("expectedHash")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::invalid("create_dashboard requires expected_hash"))?;

        if request
            .parameters
            .as_object()
            .is_none_or(|parameters| parameters.len() != 1)
        {
            return Err(Error::invalid(
                "create_dashboard parameters may contain only expected_hash",
            ));
        }

        Some(
            runtime
                .content
                .dashboard_authoring
                .validate_reference(
                    &auth.org_id,
                    &auth.user_id,
                    &Id(request.target.clone()),
                    expected_hash,
                )
                .await?
                .expires_at,
        )
    } else {
        None
    };

    let now = TimestampMicros::now();

    let required_approvals = request
        .required_approvals_override
        .unwrap_or_else(|| risk.required_approvals())
        .clamp(0, 2);

    let expires_at = request
        .expires_at
        .unwrap_or(now.0.saturating_add(DEFAULT_APPROVAL_TTL_MICROS));

    let expires_at = draft_expiry.map_or(expires_at, |draft| expires_at.min(draft.0));
    let id = Id::new();

    // A remediation action pre-approved for the incident is approved at once by the people who
    // pre-approved it; the caller runs it right away.
    let grant = if request.action == "run_remediation_action" {
        let incident_id = Id::from_string(request.target.clone());
        let remediation = &runtime.alerting.remediation;

        remediation
            .preapproval_grant(
                &auth.org_id,
                &incident_id,
                &request.parameters,
                &id,
                auth.principal_id(),
                now,
            )
            .await?
    } else {
        None
    };

    let (status, reviews, decided_at, required_approvals, risk) = match &grant {
        Some(grant) => (
            ApprovalStatus::Approved,
            grant.reviews.clone(),
            Some(now),
            grant.required_approvals,
            grant.risk,
        ),
        None if required_approvals == 0 => {
            (ApprovalStatus::Approved, json!([]), Some(now), 0, risk)
        }
        None => (
            ApprovalStatus::Pending,
            json!([]),
            None,
            required_approvals,
            risk,
        ),
    };

    let created = runtime
        .agent
        .repository
        .create_approval(ApprovalRequest {
            id: id.clone(),
            org_id: auth.org_id.clone(),
            investigation_id: request.investigation_id,
            action: request.action,
            target: request.target,
            parameters: request.parameters,
            reason: request.reason,
            impact: request.impact,
            risk,
            status,
            requested_by: auth.principal_id().clone(),
            required_approvals,
            reviews,
            expires_at: Some(TimestampMicros(expires_at)),
            decided_at,
            created_at: now,
            updated_at: now,
        })
        .await;

    let approval = match created {
        Ok(approval) => approval,
        Err(error) => {
            if grant.is_some() {
                let remediation = &runtime.alerting.remediation;
                remediation.release_preapproved_run(&auth.org_id, &id).await;
            }

            return Err(error);
        }
    };

    // The people who can approve are told; a request that needs nobody is not news.
    if approval.status == ApprovalStatus::Pending && approval.required_approvals > 0 {
        AgentApprovalNotice::new(
            runtime.alerting.notify_engine.clone(),
            runtime.agent.i18n_language.clone(),
            runtime.agent.workspace_defaults.clone(),
            runtime.administration.iam.clone(),
            runtime.administration.iam_access.clone(),
        )
        .requested(&approval);
    }

    Ok(approval)
}
