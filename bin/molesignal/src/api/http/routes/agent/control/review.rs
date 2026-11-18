// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Reviewing an approval request, shared by `POST /agent/approvals/{id}/review` and the approve and
//! reject buttons on an IM card: the requester cannot approve an operation that needs reviewers,
//! the review is recorded and audited, and an approval that reaches the number it needs runs at
//! once.

use agent::model::{ApprovalRequest, ApprovalStatus};
use domain::iam::IamContext;
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde_json::json;

use super::{OperationExecution, execute_approved_operation};
use crate::api::{AppState, http::routes::activity_audit};

/// What one review did.
pub(crate) struct Reviewed {
    /// The request as it is after the review (and after it ran, when it did).
    pub approval: ApprovalRequest,
    /// The run this review started: `None` when it did not complete the approval; an error when
    /// the approved operation could not be started at all.
    pub execution: Option<Result<OperationExecution>>,
}

/// Reviews the request `id` as `ctx`. The caller has checked that `ctx` may approve.
pub(crate) async fn review(
    state: &AppState,
    ctx: &IamContext,
    id: &Id,
    approve: bool,
    comment: &str,
) -> Result<Reviewed> {
    let existing = state.agent.repository.get_approval(&ctx.org_id, id).await?;

    if existing.required_approvals > 0 && &existing.requested_by == ctx.principal_id() {
        return Err(Error::forbidden(
            "requester cannot approve their own reviewed operation",
        ));
    }

    let saved = state
        .agent
        .repository
        .review_approval(
            &ctx.org_id,
            id,
            ctx.principal_id(),
            approve,
            comment,
            TimestampMicros::now(),
        )
        .await?;

    activity_audit::record(
        state,
        ctx,
        if approve {
            "agent.approval.approved"
        } else {
            "agent.approval.rejected"
        },
        "agent_approval",
        &saved.id.0,
        json!({"comment": comment, "status": saved.status}),
    )
    .await;

    if !(approve && saved.status == ApprovalStatus::Approved) {
        return Ok(Reviewed {
            approval: saved,
            execution: None,
        });
    }

    let execution = execute_approved_operation(
        state,
        ctx,
        &saved.id,
        format!("approval:{}:approved", saved.id.0),
    )
    .await;

    // After it ran the request reads `executed`; when it could not start it stays approved.
    let approval = match &execution {
        Ok(_) => {
            state
                .agent
                .repository
                .get_approval(&ctx.org_id, &saved.id)
                .await?
        }
        Err(_) => saved,
    };

    Ok(Reviewed {
        approval,
        execution: Some(execution),
    })
}
