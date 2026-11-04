// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Named mutation tools. Each one creates a typed approval request; execution is centralized in
//! the Agent operation registry so adapters never mutate resources directly.

use serde_json::{Map, Value, json};
use tool_runtime::{ToolInvocationContext, ToolResult, catalog::BuiltinToolKind};

use super::{ToolRuntime, dashboard};
use crate::{
    app::iam::IamContext,
    shared::{Error, Result, ids::Id},
};

pub(super) async fn execute(
    runtime: &ToolRuntime,
    auth: &IamContext,
    ctx: &ToolInvocationContext,
    kind: BuiltinToolKind,
    arguments: Value,
) -> Result<ToolResult> {
    let mut parameters = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| Error::invalid("tool arguments must be an object"))?;

    let reason = take_required_string(&mut parameters, "reason")?;
    let impact = take_required_string(&mut parameters, "impact")?;
    let expires_at = parameters.remove("expiresAt");
    let (action, target) = operation_target(kind, &mut parameters)?;

    dashboard::execute(
        runtime,
        auth,
        ctx,
        BuiltinToolKind::ProposeOperation,
        json!({
            "action": action,
            "target": target,
            "parameters": Value::Object(parameters),
            "reason": reason,
            "impact": impact,
            "expiresAt": expires_at,
        }),
    )
    .await
}

fn operation_target(
    kind: BuiltinToolKind,
    parameters: &mut Map<String, Value>,
) -> Result<(&'static str, String)> {
    let mapping = match kind {
        BuiltinToolKind::TriggerAlertRule => ("trigger_alert_rule", Some("ruleId")),
        BuiltinToolKind::AcknowledgeIncident => ("acknowledge_alert", Some("incidentId")),
        BuiltinToolKind::ResolveIncident => ("resolve_alert", Some("incidentId")),
        BuiltinToolKind::ReassignIncident => ("reassign_alert", Some("incidentId")),
        BuiltinToolKind::AddIncidentResponders => ("add_alert_responders", Some("incidentId")),
        BuiltinToolKind::EscalateIncident => ("escalate_alert", Some("incidentId")),
        BuiltinToolKind::SnoozeIncident => ("snooze_alert", Some("incidentId")),
        BuiltinToolKind::LiftIncidentInhibition => ("lift_alert_inhibition", Some("incidentId")),
        BuiltinToolKind::SetIncidentSeverity => ("set_alert_severity", Some("incidentId")),
        BuiltinToolKind::SetIncidentPriority => ("set_alert_priority", Some("incidentId")),
        BuiltinToolKind::SetIncidentUrgency => ("set_alert_urgency", Some("incidentId")),
        BuiltinToolKind::CreateIncident => ("create_incident", None),
        BuiltinToolKind::MergeIncidents => ("merge_alerts", Some("incidentId")),
        BuiltinToolKind::AddIncidentNote => ("note_alert", Some("incidentId")),
        BuiltinToolKind::CreateTicket => ("create_incident_ticket", Some("incidentId")),
        BuiltinToolKind::RunRemediationAction => ("run_remediation_action", Some("incidentId")),
        BuiltinToolKind::DeclareMajorIncident => ("declare_major_incident", Some("incidentId")),
        BuiltinToolKind::AssignIncidentRole => ("assign_incident_role", Some("incidentId")),
        BuiltinToolKind::PostIncidentUpdate => ("post_incident_update", Some("incidentId")),
        BuiltinToolKind::AddIncidentTask => ("add_incident_task", Some("incidentId")),
        BuiltinToolKind::DraftIncidentPostmortem => {
            ("draft_incident_postmortem", Some("incidentId"))
        }
        BuiltinToolKind::CreateAlertRule => ("create_alert_rule", None),
        BuiltinToolKind::UpdateAlertRule => ("update_alert_rule", Some("ruleId")),
        BuiltinToolKind::DeleteAlertRule => ("delete_alert_rule", Some("ruleId")),
        BuiltinToolKind::CreateDashboard => ("create_dashboard_model", None),
        BuiltinToolKind::UpdateDashboard => ("update_dashboard", Some("dashboardId")),
        BuiltinToolKind::DeleteDashboard => ("delete_dashboard", Some("dashboardId")),
        BuiltinToolKind::CreateFolder => ("create_folder", None),
        BuiltinToolKind::UpdateFolder => ("update_folder", Some("folderId")),
        BuiltinToolKind::DeleteFolder => ("delete_folder", Some("folderId")),
        BuiltinToolKind::CreateAnnotation => ("create_annotation", None),
        BuiltinToolKind::UpdateAnnotation => ("update_annotation", Some("annotationId")),
        BuiltinToolKind::DeleteAnnotation => ("delete_annotation", Some("annotationId")),
        BuiltinToolKind::AddDashboardPanel => ("add_dashboard_panel", Some("dashboardId")),
        BuiltinToolKind::UpdateDashboardPanel => ("update_dashboard_panel", Some("dashboardId")),
        BuiltinToolKind::MoveDashboardPanel => ("move_dashboard_panel", Some("dashboardId")),
        BuiltinToolKind::DeleteDashboardPanel => ("delete_dashboard_panel", Some("dashboardId")),
        BuiltinToolKind::SubmitSearchJob => ("submit_search_job", None),
        BuiltinToolKind::CancelSearchJob => ("cancel_search_job", Some("jobId")),
        BuiltinToolKind::RetrySearchJob => ("retry_search_job", Some("jobId")),
        BuiltinToolKind::DeleteSearchJob => ("delete_search_job", Some("jobId")),
        BuiltinToolKind::CreateSavedView => ("create_saved_view", None),
        BuiltinToolKind::UpdateSavedView => ("update_saved_view", Some("viewId")),
        BuiltinToolKind::DeleteSavedView => ("delete_saved_view", Some("viewId")),
        BuiltinToolKind::CreateFunction => ("create_function", None),
        BuiltinToolKind::UpdateFunction => ("update_function", Some("functionId")),
        BuiltinToolKind::DeleteFunction => ("delete_function", Some("functionId")),
        BuiltinToolKind::EnableScheduledPipeline => {
            ("enable_scheduled_pipeline", Some("pipelineId"))
        }
        BuiltinToolKind::DisableScheduledPipeline => {
            ("disable_scheduled_pipeline", Some("pipelineId"))
        }
        BuiltinToolKind::DeleteScheduledPipeline => {
            ("delete_scheduled_pipeline", Some("pipelineId"))
        }
        BuiltinToolKind::RunSyntheticMonitor => ("run_synthetic_monitor", Some("monitorId")),
        BuiltinToolKind::PauseSyntheticMonitor => ("pause_synthetic_monitor", Some("monitorId")),
        BuiltinToolKind::ResumeSyntheticMonitor => ("resume_synthetic_monitor", Some("monitorId")),
        BuiltinToolKind::ArchiveSyntheticMonitor => {
            ("archive_synthetic_monitor", Some("monitorId"))
        }
        BuiltinToolKind::ArchiveStatusPage => ("archive_status_page", Some("pageId")),
        BuiltinToolKind::RestoreStatusPage => ("restore_status_page", Some("pageId")),
        BuiltinToolKind::PauseStatusPageAutomation => {
            ("pause_status_page_automation", Some("pageId"))
        }
        BuiltinToolKind::ResumeStatusPageAutomation => {
            ("resume_status_page_automation", Some("pageId"))
        }
        // The page stays in the parameters: an incident is read through its page.
        BuiltinToolKind::DraftStatusPagePostmortem => {
            ("draft_status_page_postmortem", Some("eventId"))
        }
        BuiltinToolKind::RunLogInspection => ("run_log_inspection", Some("inspectionId")),
        BuiltinToolKind::RetryNotificationDelivery => {
            ("retry_notification_delivery", Some("deliveryId"))
        }
        BuiltinToolKind::AcknowledgeNotificationDelivery => {
            ("acknowledge_notification_delivery", Some("deliveryId"))
        }
        BuiltinToolKind::CreateApiToken => ("create_api_token", None),
        BuiltinToolKind::RevokeApiToken => ("revoke_api_token", Some("tokenId")),
        BuiltinToolKind::CreateServiceAccount => ("create_service_account", None),
        BuiltinToolKind::UpdateServiceAccount => {
            ("update_service_account", Some("serviceAccountId"))
        }
        BuiltinToolKind::EnableServiceAccount => {
            ("enable_service_account", Some("serviceAccountId"))
        }
        BuiltinToolKind::DisableServiceAccount => {
            ("disable_service_account", Some("serviceAccountId"))
        }
        BuiltinToolKind::DeleteServiceAccount => {
            ("delete_service_account", Some("serviceAccountId"))
        }
        _ => unreachable!("mutation handler received unrelated tool"),
    };

    let target = mapping
        .1
        .map(|field| take_required_string(parameters, field))
        .transpose()?
        .unwrap_or_else(|| Id::new().0);

    Ok((mapping.0, target))
}

