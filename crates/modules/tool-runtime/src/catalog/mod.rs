// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Compile-time registry for the platform tool catalog.
//!
//! The catalog only describes stable protocol contracts; the application layer supplies
//! executors. Tool names are public API, and both Mole Agent and the inbound MCP server must
//! read their definitions from this registry.

use serde_json::{Value, json};

use crate::{RiskLevel, ToolSpec, ToolSurface};

mod administration;
mod agent_control;
mod alerting;
mod apm;
mod content;
mod dashboard;
mod data_management;
mod incidents;
mod kind;
mod meta;
mod notifications;
mod observability;
mod platform;
mod profiles;
mod reports;
mod rum;
mod status_pages;
mod synthetics;
mod traces;

pub use kind::BuiltinToolKind;

impl BuiltinToolKind {
    pub fn spec(self) -> ToolSpec {
        match self {
            Self::ToolSearch | Self::ToolsCall => meta::spec(self),
            Self::GetPlatformCapabilities => platform::spec(self),
            Self::QueryLogs
            | Self::QueryMetrics
            | Self::ListStreams
            | Self::GetStreamSchema
            | Self::GetStreamSettings
            | Self::ListMetricNames
            | Self::ListMetricLabelValues
            | Self::ListMetricLabels
            | Self::ListMetricSeries
            | Self::SearchAround
            | Self::SearchFieldValues
            | Self::CorrelateSignals
            | Self::GetServiceTopology => observability::spec(self),
            Self::ListTraces
            | Self::GetTrace
            | Self::GetTraceDag
            | Self::ListTraceSessions
            | Self::GetTraceSession
            | Self::ListTraceUsers => traces::spec(self),
            Self::ListRecentAlerts
            | Self::ListIncidents
            | Self::GetIncident
            | Self::GetIncidentRca
            | Self::GetIncidentInsights
            | Self::AcknowledgeIncident
            | Self::ResolveIncident
            | Self::ListOnCallSchedules
            | Self::GetOnCallSchedule
            | Self::GetCurrentOnCall => incidents::spec(self),
            Self::ListAlertRules
            | Self::GetAlertRule
            | Self::CreateAlertRule
            | Self::UpdateAlertRule
            | Self::DeleteAlertRule
            | Self::TestAlertRule
            | Self::TriggerAlertRule
            | Self::ListIncidentGroups
            | Self::GetIncidentGroup
            | Self::ListMuteRules
            | Self::GetMuteRule
            | Self::ListEscalationPolicies
            | Self::GetEscalationPolicy => alerting::spec(self),
            Self::ListNotificationConnectors
            | Self::GetNotificationConnector
            | Self::ListNotificationPolicies
            | Self::GetNotificationPolicy
            | Self::ListNotificationTemplates
            | Self::GetNotificationTemplate
            | Self::ListNotificationDeliveries
            | Self::GetNotificationDelivery => notifications::spec(self),
            Self::RetryNotificationDelivery | Self::AcknowledgeNotificationDelivery => {
                notifications::spec(self)
            }
            Self::ApmOverview
            | Self::ListApmServices
            | Self::GetApmService
            | Self::ListApmTransactions
            | Self::GetApmTransaction
            | Self::ListApmDependencies
            | Self::ListApmErrors
            | Self::GetApmError
            | Self::CompareApmVersions
            | Self::GetApmHealth => apm::spec(self),
            Self::ListRumSessions
            | Self::GetRumSession
            | Self::ListRumActions
            | Self::ListRumErrors
            | Self::GetRumRelatedTraces => rum::spec(self),
            Self::ListContinuousProfiles | Self::GetProfileFlamegraph | Self::CompareProfiles => {
                profiles::spec(self)
            }
            Self::ListReportTemplates
            | Self::GetReportTemplate
            | Self::ListScheduledReports
            | Self::GetScheduledReport
            | Self::ListReportDeliveries => reports::spec(self),
            Self::ListDashboards
            | Self::GetDashboard
            | Self::CreateDashboard
            | Self::UpdateDashboard
            | Self::DeleteDashboard
            | Self::ListFolders
            | Self::GetFolder
            | Self::CreateFolder
            | Self::UpdateFolder
            | Self::DeleteFolder
            | Self::ListAnnotations
            | Self::GetAnnotation
            | Self::CreateAnnotation
            | Self::UpdateAnnotation
            | Self::DeleteAnnotation
            | Self::AddDashboardPanel
            | Self::UpdateDashboardPanel
            | Self::MoveDashboardPanel
            | Self::DeleteDashboardPanel => content::spec(self),
            Self::GetDashboardCapabilities
            | Self::PrepareDashboard
            | Self::ProposeDashboardCreation
            | Self::ProposeOperation
            | Self::ProposeAlertAction
            | Self::ProposeSyntheticMonitorAction
            | Self::ProposeStatusPageAction
            | Self::ProposeNotificationAction
            | Self::ProposeScheduledPipelineAction => dashboard::spec(self),
            Self::ListSavedViews
            | Self::GetSavedView
            | Self::CreateSavedView
            | Self::UpdateSavedView
            | Self::DeleteSavedView
            | Self::ListSearchJobs
            | Self::GetSearchJob
            | Self::GetSearchJobResults
            | Self::SubmitSearchJob
            | Self::CancelSearchJob
            | Self::RetrySearchJob
            | Self::DeleteSearchJob
            | Self::ListScheduledPipelines
            | Self::GetScheduledPipeline
            | Self::ListPipelineRuns
            | Self::EnableScheduledPipeline
            | Self::DisableScheduledPipeline
            | Self::DeleteScheduledPipeline
            | Self::ListFunctions
            | Self::GetFunction
            | Self::TestFunction
            | Self::CreateFunction
            | Self::UpdateFunction
            | Self::DeleteFunction
            | Self::ListEnrichmentTables
            | Self::ListEnrichmentRows
            | Self::GetEnrichmentValue
            | Self::ListLogPatterns
            | Self::GetLogPattern
            | Self::ListRegexPatterns
            | Self::GetRegexPattern
            | Self::ListFieldMaskingRules
            | Self::GetEffectiveFieldMasking
            | Self::ListDataConnectors
            | Self::GetDataConnector => data_management::spec(self),
            Self::ListSyntheticMonitors
            | Self::GetSyntheticMonitor
            | Self::ListSyntheticRevisions
            | Self::ListSyntheticResults
            | Self::ListSyntheticLocations
            | Self::ListSyntheticAgents
            | Self::ListSyntheticSecrets => synthetics::spec(self),
            Self::RunSyntheticMonitor
            | Self::PauseSyntheticMonitor
            | Self::ResumeSyntheticMonitor
            | Self::ArchiveSyntheticMonitor => synthetics::spec(self),
            Self::ListStatusPages
            | Self::GetStatusPage
            | Self::ListStatusPageIncidents
            | Self::GetStatusPageIncident
            | Self::ListStatusPageSubscribers
            | Self::ListStatusPageDeliveries
            | Self::ListStatusPageAutomationRules
            | Self::ListStatusPageAutomationCandidates
            | Self::GetStatusPageAutomationSettings => status_pages::spec(self),
            Self::ArchiveStatusPage
            | Self::RestoreStatusPage
            | Self::PauseStatusPageAutomation
            | Self::ResumeStatusPageAutomation => status_pages::spec(self),
            Self::GetUserProfile
            | Self::GetUserPreferences
            | Self::ListOrganizationMembers
            | Self::ListTeams
            | Self::ListRoles
            | Self::ListAuditEvents
            | Self::GetIamCapabilities
            | Self::ListServiceAccounts
            | Self::GetServiceAccount
            | Self::ListApiTokens
            | Self::CreateApiToken
            | Self::RevokeApiToken
            | Self::CreateServiceAccount
            | Self::UpdateServiceAccount
            | Self::EnableServiceAccount
            | Self::DisableServiceAccount
            | Self::DeleteServiceAccount => administration::spec(self),
            Self::ListAgentInvestigations
            | Self::GetAgentInvestigation
            | Self::ListAgentApprovals
            | Self::GetAgentApproval
            | Self::ExecuteAgentApproval
            | Self::ListAgentExecutions
            | Self::GetAgentExecution
            | Self::ListAgentAutomations => agent_control::spec(self),
        }
    }
}

