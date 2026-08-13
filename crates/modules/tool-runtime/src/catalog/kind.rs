// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

macro_rules! builtin_tool_kinds {
    ($( $variant:ident => $name:literal ),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum BuiltinToolKind {
            $( $variant, )+
        }

        impl BuiltinToolKind {
            pub const ALL: &'static [Self] = &[
                $( Self::$variant, )+
            ];

            pub const fn name(self) -> &'static str {
                match self {
                    $( Self::$variant => $name, )+
                }
            }

            pub fn from_name(name: &str) -> Option<Self> {
                Self::ALL
                    .iter()
                    .copied()
                    .find(|kind| kind.name() == name)
            }
        }
    };
}

builtin_tool_kinds! {
    ToolSearch => "tool_search",
    ToolsCall => "tools_call",
    GetPlatformCapabilities => "get_platform_capabilities",

    QueryLogs => "query_logs",
    QueryMetrics => "query_metrics",
    ListStreams => "list_streams",
    GetStreamSchema => "get_stream_schema",
    GetStreamSettings => "get_stream_settings",
    ListMetricNames => "list_metric_names",
    ListMetricLabelValues => "list_metric_label_values",
    ListMetricLabels => "list_metric_labels",
    ListMetricSeries => "list_metric_series",
    SearchAround => "search_around",
    SearchFieldValues => "search_field_values",

    ListTraces => "list_traces",
    GetTrace => "get_trace",
    GetTraceDag => "get_trace_dag",
    ListTraceSessions => "list_trace_sessions",
    GetTraceSession => "get_trace_session",
    ListTraceUsers => "list_trace_users",

    ListRecentAlerts => "list_recent_alerts",
    ListIncidents => "list_incidents",
    GetIncident => "get_incident",
    GetIncidentRca => "get_incident_rca",
    GetIncidentInsights => "get_incident_insights",
    AcknowledgeIncident => "acknowledge_incident",
    ResolveIncident => "resolve_incident",
    ListAlertRules => "list_alert_rules",
    GetAlertRule => "get_alert_rule",
    CreateAlertRule => "create_alert_rule",
    UpdateAlertRule => "update_alert_rule",
    DeleteAlertRule => "delete_alert_rule",
    TestAlertRule => "test_alert_rule",
    TriggerAlertRule => "trigger_alert_rule",
    ListIncidentGroups => "list_incident_groups",
    GetIncidentGroup => "get_incident_group",
    ListMuteRules => "list_mute_rules",
    GetMuteRule => "get_mute_rule",
    ListEscalationPolicies => "list_escalation_policies",
    GetEscalationPolicy => "get_escalation_policy",
    ListOnCallSchedules => "list_on_call_schedules",
    GetOnCallSchedule => "get_on_call_schedule",
    GetCurrentOnCall => "get_current_on_call",

    ListNotificationConnectors => "list_notification_connectors",
    GetNotificationConnector => "get_notification_connector",
    ListNotificationPolicies => "list_notification_policies",
    GetNotificationPolicy => "get_notification_policy",
    ListNotificationTemplates => "list_notification_templates",
    GetNotificationTemplate => "get_notification_template",
    ListNotificationDeliveries => "list_notification_deliveries",
    GetNotificationDelivery => "get_notification_delivery",
    RetryNotificationDelivery => "retry_notification_delivery",
    AcknowledgeNotificationDelivery => "acknowledge_notification_delivery",

    ApmOverview => "apm_overview",
    ListApmServices => "list_apm_services",
    GetApmService => "get_apm_service",
    ListApmTransactions => "list_apm_transactions",
    GetApmTransaction => "get_apm_transaction",
    ListApmDependencies => "list_apm_dependencies",
    ListApmErrors => "list_apm_errors",
    GetApmError => "get_apm_error",
    CompareApmVersions => "compare_apm_versions",
    GetApmHealth => "get_apm_health",
    CorrelateSignals => "correlate_signals",
    GetServiceTopology => "get_service_topology",

    ListRumSessions => "list_rum_sessions",
    GetRumSession => "get_rum_session",
    ListRumActions => "list_rum_actions",
    ListRumErrors => "list_rum_errors",
    GetRumRelatedTraces => "get_rum_related_traces",

    ListContinuousProfiles => "list_continuous_profiles",
    GetProfileFlamegraph => "get_profile_flamegraph",
    CompareProfiles => "compare_profiles",

    ListReportTemplates => "list_report_templates",
    GetReportTemplate => "get_report_template",
    ListScheduledReports => "list_scheduled_reports",
    GetScheduledReport => "get_scheduled_report",
    ListReportDeliveries => "list_report_deliveries",
    ListDashboards => "list_dashboards",
    GetDashboard => "get_dashboard",
    CreateDashboard => "create_dashboard",
    UpdateDashboard => "update_dashboard",
    DeleteDashboard => "delete_dashboard",
    ListFolders => "list_folders",
    GetFolder => "get_folder",
    CreateFolder => "create_folder",
    UpdateFolder => "update_folder",
    DeleteFolder => "delete_folder",
    ListAnnotations => "list_annotations",
    GetAnnotation => "get_annotation",
    CreateAnnotation => "create_annotation",
    UpdateAnnotation => "update_annotation",
    DeleteAnnotation => "delete_annotation",
    AddDashboardPanel => "add_dashboard_panel",
    UpdateDashboardPanel => "update_dashboard_panel",
    MoveDashboardPanel => "move_dashboard_panel",
    DeleteDashboardPanel => "delete_dashboard_panel",
    GetDashboardCapabilities => "get_dashboard_capabilities",
    PrepareDashboard => "prepare_dashboard",
    ProposeDashboardCreation => "propose_dashboard_creation",
    ProposeOperation => "propose_operation",
    ProposeAlertAction => "propose_alert_action",
    ProposeSyntheticMonitorAction => "propose_synthetic_monitor_action",
    ProposeStatusPageAction => "propose_status_page_action",
    ProposeNotificationAction => "propose_notification_action",
    ProposeScheduledPipelineAction => "propose_scheduled_pipeline_action",

    ListSavedViews => "list_saved_views",
    GetSavedView => "get_saved_view",
    CreateSavedView => "create_saved_view",
    UpdateSavedView => "update_saved_view",
    DeleteSavedView => "delete_saved_view",
    ListSearchJobs => "list_search_jobs",
    GetSearchJob => "get_search_job",
    GetSearchJobResults => "get_search_job_results",
    SubmitSearchJob => "submit_search_job",
    CancelSearchJob => "cancel_search_job",
    RetrySearchJob => "retry_search_job",
    DeleteSearchJob => "delete_search_job",
    ListScheduledPipelines => "list_scheduled_pipelines",
    GetScheduledPipeline => "get_scheduled_pipeline",
    ListPipelineRuns => "list_pipeline_runs",
    EnableScheduledPipeline => "enable_scheduled_pipeline",
    DisableScheduledPipeline => "disable_scheduled_pipeline",
    DeleteScheduledPipeline => "delete_scheduled_pipeline",
    ListFunctions => "list_functions",
    GetFunction => "get_function",
    TestFunction => "test_function",
    CreateFunction => "create_function",
    UpdateFunction => "update_function",
    DeleteFunction => "delete_function",
    ListEnrichmentTables => "list_enrichment_tables",
    ListEnrichmentRows => "list_enrichment_rows",
    GetEnrichmentValue => "get_enrichment_value",
    ListLogPatterns => "list_log_patterns",
    GetLogPattern => "get_log_pattern",
    ListRegexPatterns => "list_regex_patterns",
    GetRegexPattern => "get_regex_pattern",
    ListFieldMaskingRules => "list_field_masking_rules",
    GetEffectiveFieldMasking => "get_effective_field_masking",
    ListDataConnectors => "list_data_connectors",
    GetDataConnector => "get_data_connector",

    ListSyntheticMonitors => "list_synthetic_monitors",
    GetSyntheticMonitor => "get_synthetic_monitor",
    ListSyntheticRevisions => "list_synthetic_revisions",
    ListSyntheticResults => "list_synthetic_results",
    ListSyntheticLocations => "list_synthetic_locations",
    ListSyntheticAgents => "list_synthetic_agents",
    ListSyntheticSecrets => "list_synthetic_secrets",
    RunSyntheticMonitor => "run_synthetic_monitor",
    PauseSyntheticMonitor => "pause_synthetic_monitor",
    ResumeSyntheticMonitor => "resume_synthetic_monitor",
    ArchiveSyntheticMonitor => "archive_synthetic_monitor",

    ListStatusPages => "list_status_pages",
    GetStatusPage => "get_status_page",
    ListStatusPageIncidents => "list_status_page_incidents",
    GetStatusPageIncident => "get_status_page_incident",
    ListStatusPageSubscribers => "list_status_page_subscribers",
    ListStatusPageDeliveries => "list_status_page_deliveries",
    ListStatusPageAutomationRules => "list_status_page_automation_rules",
    ListStatusPageAutomationCandidates => "list_status_page_automation_candidates",
    GetStatusPageAutomationSettings => "get_status_page_automation_settings",
    ArchiveStatusPage => "archive_status_page",
    RestoreStatusPage => "restore_status_page",
    PauseStatusPageAutomation => "pause_status_page_automation",
    ResumeStatusPageAutomation => "resume_status_page_automation",

    GetUserProfile => "get_user_profile",
    GetUserPreferences => "get_user_preferences",
    ListOrganizationMembers => "list_organization_members",
    ListTeams => "list_teams",
    ListRoles => "list_roles",
    ListAuditEvents => "list_audit_events",
    GetIamCapabilities => "get_iam_capabilities",
    ListServiceAccounts => "list_service_accounts",
    GetServiceAccount => "get_service_account",
    ListApiTokens => "list_api_tokens",
    CreateApiToken => "create_api_token",
    RevokeApiToken => "revoke_api_token",
    CreateServiceAccount => "create_service_account",
    UpdateServiceAccount => "update_service_account",
    EnableServiceAccount => "enable_service_account",
    DisableServiceAccount => "disable_service_account",
    DeleteServiceAccount => "delete_service_account",

    ListAgentInvestigations => "list_agent_investigations",
    GetAgentInvestigation => "get_agent_investigation",
    ListAgentApprovals => "list_agent_approvals",
    GetAgentApproval => "get_agent_approval",
    ExecuteAgentApproval => "execute_agent_approval",
    ListAgentExecutions => "list_agent_executions",
    GetAgentExecution => "get_agent_execution",
    ListAgentAutomations => "list_agent_automations",
}
