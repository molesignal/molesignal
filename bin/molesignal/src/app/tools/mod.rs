// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 平台 Tool Runtime。
//!
//! 这里是协议中立的应用层入口：Mole Agent 和未来的入站 MCP Server 都把可信身份
//! 上下文交给同一个 runtime。HTTP、SSE、MCP JSON-RPC 和模型供应商格式不得进入本层。

use std::sync::Arc;

use tool_runtime::{
    PermissionMode, ToolAccess, ToolExecutionMode, ToolInvocationContext, ToolResult,
    catalog::BuiltinToolKind,
};

use crate::{
    app::iam::IamContext,
    shared::{Error, Result},
};

mod administration;
mod agent_control;
mod alerting;
mod apm;
mod common;
mod content;
mod correlation;
pub(crate) mod dashboard;
mod data_management;
pub mod dependencies;
mod incidents;
mod mutations;
mod notifications;
mod observability;
mod platform;
mod profiles;
pub(crate) mod reports;
mod rum;
mod status_pages;
mod synthetics;
mod traces;

pub use dependencies::{
    AdministrationToolDependencies, AgentToolDependencies, AlertingToolDependencies,
    ContentToolDependencies, DataToolDependencies, ObservabilityToolDependencies,
    ToolRuntimeDependencies,
};

const HARD_RESULT_BYTES: usize = 8 * 1_048_576;

#[derive(Clone)]
pub struct ToolRuntime {
    pub(super) observability: ObservabilityToolDependencies,
    pub(super) alerting: AlertingToolDependencies,
    pub(super) content: ContentToolDependencies,
    pub(super) data: DataToolDependencies,
    pub(super) synthetics: Arc<crate::app::synthetics::SyntheticService>,
    pub(super) status_pages: Arc<crate::app::status_page::StatusPageService>,
    pub(super) administration: AdministrationToolDependencies,
    pub(super) agent: AgentToolDependencies,
    pub(super) license: Arc<dyn crate::shared::LicenseGate>,
}

impl ToolRuntime {
    pub fn new(dependencies: ToolRuntimeDependencies) -> Self {
        Self {
            observability: dependencies.observability,
            alerting: dependencies.alerting,
            content: dependencies.content,
            data: dependencies.data,
            synthetics: dependencies.synthetics,
            status_pages: dependencies.status_pages,
            administration: dependencies.administration,
            agent: dependencies.agent,
            license: dependencies.license,
        }
    }