pub fn builtin_tools() -> Vec<ToolSpec> {
    BuiltinToolKind::ALL
        .iter()
        .copied()
        .map(BuiltinToolKind::spec)
        .collect()
}

pub fn pinned_tools() -> Vec<ToolSpec> {
    builtin_tools()
        .into_iter()
        .filter(|tool| tool.exposure.pinned)
        .collect()
}

pub fn tools_for_surface(surface: ToolSurface) -> Vec<ToolSpec> {
    builtin_tools()
        .into_iter()
        .filter(|tool| tool.exposure.available_on(surface))
        .collect()
}

pub fn is_builtin_tool(name: &str) -> bool {
    BuiltinToolKind::from_name(name).is_some()
}

pub fn is_meta_tool(name: &str) -> bool {
    matches!(
        BuiltinToolKind::from_name(name),
        Some(BuiltinToolKind::ToolSearch | BuiltinToolKind::ToolsCall)
    )
}

pub fn risk_for_tool(name: &str) -> Option<RiskLevel> {
    BuiltinToolKind::from_name(name).map(|kind| kind.spec().risk)
}

pub(super) fn object_schema(properties: Value) -> Value {
    json!({"type": "object", "properties": properties, "additionalProperties": false})
}

pub(super) fn open_output() -> Value {
    json!({"type": "object", "additionalProperties": true})
}