fn take_required_string(parameters: &mut Map<String, Value>, field: &str) -> Result<String> {
    parameters
        .remove(field)
        .and_then(|value| value.as_str().map(str::trim).map(str::to_string))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::invalid(format!("{field} is required")))
}

#[cfg(test)]
mod tests {
    use tool_runtime::ToolAccess;

    use super::*;

    #[test]
    fn every_managed_mutation_maps_to_a_registered_operation_with_the_same_risk() {
        let target_fields = [
            "ruleId",
            "incidentId",
            "dashboardId",
            "folderId",
            "annotationId",
            "jobId",
            "viewId",
            "functionId",
            "pipelineId",
            "monitorId",
            "pageId",
            "deliveryId",
            "tokenId",
            "serviceAccountId",
            "inspectionId",
            "eventId",
        ];
        for kind in BuiltinToolKind::ALL
            .iter()
            .copied()
            .filter(|kind| kind.spec().access == ToolAccess::ManagedMutation)
        {
            let mut parameters = target_fields
                .iter()
                .map(|field| ((*field).to_string(), Value::String("target".into())))
                .collect::<Map<_, _>>();
            let (action, _) = operation_target(kind, &mut parameters)
                .unwrap_or_else(|error| panic!("{}: {error}", kind.name()));
            let (risk, _) = dashboard::operation_policy(action)
                .unwrap_or_else(|error| panic!("{} -> {action}: {error}", kind.name()));
            assert_eq!(kind.spec().risk, risk, "{} -> {action}", kind.name());
        }
    }
}