    pub async fn execute(
        &self,
        ctx: &ToolInvocationContext,
        kind: BuiltinToolKind,
        arguments: serde_json::Value,
    ) -> Result<ToolResult> {
        self.authorize(ctx, kind)?;
        if matches!(
            kind,
            BuiltinToolKind::ToolSearch | BuiltinToolKind::ToolsCall
        ) {
            return Ok(ToolResult::error(
                "meta tools must be handled by the entry adapter",
            ));
        }

        let auth = ctx.iam_context();

        let result = match kind {
            BuiltinToolKind::GetPlatformCapabilities => {
                platform::execute(self, auth, ctx, kind, arguments).await
            }
            BuiltinToolKind::QueryLogs
            | BuiltinToolKind::QueryMetrics
            | BuiltinToolKind::ListStreams
            | BuiltinToolKind::GetStreamSchema
            | BuiltinToolKind::GetStreamSettings
            | BuiltinToolKind::ListMetricNames
            | BuiltinToolKind::ListMetricLabelValues
            | BuiltinToolKind::ListMetricLabels
            | BuiltinToolKind::ListMetricSeries
            | BuiltinToolKind::SearchAround
            | BuiltinToolKind::SearchFieldValues => {
                observability::execute(self, auth, kind, arguments).await
            }
            BuiltinToolKind::ListTraces
            | BuiltinToolKind::GetTrace
            | BuiltinToolKind::GetTraceDag
            | BuiltinToolKind::ListTraceSessions
            | BuiltinToolKind::GetTraceSession
            | BuiltinToolKind::ListTraceUsers => traces::execute(self, auth, kind, arguments).await,
            BuiltinToolKind::ListRecentAlerts
            | BuiltinToolKind::ListIncidents
            | BuiltinToolKind::GetIncident
            | BuiltinToolKind::GetIncidentRca
            | BuiltinToolKind::GetIncidentInsights
            | BuiltinToolKind::ListOnCallSchedules
            | BuiltinToolKind::GetOnCallSchedule
            | BuiltinToolKind::GetCurrentOnCall => {
                incidents::execute(self, auth, kind, arguments).await
            }
            BuiltinToolKind::ListAlertRules
            | BuiltinToolKind::GetAlertRule
            | BuiltinToolKind::TestAlertRule
            | BuiltinToolKind::ListIncidentGroups
            | BuiltinToolKind::GetIncidentGroup
            | BuiltinToolKind::ListMuteRules
            | BuiltinToolKind::GetMuteRule
            | BuiltinToolKind::ListEscalationPolicies
            | BuiltinToolKind::GetEscalationPolicy => {
                alerting::execute(self, auth, kind, arguments).await
            }
            BuiltinToolKind::ListNotificationConnectors
            | BuiltinToolKind::GetNotificationConnector
            | BuiltinToolKind::ListNotificationPolicies
            | BuiltinToolKind::GetNotificationPolicy
            | BuiltinToolKind::ListNotificationTemplates
            | BuiltinToolKind::GetNotificationTemplate
            | BuiltinToolKind::ListNotificationDeliveries
            | BuiltinToolKind::GetNotificationDelivery => {
                notifications::execute(self, auth, kind, arguments).await
            }
            BuiltinToolKind::ApmOverview
            | BuiltinToolKind::ListApmServices
            | BuiltinToolKind::GetApmService
            | BuiltinToolKind::ListApmTransactions
            | BuiltinToolKind::GetApmTransaction
            | BuiltinToolKind::ListApmDependencies
            | BuiltinToolKind::ListApmErrors
            | BuiltinToolKind::GetApmError
            | BuiltinToolKind::CompareApmVersions
            | BuiltinToolKind::GetApmHealth => apm::execute(self, auth, kind, arguments).await,
            BuiltinToolKind::CorrelateSignals | BuiltinToolKind::GetServiceTopology => {
                correlation::execute(self, auth, kind, arguments).await
            }
            BuiltinToolKind::ListRumSessions
            | BuiltinToolKind::GetRumSession
            | BuiltinToolKind::ListRumActions
            | BuiltinToolKind::ListRumErrors
            | BuiltinToolKind::GetRumRelatedTraces => {
                rum::execute(self, auth, kind, arguments).await
            }
            BuiltinToolKind::ListContinuousProfiles
            | BuiltinToolKind::GetProfileFlamegraph
            | BuiltinToolKind::CompareProfiles => {
                profiles::execute(self, auth, kind, arguments).await
            }
            BuiltinToolKind::ListReportTemplates
            | BuiltinToolKind::GetReportTemplate
            | BuiltinToolKind::ListScheduledReports
            | BuiltinToolKind::GetScheduledReport
            | BuiltinToolKind::ListReportDeliveries => {
                reports::execute(self, auth, kind, arguments).await
            }
            BuiltinToolKind::ListDashboards
            | BuiltinToolKind::GetDashboard
            | BuiltinToolKind::ListFolders
            | BuiltinToolKind::GetFolder
            | BuiltinToolKind::ListAnnotations
            | BuiltinToolKind::GetAnnotation => content::execute(self, auth, kind, arguments).await,
            BuiltinToolKind::GetDashboardCapabilities
            | BuiltinToolKind::PrepareDashboard
            | BuiltinToolKind::ProposeDashboardCreation
            | BuiltinToolKind::ProposeOperation
            | BuiltinToolKind::ProposeAlertAction
            | BuiltinToolKind::ProposeSyntheticMonitorAction
            | BuiltinToolKind::ProposeStatusPageAction
            | BuiltinToolKind::ProposeNotificationAction
            | BuiltinToolKind::ProposeScheduledPipelineAction => {
                dashboard::execute(self, auth, ctx, kind, arguments).await
            }
            BuiltinToolKind::ListSavedViews
            | BuiltinToolKind::GetSavedView
            | BuiltinToolKind::ListSearchJobs
            | BuiltinToolKind::GetSearchJob
            | BuiltinToolKind::GetSearchJobResults
            | BuiltinToolKind::ListScheduledPipelines
            | BuiltinToolKind::GetScheduledPipeline
            | BuiltinToolKind::ListPipelineRuns
            | BuiltinToolKind::ListFunctions
            | BuiltinToolKind::GetFunction
            | BuiltinToolKind::TestFunction
            | BuiltinToolKind::ListEnrichmentTables
            | BuiltinToolKind::ListEnrichmentRows
            | BuiltinToolKind::GetEnrichmentValue
            | BuiltinToolKind::ListLogPatterns
            | BuiltinToolKind::GetLogPattern
            | BuiltinToolKind::ListRegexPatterns
            | BuiltinToolKind::GetRegexPattern
            | BuiltinToolKind::ListFieldMaskingRules
            | BuiltinToolKind::GetEffectiveFieldMasking
            | BuiltinToolKind::ListDataConnectors
            | BuiltinToolKind::GetDataConnector => {
                data_management::execute(self, auth, kind, arguments).await
            }
            BuiltinToolKind::ListSyntheticMonitors
            | BuiltinToolKind::GetSyntheticMonitor
            | BuiltinToolKind::ListSyntheticRevisions
            | BuiltinToolKind::ListSyntheticResults
            | BuiltinToolKind::ListSyntheticLocations
            | BuiltinToolKind::ListSyntheticAgents
            | BuiltinToolKind::ListSyntheticSecrets => {
                synthetics::execute(self, auth, kind, arguments).await
            }
            BuiltinToolKind::ListStatusPages
            | BuiltinToolKind::GetStatusPage
            | BuiltinToolKind::ListStatusPageIncidents
            | BuiltinToolKind::GetStatusPageIncident
            | BuiltinToolKind::ListStatusPageSubscribers
            | BuiltinToolKind::ListStatusPageDeliveries
            | BuiltinToolKind::ListStatusPageAutomationRules
            | BuiltinToolKind::ListStatusPageAutomationCandidates
            | BuiltinToolKind::GetStatusPageAutomationSettings => {
                status_pages::execute(self, auth, kind, arguments).await
            }
            BuiltinToolKind::GetUserProfile
            | BuiltinToolKind::GetUserPreferences
            | BuiltinToolKind::ListOrganizationMembers
            | BuiltinToolKind::ListTeams
            | BuiltinToolKind::ListRoles
            | BuiltinToolKind::ListAuditEvents
            | BuiltinToolKind::GetIamCapabilities => {
                administration::execute(self, auth, kind, arguments).await
            }
            BuiltinToolKind::ListServiceAccounts
            | BuiltinToolKind::GetServiceAccount
            | BuiltinToolKind::ListApiTokens => {
                administration::execute(self, auth, kind, arguments).await
            }
            BuiltinToolKind::TriggerAlertRule
            | BuiltinToolKind::AcknowledgeIncident
            | BuiltinToolKind::ResolveIncident
            | BuiltinToolKind::CreateAlertRule
            | BuiltinToolKind::UpdateAlertRule
            | BuiltinToolKind::DeleteAlertRule
            | BuiltinToolKind::CreateDashboard
            | BuiltinToolKind::UpdateDashboard
            | BuiltinToolKind::DeleteDashboard
            | BuiltinToolKind::CreateFolder
            | BuiltinToolKind::UpdateFolder
            | BuiltinToolKind::DeleteFolder
            | BuiltinToolKind::CreateAnnotation
            | BuiltinToolKind::UpdateAnnotation
            | BuiltinToolKind::DeleteAnnotation
            | BuiltinToolKind::AddDashboardPanel
            | BuiltinToolKind::UpdateDashboardPanel
            | BuiltinToolKind::MoveDashboardPanel
            | BuiltinToolKind::DeleteDashboardPanel
            | BuiltinToolKind::SubmitSearchJob
            | BuiltinToolKind::CancelSearchJob
            | BuiltinToolKind::RetrySearchJob
            | BuiltinToolKind::DeleteSearchJob
            | BuiltinToolKind::CreateSavedView
            | BuiltinToolKind::UpdateSavedView
            | BuiltinToolKind::DeleteSavedView
            | BuiltinToolKind::CreateFunction
            | BuiltinToolKind::UpdateFunction
            | BuiltinToolKind::DeleteFunction
            | BuiltinToolKind::EnableScheduledPipeline
            | BuiltinToolKind::DisableScheduledPipeline
            | BuiltinToolKind::DeleteScheduledPipeline
            | BuiltinToolKind::RunSyntheticMonitor
            | BuiltinToolKind::PauseSyntheticMonitor
            | BuiltinToolKind::ResumeSyntheticMonitor
            | BuiltinToolKind::ArchiveSyntheticMonitor
            | BuiltinToolKind::ArchiveStatusPage
            | BuiltinToolKind::RestoreStatusPage
            | BuiltinToolKind::PauseStatusPageAutomation
            | BuiltinToolKind::ResumeStatusPageAutomation
            | BuiltinToolKind::RetryNotificationDelivery
            | BuiltinToolKind::AcknowledgeNotificationDelivery
            | BuiltinToolKind::CreateApiToken
            | BuiltinToolKind::RevokeApiToken
            | BuiltinToolKind::CreateServiceAccount
            | BuiltinToolKind::UpdateServiceAccount
            | BuiltinToolKind::EnableServiceAccount
            | BuiltinToolKind::DisableServiceAccount
            | BuiltinToolKind::DeleteServiceAccount => {
                mutations::execute(self, auth, ctx, kind, arguments).await
            }
            BuiltinToolKind::ListAgentInvestigations
            | BuiltinToolKind::GetAgentInvestigation
            | BuiltinToolKind::ListAgentApprovals
            | BuiltinToolKind::GetAgentApproval
            | BuiltinToolKind::ListAgentExecutions
            | BuiltinToolKind::GetAgentExecution
            | BuiltinToolKind::ListAgentAutomations => {
                agent_control::execute(self, auth, kind, arguments).await
            }
            BuiltinToolKind::ExecuteAgentApproval => unreachable!(
                "approved execution must be handled by the entry operation coordinator"
            ),
            BuiltinToolKind::ToolSearch | BuiltinToolKind::ToolsCall => unreachable!(),
        }?;
        if result.serialized_len() > HARD_RESULT_BYTES {
            Ok(ToolResult::error(format!(
                "tool `{}` response exceeded the platform hard limit",
                kind.name()
            )))
        } else {
            Ok(result)
        }
    }

