// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use domain::iam::IamContext;
use kernel::{Error, Result, ids::Id};
use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::{
    RiskLevel, ToolExecutionMode, ToolInvocationContext, ToolResult, catalog::BuiltinToolKind,
};

use super::{
    ToolRuntime,
    approvals::{CreateApprovalRequest, create_agent_approval},
    common::parse_args,
};

pub(super) async fn execute(
    runtime: &ToolRuntime,
    auth: &IamContext,
    ctx: &ToolInvocationContext,
    kind: BuiltinToolKind,
    arguments: Value,
) -> Result<ToolResult> {
    match kind {
        BuiltinToolKind::GetDashboardCapabilities => {
            let _: EmptyArgs = parse_args(arguments)?;
            result_json(&runtime.content.dashboard_authoring.capabilities().await?)
        }
        BuiltinToolKind::PrepareDashboard => {
            let prepared = runtime
                .content
                .dashboard_authoring
                .prepare(auth.org_id.clone(), auth.user_id.clone(), arguments)
                .await?;

            result_json(&prepared)
        }
        BuiltinToolKind::ProposeDashboardCreation => {
            propose_dashboard(runtime, auth, ctx, arguments).await
        }
        BuiltinToolKind::ProposeOperation => propose_operation(runtime, auth, ctx, arguments).await,
        BuiltinToolKind::ProposeAlertAction
        | BuiltinToolKind::ProposeSyntheticMonitorAction
        | BuiltinToolKind::ProposeStatusPageAction
        | BuiltinToolKind::ProposeNotificationAction
        | BuiltinToolKind::ProposeScheduledPipelineAction => {
            propose_typed_operation(runtime, auth, ctx, kind, arguments).await
        }
        _ => unreachable!("dashboard handler received unrelated tool"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyArgs {}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DashboardProposalArgs {
    draft_id: String,
    expected_hash: String,
    reason: String,
    impact: String,
}

async fn propose_dashboard(
    runtime: &ToolRuntime,
    auth: &IamContext,
    ctx: &ToolInvocationContext,
    arguments: Value,
) -> Result<ToolResult> {
    require_proposal_policy(ctx)?;
    let args: DashboardProposalArgs = parse_args(arguments)?;
    let draft_id = Id(args.draft_id);

    let draft = runtime
        .content
        .dashboard_authoring
        .validate_reference(&auth.org_id, &auth.user_id, &draft_id, &args.expected_hash)
        .await?;

    let approval = create_agent_approval(
        runtime,
        auth,
        CreateApprovalRequest {
            investigation_id: ctx.investigation_id().map(|value| Id(value.to_string())),
            action: "create_dashboard".into(),
            target: draft_id.0,
            parameters: json!({"expectedHash": args.expected_hash}),
            reason: args.reason,
            impact: args.impact,
            expires_at: Some(draft.expires_at.0),
            required_approvals_override: execution_mode_reviews(ctx)?,
            risk_override: None,
        },
    )
    .await?;

    Ok(ToolResult::json(json!({
        "approval": approval, "draftId": draft.id, "modelHash": draft.model_hash,
        "message": "The dashboard creation proposal was submitted. No dashboard will be created until it is explicitly confirmed or approved and executed.",
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OperationProposalArgs {
    action: String,
    target: String,
    #[serde(default)]
    parameters: Value,
    reason: String,
    impact: String,
    #[serde(default)]
    expires_at: Option<i64>,
}

async fn propose_operation(
    runtime: &ToolRuntime,
    auth: &IamContext,
    ctx: &ToolInvocationContext,
    arguments: Value,
) -> Result<ToolResult> {
    require_proposal_policy(ctx)?;
    let args: OperationProposalArgs = parse_args(arguments)?;

    let approval = create_agent_approval(
        runtime,
        auth,
        CreateApprovalRequest {
            investigation_id: ctx.investigation_id().map(|value| Id(value.to_string())),
            action: args.action,
            target: args.target,
            parameters: args.parameters,
            reason: args.reason,
            impact: args.impact,
            expires_at: args.expires_at,
            required_approvals_override: execution_mode_reviews(ctx)?,
            risk_override: None,
        },
    )
    .await?;

    Ok(ToolResult::json(json!({
        "approval": approval,
        "message": "Mole Agent created an approval request. The operation will not run before approval.",
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TypedOperationProposalArgs {
    action: String,
    incident_id: Option<String>,
    monitor_id: Option<String>,
    page_id: Option<String>,
    delivery_id: Option<String>,
    pipeline_id: Option<String>,
    reason: String,
    impact: String,
    #[serde(default)]
    expires_at: Option<i64>,
}

async fn propose_typed_operation(
    runtime: &ToolRuntime,
    auth: &IamContext,
    ctx: &ToolInvocationContext,
    kind: BuiltinToolKind,
    arguments: Value,
) -> Result<ToolResult> {
    require_proposal_policy(ctx)?;
    let args: TypedOperationProposalArgs = parse_args(arguments)?;

    let (target, allowed) = match kind {
        BuiltinToolKind::ProposeAlertAction => (
            args.incident_id,
            &["acknowledge_alert", "resolve_alert"][..],
        ),
        BuiltinToolKind::ProposeSyntheticMonitorAction => (
            args.monitor_id,
            &[
                "run_synthetic_monitor",
                "pause_synthetic_monitor",
                "resume_synthetic_monitor",
                "archive_synthetic_monitor",
            ][..],
        ),
        BuiltinToolKind::ProposeStatusPageAction => (
            args.page_id,
            &[
                "archive_status_page",
                "restore_status_page",
                "pause_status_page_automation",
                "resume_status_page_automation",
            ][..],
        ),
        BuiltinToolKind::ProposeNotificationAction => (
            args.delivery_id,
            &[
                "retry_notification_delivery",
                "acknowledge_notification_delivery",
            ][..],
        ),
        BuiltinToolKind::ProposeScheduledPipelineAction => (
            args.pipeline_id,
            &[
                "enable_scheduled_pipeline",
                "disable_scheduled_pipeline",
                "delete_scheduled_pipeline",
            ][..],
        ),
        _ => unreachable!("typed operation helper received unrelated tool"),
    };

    if !allowed.contains(&args.action.as_str()) {
        return Err(Error::invalid(format!(
            "action '{}' is not valid for tool '{}'",
            args.action,
            kind.name()
        )));
    }

    let target = target
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| Error::invalid("operation target is required"))?;

    let approval = create_agent_approval(
        runtime,
        auth,
        CreateApprovalRequest {
            investigation_id: ctx.investigation_id().map(|value| Id(value.to_string())),
            action: args.action,
            target,
            parameters: json!({}),
            reason: args.reason,
            impact: args.impact,
            expires_at: args.expires_at,
            required_approvals_override: execution_mode_reviews(ctx)?,
            risk_override: None,
        },
    )
    .await?;

    Ok(ToolResult::json(json!({
        "approval": approval,
        "message": "Mole Agent created an approval request. The operation will not run before approval.",
    })))
}

fn require_proposal_policy(ctx: &ToolInvocationContext) -> Result<()> {
    if !ctx.execution_policy().allows_approval_request() {
        Err(Error::forbidden(
            "the current execution policy does not allow creating approval requests",
        ))
    } else if ctx.execution_mode().required_approvals().is_none() {
        Err(Error::forbidden(
            "the active Tool Policy disables this operation",
        ))
    } else {
        Ok(())
    }
}

fn execution_mode_reviews(ctx: &ToolInvocationContext) -> Result<Option<i32>> {
    ctx.execution_mode()
        .required_approvals()
        .map(Some)
        .ok_or_else(|| Error::forbidden("the active Tool Policy disables this operation"))
}

pub(crate) fn operation_policy(action: &str) -> Result<(RiskLevel, &'static str)> {
    match action {
        "acknowledge_alert" | "resolve_alert" | "snooze_alert" => {
            Ok((RiskLevel::L2, "alerts.incidents.acknowledge"))
        }
        "reassign_alert"
        | "add_alert_responders"
        | "lift_alert_inhibition"
        | "set_alert_severity"
        | "set_alert_priority"
        | "set_alert_urgency" => Ok((RiskLevel::L2, "alerts.incidents.update")),
        "escalate_alert" | "merge_alerts" => Ok((RiskLevel::L2, "alerts.incidents.escalate")),
        "create_incident" => Ok((RiskLevel::L2, "alerts.incidents.create")),
        "note_alert" => Ok((RiskLevel::L1, "alerts.incidents.update")),
        // 从页面发起时按动作自己设的 L2 或 L3；Mole Agent 提议的一律按 L3。
        "run_remediation_action" => Ok((RiskLevel::L3, "alerts.incidents.remediate")),
        "create_incident_ticket" | "assign_incident_role" | "add_incident_task" => {
            Ok((RiskLevel::L1, "alerts.incidents.update"))
        }
        "declare_major_incident" | "post_incident_update" => {
            Ok((RiskLevel::L2, "alerts.incidents.escalate"))
        }
        "draft_incident_postmortem" => Ok((RiskLevel::L1, "postmortems.manage")),
        "create_dashboard" => Ok((RiskLevel::L1, "dashboards.create")),
        "run_synthetic_monitor" | "pause_synthetic_monitor" | "resume_synthetic_monitor" => {
            Ok((RiskLevel::L2, "synthetics.monitors.manage"))
        }
        "archive_synthetic_monitor" => Ok((RiskLevel::L3, "synthetics.monitors.archive")),
        "archive_status_page" => Ok((RiskLevel::L3, "status_pages.archive")),
        "restore_status_page"
        | "pause_status_page_automation"
        | "resume_status_page_automation" => Ok((RiskLevel::L2, "status_pages.automation.manage")),
        "draft_status_page_postmortem" => Ok((RiskLevel::L1, "status_pages.events.manage")),
        "run_log_inspection" => Ok((RiskLevel::L1, "inspections.run")),
        "retry_notification_delivery" => Ok((RiskLevel::L2, "notify.deliveries.retry")),
        "acknowledge_notification_delivery" => Ok((RiskLevel::L2, "notify.deliveries.acknowledge")),
        "enable_scheduled_pipeline" | "disable_scheduled_pipeline" => {
            Ok((RiskLevel::L2, "pipelines.edit"))
        }
        "delete_scheduled_pipeline" => Ok((RiskLevel::L3, "pipelines.delete")),
        "trigger_alert_rule" => Ok((RiskLevel::L3, "alerts.rules.manage")),
        "create_alert_rule" | "update_alert_rule" => Ok((RiskLevel::L2, "alerts.rules.manage")),
        "delete_alert_rule" => Ok((RiskLevel::L3, "alerts.rules.delete")),
        "create_annotation" => Ok((RiskLevel::L1, "dashboards.annotations.manage")),
        "move_dashboard_panel" => Ok((RiskLevel::L1, "dashboards.edit")),
        "update_annotation" => Ok((RiskLevel::L2, "dashboards.annotations.manage")),
        "add_dashboard_panel" | "update_dashboard_panel" => Ok((RiskLevel::L2, "dashboards.edit")),
        "delete_annotation" => Ok((RiskLevel::L3, "dashboards.annotations.manage")),
        "delete_dashboard_panel" => Ok((RiskLevel::L3, "dashboards.edit")),
        "create_dashboard_model" => Ok((RiskLevel::L2, "dashboards.create")),
        "update_dashboard" => Ok((RiskLevel::L2, "dashboards.edit")),
        "delete_dashboard" => Ok((RiskLevel::L3, "dashboards.delete")),
        "create_folder" => Ok((RiskLevel::L1, "dashboards.folders.manage")),
        "update_folder" => Ok((RiskLevel::L2, "dashboards.folders.manage")),
        "delete_folder" => Ok((RiskLevel::L3, "dashboards.folders.delete")),
        "submit_search_job" | "cancel_search_job" => Ok((RiskLevel::L1, "query.jobs.run")),
        "retry_search_job" => Ok((RiskLevel::L2, "query.jobs.run")),
        "delete_search_job" => Ok((RiskLevel::L3, "query.jobs.run")),
        "create_saved_view" => Ok((RiskLevel::L1, "saved_views.create")),
        "update_saved_view" => Ok((RiskLevel::L2, "saved_views.edit")),
        "delete_saved_view" => Ok((RiskLevel::L3, "saved_views.delete")),
        "create_function" => Ok((RiskLevel::L2, "functions.create")),
        "update_function" => Ok((RiskLevel::L2, "functions.edit")),
        "delete_function" => Ok((RiskLevel::L3, "functions.delete")),
        "create_api_token" => Ok((RiskLevel::L2, "personal_tokens.manage")),
        "revoke_api_token" => Ok((RiskLevel::L3, "api_tokens.revoke")),
        "create_service_account" | "update_service_account" | "enable_service_account" => {
            Ok((RiskLevel::L2, "service_accounts.manage"))
        }
        "disable_service_account" => Ok((RiskLevel::L3, "service_accounts.manage")),
        "delete_service_account" => Ok((RiskLevel::L3, "service_accounts.delete")),
        other => Err(Error::invalid(format!(
            "operation `{other}` is not registered"
        ))),
    }
}

pub(crate) fn dashboard_required_approvals(mode: ToolExecutionMode) -> Result<i32> {
    mode.required_approvals().ok_or_else(|| {
        Error::forbidden("Dashboard creation is disabled by the active operation policy")
    })
}

pub(crate) fn operation_permission_allowed(
    auth: &IamContext,
    action: &str,
    required_permission: &str,
) -> bool {
    if matches!(
        action,
        "submit_search_job" | "cancel_search_job" | "retry_search_job" | "delete_search_job"
    ) {
        auth.has_permission("streams.query") || auth.has_permission("sys.telemetry.read")
    } else {
        auth.has_permission(required_permission)
    }
}

fn result_json(value: &impl serde::Serialize) -> Result<ToolResult> {
    serde_json::to_value(value)
        .map(ToolResult::json)
        .map_err(|error| Error::internal(error.to_string()))
}
