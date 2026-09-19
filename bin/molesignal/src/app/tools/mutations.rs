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
    let expires_at_micros = parameters.remove("expires_at_micros");
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
            "expires_at_micros": expires_at_micros,
        }),
    )
    .await
}

fn operation_target(
    kind: BuiltinToolKind,
    parameters: &mut Map<String, Value>,
) -> Result<(&'static str, String)> {
    let mapping = match kind {
        BuiltinToolKind::TriggerAlertRule => ("trigger_alert_rule", Some("rule_id")),
        BuiltinToolKind::AcknowledgeIncident => ("acknowledge_alert", Some("incident_id")),
        BuiltinToolKind::ResolveIncident => ("resolve_alert", Some("incident_id")),
        BuiltinToolKind::CreateAlertRule => ("create_alert_rule", None),
        BuiltinToolKind::UpdateAlertRule => ("update_alert_rule", Some("rule_id")),
        BuiltinToolKind::DeleteAlertRule => ("delete_alert_rule", Some("rule_id")),
        BuiltinToolKind::CreateDashboard => ("create_dashboard_model", None),
        BuiltinToolKind::UpdateDashboard => ("update_dashboard", Some("dashboard_id")),
        BuiltinToolKind::DeleteDashboard => ("delete_dashboard", Some("dashboard_id")),
        BuiltinToolKind::CreateFolder => ("create_folder", None),
        BuiltinToolKind::UpdateFolder => ("update_folder", Some("folder_id")),
        BuiltinToolKind::DeleteFolder => ("delete_folder", Some("folder_id")),
        BuiltinToolKind::CreateAnnotation => ("create_annotation", None),
        BuiltinToolKind::UpdateAnnotation => ("update_annotation", Some("annotation_id")),
        BuiltinToolKind::DeleteAnnotation => ("delete_annotation", Some("annotation_id")),
        BuiltinToolKind::AddDashboardPanel => ("add_dashboard_panel", Some("dashboard_id")),
        BuiltinToolKind::UpdateDashboardPanel => ("update_dashboard_panel", Some("dashboard_id")),
        BuiltinToolKind::MoveDashboardPanel => ("move_dashboard_panel", Some("dashboard_id")),
        BuiltinToolKind::DeleteDashboardPanel => ("delete_dashboard_panel", Some("dashboard_id")),
        BuiltinToolKind::SubmitSearchJob => ("submit_search_job", None),
        BuiltinToolKind::CancelSearchJob => ("cancel_search_job", Some("job_id")),
        BuiltinToolKind::RetrySearchJob => ("retry_search_job", Some("job_id")),
        BuiltinToolKind::DeleteSearchJob => ("delete_search_job", Some("job_id")),
        BuiltinToolKind::CreateSavedView => ("create_saved_view", None),
        BuiltinToolKind::UpdateSavedView => ("update_saved_view", Some("view_id")),
        BuiltinToolKind::DeleteSavedView => ("delete_saved_view", Some("view_id")),
        BuiltinToolKind::CreateFunction => ("create_function", None),
        BuiltinToolKind::UpdateFunction => ("update_function", Some("function_id")),
        BuiltinToolKind::DeleteFunction => ("delete_function", Some("function_id")),
        BuiltinToolKind::EnableScheduledPipeline => {
            ("enable_scheduled_pipeline", Some("pipeline_id"))
        }
        BuiltinToolKind::DisableScheduledPipeline => {
            ("disable_scheduled_pipeline", Some("pipeline_id"))
        }
        BuiltinToolKind::DeleteScheduledPipeline => {
            ("delete_scheduled_pipeline", Some("pipeline_id"))
        }
        BuiltinToolKind::RunSyntheticMonitor => ("run_synthetic_monitor", Some("monitor_id")),
        BuiltinToolKind::PauseSyntheticMonitor => ("pause_synthetic_monitor", Some("monitor_id")),
        BuiltinToolKind::ResumeSyntheticMonitor => ("resume_synthetic_monitor", Some("monitor_id")),
        BuiltinToolKind::ArchiveSyntheticMonitor => {
            ("archive_synthetic_monitor", Some("monitor_id"))
        }
        BuiltinToolKind::ArchiveStatusPage => ("archive_status_page", Some("page_id")),
        BuiltinToolKind::RestoreStatusPage => ("restore_status_page", Some("page_id")),
        BuiltinToolKind::PauseStatusPageAutomation => {
            ("pause_status_page_automation", Some("page_id"))
        }
        BuiltinToolKind::ResumeStatusPageAutomation => {
            ("resume_status_page_automation", Some("page_id"))
        }
        BuiltinToolKind::RetryNotificationDelivery => {
            ("retry_notification_delivery", Some("delivery_id"))
        }
        BuiltinToolKind::AcknowledgeNotificationDelivery => {
            ("acknowledge_notification_delivery", Some("delivery_id"))
        }
        BuiltinToolKind::CreateApiToken => ("create_api_token", None),
        BuiltinToolKind::RevokeApiToken => ("revoke_api_token", Some("token_id")),
        BuiltinToolKind::CreateServiceAccount => ("create_service_account", None),
        BuiltinToolKind::UpdateServiceAccount => {
            ("update_service_account", Some("service_account_id"))
        }
        BuiltinToolKind::EnableServiceAccount => {
            ("enable_service_account", Some("service_account_id"))
        }
        BuiltinToolKind::DisableServiceAccount => {
            ("disable_service_account", Some("service_account_id"))
        }
        BuiltinToolKind::DeleteServiceAccount => {
            ("delete_service_account", Some("service_account_id"))
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
            "rule_id",
            "incident_id",
            "dashboard_id",
            "folder_id",
            "annotation_id",
            "job_id",
            "view_id",
            "function_id",
            "pipeline_id",
            "monitor_id",
            "page_id",
            "delivery_id",
            "token_id",
            "service_account_id",
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