    /// Apply protocol-neutral license, mode, and IAM checks before an adapter-specific action.
    pub fn authorize(&self, ctx: &ToolInvocationContext, kind: BuiltinToolKind) -> Result<()> {
        if ctx.is_query_generation_only() {
            return Err(Error::forbidden(
                "tool calls are disabled while query-generation-only mode is active",
            ));
        }
        if !self.license.has_feature(crate::agent::FEATURE) {
            return Err(Error::forbidden(
                "Mole Agent tools require the agent feature",
            ));
        }
        let spec = kind.spec();
        if !spec.exposure.available_on(ctx.source().surface()) {
            return Err(Error::forbidden(format!(
                "tool `{}` is not exposed on the `{}` surface",
                kind.name(),
                ctx.source().as_str()
            )));
        }
        if ctx.execution_mode() == ToolExecutionMode::Disabled {
            return Err(Error::forbidden(format!(
                "tool `{}` is disabled by the active Tool Policy",
                kind.name()
            )));
        }
        if !ctx.execution_mode().allowed_for_risk(spec.risk) {
            return Err(Error::forbidden(format!(
                "execution mode is below the minimum risk policy for tool `{}`",
                kind.name()
            )));
        }
        if matches!(spec.access, ToolAccess::ReadOnly | ToolAccess::Preflight)
            && ctx.execution_mode() != ToolExecutionMode::Automatic
        {
            return Err(Error::forbidden(format!(
                "tool `{}` cannot run in a non-automatic mode",
                kind.name()
            )));
        }
        if spec.access == ToolAccess::ExecutesApprovedOperation
            && matches!(
                ctx.execution_mode(),
                ToolExecutionMode::SingleApproval | ToolExecutionMode::DualApproval
            )
        {
            return Err(Error::forbidden(
                "execute_agent_approval cannot itself require another approval",
            ));
        }
        if matches!(
            spec.access,
            ToolAccess::ManagedMutation
                | ToolAccess::CreatesApprovalRequest
                | ToolAccess::ExecutesApprovedOperation
        ) && !ctx.execution_policy().allows_approval_request()
        {
            return Err(Error::forbidden(
                "the current execution policy does not allow managed mutations",
            ));
        }
        authorize(kind, ctx.iam_context())
    }
}

fn authorize(kind: BuiltinToolKind, auth: &IamContext) -> Result<()> {
    let has = |key: &str| auth.permissions.contains(key);
    let telemetry = || has("streams.query") || has("sys.telemetry.read");
    let spec = kind.spec();
    let allowed = if matches!(kind, BuiltinToolKind::PrepareDashboard) {
        has("agent.use") && has("dashboards.create") && telemetry()
    } else {
        match spec.permission_mode {
            PermissionMode::All => spec.required_permissions.iter().all(|key| has(key)),
            PermissionMode::Any => spec.required_permissions.iter().any(|key| has(key)),
        }
    };
    if allowed {
        Ok(())
    } else {
        Err(Error::forbidden(format!(
            "missing permission for tool `{}`",
            kind.name()
        )))
    }
}
