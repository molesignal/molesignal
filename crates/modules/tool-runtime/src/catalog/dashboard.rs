// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::{BuiltinToolKind, object_schema, open_output};
use crate::{RiskLevel, ToolSpec};

pub(super) fn spec(kind: BuiltinToolKind) -> ToolSpec {
    match kind {
        BuiltinToolKind::GetDashboardCapabilities => ToolSpec::read(
            kind.name(), "Return supported Dashboard authoring contracts and limits.",
            "dashboard_reports", "dashboard_authoring",
            object_schema(json!({})), open_output(), &["agent.use", "dashboards.create"], &["Dashboard", "Authoring"],
        ).pinned().mole_agent_only(),
        BuiltinToolKind::PrepareDashboard => ToolSpec::read(
            kind.name(), "Validate, preflight, and persist a temporary Dashboard draft without creating a Dashboard.",
            "dashboard_reports", "dashboard_authoring",
            contracts::dashboard_authoring_validator().schema().clone(),
            open_output(), &["agent.use", "dashboards.create", "streams.query"], &["Dashboard", "Draft"],
        ).preflight().pinned().mole_agent_only(),
        BuiltinToolKind::ProposeDashboardCreation => ToolSpec::read(
            kind.name(), "Create an approval request for a previously prepared Dashboard draft; never creates it directly.",
            "dashboard_reports", "dashboard_authoring",
            json!({
                "type": "object", "required": ["draft_id", "expected_hash", "reason", "impact"],
                "properties": {
                    "draft_id": {"type": "string"}, "expected_hash": {"type": "string"},
                    "reason": {"type": "string"}, "impact": {"type": "string"}
                }, "additionalProperties": false
            }), open_output(), &["agent.use", "dashboards.create"], &["Dashboard", "Approval"],
        ).proposal(RiskLevel::L1).pinned().mole_agent_only(),
        BuiltinToolKind::ProposeOperation => ToolSpec::read(
            kind.name(), "Create an approval request for a registered operation; never executes it directly.",
            "automation", "operations",
            json!({
                "type": "object", "required": ["action", "target", "parameters", "reason", "impact"],
                "properties": {
                    "action": {"type": "string", "enum": [
                        "acknowledge_alert", "resolve_alert",
                        "run_synthetic_monitor", "pause_synthetic_monitor", "resume_synthetic_monitor", "archive_synthetic_monitor",
                        "archive_status_page", "restore_status_page", "pause_status_page_automation", "resume_status_page_automation",
                        "retry_notification_delivery", "acknowledge_notification_delivery",
                        "enable_scheduled_pipeline", "disable_scheduled_pipeline", "delete_scheduled_pipeline"
                    ]},
                    "target": {"type": "string"}, "parameters": {"type": "object"},
                    "reason": {"type": "string"}, "impact": {"type": "string"},
                    "expires_at_micros": {"type": "integer"}
                }, "additionalProperties": false
            }), open_output(), &["agent.use"], &["Approval", "Operations"],
        ).proposal(RiskLevel::L3).mole_agent_only(),
        BuiltinToolKind::ProposeAlertAction => action_proposal(
            kind,
            "Create an approval request to acknowledge or resolve one alert incident.",
            "alert",
            "incident_id",
            &["acknowledge_alert", "resolve_alert"],
            &["agent.use"],
            RiskLevel::L2,
        ),
        BuiltinToolKind::ProposeSyntheticMonitorAction => action_proposal(
            kind,
            "Create an approval request to run, pause, resume, or archive one synthetic monitor.",
            "synthetic",
            "monitor_id",
            &[
                "run_synthetic_monitor", "pause_synthetic_monitor",
                "resume_synthetic_monitor", "archive_synthetic_monitor",
            ],
            &["agent.use", "synthetics.manage"],
            RiskLevel::L3,
        ),
        BuiltinToolKind::ProposeStatusPageAction => action_proposal(
            kind,
            "Create an approval request to archive or restore a status page, or pause/resume its automation.",
            "status_page",
            "page_id",
            &[
                "archive_status_page", "restore_status_page",
                "pause_status_page_automation", "resume_status_page_automation",
            ],
            &["agent.use", "status_pages.manage"],
            RiskLevel::L3,
        ),
        BuiltinToolKind::ProposeNotificationAction => action_proposal(
            kind,
            "Create an approval request to retry or acknowledge one notification delivery.",
            "notification",
            "delivery_id",
            &["retry_notification_delivery", "acknowledge_notification_delivery"],
            &["agent.use"],
            RiskLevel::L2,
        ),
        BuiltinToolKind::ProposeScheduledPipelineAction => action_proposal(
            kind,
            "Create an approval request to enable, disable, or delete one scheduled pipeline.",
            "pipeline",
            "pipeline_id",
            &[
                "enable_scheduled_pipeline", "disable_scheduled_pipeline",
                "delete_scheduled_pipeline",
            ],
            &["agent.use"],
            RiskLevel::L3,
        ),
        _ => unreachable!("dashboard catalog received unrelated kind"),
    }
}

fn action_proposal(
    kind: BuiltinToolKind,
    description: &str,
    category: &str,
    target_field: &str,
    actions: &[&str],
    permissions: &[&str],
    risk: RiskLevel,
) -> ToolSpec {
    ToolSpec::read(
        kind.name(),
        description,
        "automation",
        category,
        json!({
            "type": "object",
            "required": ["action", target_field, "reason", "impact"],
            "properties": {
                "action": {"type": "string", "enum": actions},
                (target_field): {"type": "string"},
                "reason": {"type": "string"},
                "impact": {"type": "string"},
                "expires_at_micros": {"type": "integer"}
            },
            "additionalProperties": false
        }),
        open_output(),
        permissions,
        &["Approval", "Operations"],
    )
    .proposal(risk)
    .mole_agent_only()
}
