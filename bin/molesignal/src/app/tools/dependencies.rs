// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Tool Runtime 的应用依赖集合。
//!
//! 这里按产品域分组，避免 composition root 与 runtime 构造函数演变成无语义的长参数列。

use std::sync::Arc;

use object_store::ObjectStore;

use crate::{
    agent::model::AgentRepository,
    app::{
        alerting::{AlertingService, rule_evaluator::RuleEvaluator},
        apm::{ApmQueryService, ApmRuntime},
        dashboard::{DashboardService, authoring::DashboardAuthoringService},
        iam::{IamAccessService, IamService},
        intake::FunctionExecutor,
        notify::{NotifyEngine, NotifyService},
        query::{QueryService, jobs::SearchJobService},
        status_page::StatusPageService,
        synthetics::SyntheticService,
    },
    domain::{
        alerting::{
            incident_group::IncidentGroupRepository, mute::MuteRuleRepository,
            repositories::IncidentRcaRepository,
        },
        function::FunctionRepository,
        iam::{
            TeamRepository, api_token::ApiTokenRepository,
            service_account::ServiceAccountRepository,
        },
        masking::{FieldMaskingProvider, FieldMaskingRuleRepository},
        query::SlowQueryRepository,
        saved_view::SavedViewRepository,
        storage::QueryFileSource,
        stream::StreamRepository,
    },
    infra::{
        connectors::ConnectorRepository,
        persistence::repositories::{
            annotations::AnnotationRepository, audit_events::AuditEventRepository,
            iam::roles::IamRoleRepository, log_patterns::LogPatternRepository,
            notify::NotifyTemplateManagementRepository, pipelines::runs::PipelineRunRepository,
            regex_patterns::RegexPatternRepository, report_templates::ReportTemplateRepository,
            scheduled_reports::ScheduledReportRepository,
            user_preferences::UserPreferencesRepository,
        },
        pipeline::{ExtendKvRepository, ScheduledPipelineRepository},
        query::catalog_source::CatalogQuerySource,
        traces::ServiceGraphRepository,
    },
    shared::LicenseGate,
};

#[derive(Clone)]
pub struct ToolRuntimeDependencies {
    pub observability: ObservabilityToolDependencies,
    pub alerting: AlertingToolDependencies,
    pub content: ContentToolDependencies,
    pub data: DataToolDependencies,
    pub synthetics: Arc<SyntheticService>,
    pub status_pages: Arc<StatusPageService>,
    pub administration: AdministrationToolDependencies,
    pub agent: AgentToolDependencies,
    pub license: Arc<dyn LicenseGate>,
}

#[derive(Clone)]
pub struct ObservabilityToolDependencies {
    pub query: Arc<QueryService>,
    pub streams: Arc<dyn StreamRepository>,
    pub apm: Arc<ApmQueryService>,
    pub apm_runtime: Option<Arc<ApmRuntime>>,
    pub service_graph: Arc<dyn ServiceGraphRepository>,
    pub catalog_files: Arc<dyn QueryFileSource>,
    pub catalog_query: Arc<CatalogQuerySource>,
    pub object_store: Arc<dyn ObjectStore>,
    pub slow_queries: Arc<dyn SlowQueryRepository>,
}

#[derive(Clone)]
pub struct AlertingToolDependencies {
    pub service: Arc<AlertingService>,
    pub evaluator: Arc<RuleEvaluator>,
    pub incident_rca: Arc<dyn IncidentRcaRepository>,
    pub incident_groups: Arc<dyn IncidentGroupRepository>,
    pub mute_rules: Arc<dyn MuteRuleRepository>,
    pub notify: Arc<NotifyService>,
    pub notify_engine: Arc<NotifyEngine>,
    pub notify_templates: Arc<dyn NotifyTemplateManagementRepository>,
}

#[derive(Clone)]
pub struct ContentToolDependencies {
    pub dashboard: Arc<DashboardService>,
    pub dashboard_authoring: Arc<DashboardAuthoringService>,
    pub report_templates: Arc<dyn ReportTemplateRepository>,
    pub scheduled_reports: Arc<dyn ScheduledReportRepository>,
    pub annotations: Arc<dyn AnnotationRepository>,
}

#[derive(Clone)]
pub struct DataToolDependencies {
    pub saved_views: Arc<dyn SavedViewRepository>,
    pub search_jobs: Arc<SearchJobService>,
    pub scheduled_pipelines: Arc<dyn ScheduledPipelineRepository>,
    pub pipeline_runs: Arc<dyn PipelineRunRepository>,
    pub functions: Arc<dyn FunctionRepository>,
    pub function_executor: Arc<dyn FunctionExecutor>,
    pub functions_js_runtime_enabled: bool,
    pub enrichment: Arc<dyn ExtendKvRepository>,
    pub log_patterns: Arc<dyn LogPatternRepository>,
    pub regex_patterns: Arc<dyn RegexPatternRepository>,
    pub field_masking_rules: Arc<dyn FieldMaskingRuleRepository>,
    pub field_masking: Arc<dyn FieldMaskingProvider>,
    pub connectors: Arc<dyn ConnectorRepository>,
}

#[derive(Clone)]
pub struct AdministrationToolDependencies {
    pub iam: Arc<IamService>,
    pub iam_access: Arc<IamAccessService>,
    pub teams: Arc<dyn TeamRepository>,
    pub roles: Arc<dyn IamRoleRepository>,
    pub audit_events: Arc<dyn AuditEventRepository>,
    pub user_preferences: Arc<dyn UserPreferencesRepository>,
    pub service_accounts: Arc<dyn ServiceAccountRepository>,
    pub api_tokens: Arc<dyn ApiTokenRepository>,
}

#[derive(Clone)]
pub struct AgentToolDependencies {
    pub repository: Arc<dyn AgentRepository>,
}