pub(super) fn time_range_schema() -> Value {
    json!({
        "type": "object",
        "required": ["start_micros", "end_micros"],
        "properties": {
            "start_micros": {"type": "integer"},
            "end_micros": {"type": "integer"}
        },
        "additionalProperties": false
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::{ToolAccess, ToolExposure};

    #[test]
    fn catalog_names_are_complete_and_unique() {
        let tools = builtin_tools();
        let names = tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<HashSet<_>>();
        assert_eq!(tools.len(), BuiltinToolKind::ALL.len());
        assert_eq!(names.len(), tools.len());
    }

    #[test]
    fn catalog_metadata_is_canonical_and_not_localized() {
        fn contains_cjk(value: &str) -> bool {
            value.chars().any(|character| {
                matches!(
                    u32::from(character),
                    0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xf900..=0xfaff
                )
            })
        }

        for tool in builtin_tools() {
            assert!(
                !tool.display_name.trim().is_empty(),
                "{} has no display name",
                tool.name
            );
            assert!(
                !tool.description.trim().is_empty(),
                "{} has no description",
                tool.name
            );
            assert!(
                !contains_cjk(&tool.display_name),
                "{} has a localized display name in the protocol catalog",
                tool.name
            );
            assert!(
                !contains_cjk(&tool.description),
                "{} has a localized description in the protocol catalog",
                tool.name
            );
        }
    }

    #[test]
    fn schemas_are_objects_and_identity_is_never_an_argument() {
        for tool in builtin_tools() {
            assert_eq!(
                tool.input_schema.get("type").and_then(Value::as_str),
                Some("object")
            );
            let properties = tool
                .input_schema
                .get("properties")
                .and_then(Value::as_object);
            assert!(properties.is_none_or(|properties| {
                !properties.contains_key("org_id") && !properties.contains_key("user_id")
            }));
        }
    }

    #[test]
    fn every_required_input_is_declared_as_a_property() {
        for tool in builtin_tools() {
            let properties = tool
                .input_schema
                .get("properties")
                .and_then(Value::as_object)
                .expect("tool input schema must declare properties");
            let required = tool
                .input_schema
                .get("required")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str);
            for field in required {
                assert!(
                    properties.contains_key(field),
                    "tool {} requires undeclared input {}",
                    tool.name,
                    field
                );
            }
        }
    }

    #[test]
    fn any_permission_tools_have_alternatives() {
        for tool in builtin_tools()
            .into_iter()
            .filter(|tool| tool.permission_mode == crate::PermissionMode::Any)
        {
            assert!(
                tool.required_permissions.len() > 1,
                "tool {} uses any-permission mode without alternatives",
                tool.name
            );
        }
    }

    #[test]
    fn only_meta_tools_have_meta_exposure() {
        for tool in builtin_tools() {
            if is_meta_tool(&tool.name) {
                assert_eq!(tool.exposure, ToolExposure::META);
            }
        }
    }

    #[test]
    fn inbound_mcp_surface_excludes_adapter_meta_and_agent_authoring_tools() {
        let names = tools_for_surface(ToolSurface::InboundMcp)
            .into_iter()
            .map(|tool| tool.name)
            .collect::<HashSet<_>>();
        assert!(!names.contains("tool_search"));
        assert!(!names.contains("tools_call"));
        assert!(!names.contains("prepare_dashboard"));
        assert!(!names.contains("propose_operation"));
        assert!(!names.iter().any(|name| name.starts_with("propose_")));
        assert!(names.contains("create_annotation"));
        assert!(names.contains("execute_agent_approval"));
        assert!(
            !tools_for_surface(ToolSurface::MoleAgent)
                .iter()
                .any(|tool| tool.name == "execute_agent_approval")
        );
    }

    #[test]
    fn inbound_mcp_has_exactly_seven_pinned_read_tools() {
        let names = tools_for_surface(ToolSurface::InboundMcp)
            .into_iter()
            .filter(|tool| tool.exposure.pinned)
            .map(|tool| tool.name)
            .collect::<HashSet<_>>();
        assert_eq!(
            names,
            HashSet::from([
                "query_logs".into(),
                "query_metrics".into(),
                "list_streams".into(),
                "get_stream_schema".into(),
                "list_traces".into(),
                "get_trace".into(),
                "get_incident".into(),
            ])
        );
    }

    #[test]
    fn every_safe_atomic_mutation_is_exposed_to_inbound_mcp() {
        for tool in builtin_tools().into_iter().filter(|tool| {
            matches!(
                tool.access,
                ToolAccess::ManagedMutation | ToolAccess::ExecutesApprovedOperation
            )
        }) {
            if matches!(
                tool.name.as_str(),
                "create_api_token" | "create_service_account"
            ) {
                assert!(!tool.exposure.inbound_mcp, "{}", tool.name);
            } else {
                assert!(tool.exposure.inbound_mcp, "{}", tool.name);
            }
        }
    }

    #[test]
    fn proposal_tools_never_claim_direct_execution() {
        for tool in builtin_tools()
            .into_iter()
            .filter(|tool| matches!(tool.access, ToolAccess::CreatesApprovalRequest))
        {
            assert!(!tool.annotations.destructive);
            assert!(tool.annotations.supports_dry_run);
        }
    }

    #[test]
    fn managed_mutations_are_explicitly_non_read_only() {
        for tool in builtin_tools()
            .into_iter()
            .filter(|tool| matches!(tool.access, ToolAccess::ManagedMutation))
        {
            assert!(!tool.annotations.read_only, "{}", tool.name);
            assert!(tool.annotations.supports_dry_run, "{}", tool.name);
        }
    }

    #[test]
    fn managed_mutations_always_collect_approval_context() {
        for tool in builtin_tools()
            .into_iter()
            .filter(|tool| tool.access == ToolAccess::ManagedMutation)
        {
            let properties = tool.input_schema["properties"]
                .as_object()
                .expect("managed mutation properties");
            let required = tool.input_schema["required"]
                .as_array()
                .expect("managed mutation required fields");
            for field in ["reason", "impact"] {
                assert!(
                    properties.contains_key(field),
                    "{} lacks {field}",
                    tool.name
                );
                assert!(
                    required.iter().any(|value| value == field),
                    "{} does not require {field}",
                    tool.name
                );
            }
        }
    }
}
