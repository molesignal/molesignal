// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Shared approved-operation coordinator used by Mole Agent and future inbound MCP adapters.

use serde_json::{Value, json};

use super::{dashboard_operation, execution_response, require_license, resource_operation};
use crate::{
    agent::model::{ApprovalRequest, ApprovalStatus, Execution, ExecutionStatus},
    api::{
        AppState,
        http::{middleware::Permission, routes::activity_audit},
    },
    app::{
        iam::IamContext,
        tools::dashboard::{operation_permission_allowed, operation_policy},
    },
    domain::alerting::incident::IncidentStatus,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(in crate::api::http::routes::agent) struct OperationExecution {
    pub execution: Execution,
    pub one_time_result: Option<Value>,
}

pub(in crate::api::http::routes::agent) async fn execute_approved_operation(
    state: &AppState,
    ctx: &IamContext,
    approval_id: &Id,
    idempotency_key: String,
) -> Result<OperationExecution> {
    require_license(state)?;
    if idempotency_key.trim().is_empty() || idempotency_key.len() > 128 {
        return Err(Error::invalid(
            "idempotency_key length must be between 1 and 128",
        ));
    }
    let approval = state
        .agent
        .repository
        .get_approval(&ctx.org_id, approval_id)
        .await?;
    authorize_execution_actor(ctx, &approval)?;
    let (_, required_permission) = operation_policy(&approval.action)?;
    if !operation_permission_allowed(ctx, &approval.action, required_permission) {
        return Err(Error::forbidden(format!(
            "operation '{}' requires permission '{}'",
            approval.action, required_permission
        )));
    }
    if let Some(existing) = state
        .agent
        .repository
        .find_execution_by_key(&ctx.org_id, &idempotency_key)
        .await?
    {
        if existing.approval_request_id != approval.id {
            return Err(Error::conflict(
                "idempotency_key is already bound to another approval",
            ));
        }
        return Ok(OperationExecution {
            execution: existing,
            one_time_result: None,
        });
    }
    if approval.status == ApprovalStatus::Executed {
        return state
            .agent
            .repository
            .list_executions(&ctx.org_id)
            .await?
            .into_iter()
            .find(|execution| execution.approval_request_id == approval.id)
            .map(|execution| OperationExecution {
                execution,
                one_time_result: None,
            })
            .ok_or_else(|| Error::internal("executed approval is missing its execution"));
    }
    if approval.status != ApprovalStatus::Approved {
        return Err(Error::conflict("approval has not reached approved status"));
    }
    if approval
        .expires_at
        .is_some_and(|expires| expires.0 <= TimestampMicros::now().0)
    {
        return Err(Error::conflict("approval has expired"));
    }
    let approved_by = approval
        .reviews
        .as_array()
        .into_iter()
        .flatten()
        .filter(|review| review["decision"] == "approved")
        .filter_map(|review| review["reviewer_id"].as_str())
        .map(|id| Id(id.to_string()))
        .collect::<Vec<_>>();
    let now = TimestampMicros::now();
    let mut execution = Execution {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        approval_request_id: approval.id.clone(),
        investigation_id: approval.investigation_id.clone(),
        action: approval.action.clone(),
        target: approval.target.clone(),
        parameters: approval.parameters.clone(),
        idempotency_key,
        requested_by: approval.requested_by.clone(),
        approved_by,
        status: ExecutionStatus::Running,
        output_summary: None,
        error: None,
        verification: json!({}),
        started_at: Some(now),
        finished_at: None,
        created_at: now,
        updated_at: now,
    };
    let proposed_execution_id = execution.id.clone();
    execution = state.agent.repository.create_execution(execution).await?;
    if execution.id != proposed_execution_id {
        if execution.approval_request_id != approval.id {
            return Err(Error::conflict(
                "idempotency_key is already bound to another approval",
            ));
        }
        return Ok(OperationExecution {
            execution,
            one_time_result: None,
        });
    }
    let outcome = run_registered_operation(state, ctx, &approval).await;
    let finished = TimestampMicros::now();
    let mut one_time_result = None;
    match outcome {
        Ok(mut outcome) => {
            one_time_result = execution_response::take_one_time_result(&mut outcome.verification);
            execution.status = ExecutionStatus::Succeeded;
            execution.output_summary = Some(outcome.summary);
            execution.verification = outcome.verification;
        }
        Err(error) => {
            execution.status = ExecutionStatus::Failed;
            execution.error = Some(error.to_string());
            execution.verification = json!({"verified": false});
        }
    }
    execution.finished_at = Some(finished);
    execution.updated_at = finished;
    execution = state.agent.repository.update_execution(execution).await?;
    let _ = state
        .agent
        .repository
        .mark_approval_executed(&ctx.org_id, approval_id, finished)
        .await?;
    activity_audit::record(
        state,
        ctx,
        "agent.execution.completed",
        "agent_execution",
        &execution.id.0,
        json!({
            "approval_id": approval.id,
            "action": execution.action,
            "target": execution.target,
            "status": execution.status,
            "idempotency_key": execution.idempotency_key,
        }),
    )
    .await;
    Ok(OperationExecution {
        execution,
        one_time_result,
    })
}

fn authorize_execution_actor(ctx: &IamContext, approval: &ApprovalRequest) -> Result<()> {
    if approval.required_approvals > 0 {
        if &approval.requested_by == ctx.principal_id() {
            Ok(())
        } else {
            Permission::require_key(ctx, "agent.approve")
        }
    } else if &approval.requested_by == ctx.principal_id() {
        Ok(())
    } else {
        Err(Error::forbidden(
            "only the requester can execute a confirmation-mode operation",
        ))
    }
}

async fn run_registered_operation(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    if approval.action == "create_dashboard" {
        return dashboard_operation::execute(state, ctx, approval).await;
    }
    if resource_operation::supports(&approval.action) {
        return resource_operation::execute(state, ctx, approval).await;
    }
    let incident_id = Id(approval.target.clone());
    let incident = state.alerting.service.get_incident(&incident_id).await?;
    if incident.org_id != ctx.org_id {
        return Err(Error::forbidden("alert belongs to another organization"));
    }
    match approval.action.as_str() {
        "acknowledge_alert" => match incident.status {
            IncidentStatus::Acknowledged => Ok(OperationOutcome::verified(
                "alert was already acknowledged; no change",
            )),
            IncidentStatus::Open => {
                state
                    .alerting
                    .service
                    .acknowledge(
                        &incident_id,
                        ctx.principal_id().clone(),
                        TimestampMicros::now(),
                    )
                    .await?;
                Ok(OperationOutcome::verified(
                    "alert acknowledged and state re-read successfully",
                ))
            }
            _ => Err(Error::conflict("only an open alert can be acknowledged")),
        },
        "resolve_alert" => match incident.status {
            IncidentStatus::Resolved | IncidentStatus::Closed => Ok(OperationOutcome::verified(
                "alert was already resolved; no change",
            )),
            IncidentStatus::Open | IncidentStatus::Acknowledged => {
                state
                    .alerting
                    .service
                    .resolve(
                        &incident_id,
                        ctx.principal_id().clone(),
                        TimestampMicros::now(),
                    )
                    .await?;
                Ok(OperationOutcome::verified(
                    "alert resolved and state re-read successfully",
                ))
            }
        },
        other => Err(Error::invalid(format!(
            "operation `{other}` is not registered"
        ))),
    }
}

pub(super) struct OperationOutcome {
    pub(super) summary: String,
    pub(super) verification: Value,
}

impl OperationOutcome {
    fn verified(summary: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
            verification: json!({"verified": true}),
        }
    }

    pub(super) fn with_one_time_result(
        summary: impl Into<String>,
        verification: Value,
        one_time_result: Value,
    ) -> Self {
        Self {
            summary: summary.into(),
            verification: execution_response::attach_one_time_result(verification, one_time_result),
        }
    }
}
