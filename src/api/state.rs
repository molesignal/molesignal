// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::Arc;

use object_store::ObjectStore;

use crate::{
    app::{
        alerting::AlertingService,
        cluster::ClusterRegistry,
        dashboard::{DashboardService, authoring::DashboardAuthoringService},
        iam::{IamAccessService, IamService},
        ingestion::IngestService,
        notify::{NotifyEngine, NotifyService},
        profile_storage::ProfileStorageService,
        profiling::ProfilingService,
        query::QueryService,
        self_telemetry::SelfTelemetryRuntime,
        trace::{TracePipeline, candidate_router::TraceCandidateRouter},
    },
    domain::{
        alerting::{
            incident_group::IncidentGroupRepository, mute::MuteRuleRepository,
            repositories::IncidentRcaRepository, semantic_group::SemanticGroupRepository,
        },
        function::FunctionRepository,
        iam::{
            IamPlatformAdministratorRepository, InstanceSettingsRepository, SsoProviderRepository,
            TeamRepository, api_token::ApiTokenRepository,
        },
        license::LicenseVersionRepository,
        query::SlowQueryRepository,
        rum::DebugArtifactRepository,
        saved_view::SavedViewRepository,
        storage::ParquetFileMetaRepository,
        stream::StreamRepository,
        trace_policy::{TraceDebugTokenRepository, TracePolicyRepository},
    },
    infra::{
        caching::{BillingStateCache, OrgSchemaCache, ParquetDiskCache},
        cipher::{CipherKeyRepository, FieldKeyService},
        cluster::{ClusterSecretRepository, RemoteClustersRepository},
        connectors::ConnectorRepository,
        ingester::PrometheusSeriesAdmission,
        masking::MaskingService,
        notify::EmailSender,
        persistence::repositories::{
            annotations::AnnotationRepository,
            audit_events::AuditEventRepository,
            billing_settings::BillingSettingsRepository,
            cluster::{
                events::{
                    ClusterEventOutboxRepository, ClusterOrgLinkRepository,
                    ClusterResourceVersionRepository, SeenEventsRepository,
                },
                nodes::ClusterNodesRepository,
            },
            domains::DomainRepository,
            email_domains::EmailDomainRepository,
            file_download_tokens::FileDownloadTokenRepository,
            iam::roles::IamRoleRepository,
            intelligence::{
                chat_archives::ChatArchiveRepository, chats::ChatRepository,
                model_providers::ModelProviderRepository, prompts::AgentPromptRepository,
                toolsets::AgentToolsetRepository,
            },
            investigation_blobs::InvestigationBlobRepository,
            invitations::InvitationRepository,
            log_patterns::LogPatternRepository,
            marketplace::MarketplaceRepository,
            model_prices::ModelPriceRepository,
            notify::NotifyTemplateManagementRepository,
            password_resets::PasswordResetRepository,
            pipelines::runs::PipelineRunRepository,
            regex_patterns::RegexPatternRepository,
            report_templates::ReportTemplateRepository,
            resource_shares::ResourceShareRepository,
            scheduled_reports::ScheduledReportRepository,
            search::jobs::SearchJobRepository,
            signing_secrets::SigningSecretRepository,
            trials::TrialRepository,
            usage::UsageRepository,
            user_preferences::UserPreferencesRepository,
            web_search::WebSearchRepository,
            workspace_preference_defaults::WorkspacePreferenceDefaultsRepository,
        },
        pipeline::{ExtendKvRepository, ExtendTable, ScheduledPipelineRepository},
        query::federation_cancel::FederationCancelRegistry,
        quotas::QuotaLimiter,
        rum::replay::RumReplayWriter,
        sso::{JwksCache, SsoSessionRepository, SsoStateStore},
        traces::ServiceGraphRepository,
    },
    intelligence::{model::IntelligenceRepository, tool_control::ToolControlRepository},
    shared::{
        LicenseGate, LicenseHolder, ReportRenderer, drain::DrainController, health::Probe, ids::Id,
        tail_sampling::TailSampler,
    },
};

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct TraceSystemLoadHealth {
    pub system_org: bool,
    pub license: bool,
    pub trace_policy: bool,
}

/// 注入到 axum 的应用状态。顶层只保留核心 application service 与功能状态，
/// 具体 repository 不再平铺为全局字段。
#[derive(Clone)]
pub struct AppState {
    pub ingestion: Arc<IngestService>,
    pub query: Arc<QueryService>,
    pub dashboard: Arc<DashboardService>,
    pub alerting: AlertingState,
    pub iam: IamState,
    pub telemetry: TelemetryState,
    pub storage: StorageState,
    pub cluster: ClusterState,
    pub platform: PlatformState,
    pub intelligence: IntelligenceState,
}

#[derive(Clone)]
pub struct AlertingState {
    pub service: Arc<AlertingService>,
    pub notify: Arc<NotifyService>,
    pub notify_engine: Arc<NotifyEngine>,
    pub incident_groups: Arc<dyn IncidentGroupRepository>,
    pub semantic_groups: Arc<dyn SemanticGroupRepository>,
    pub templates: Arc<dyn NotifyTemplateManagementRepository>,
    pub mute_rules: Arc<dyn MuteRuleRepository>,
}

#[derive(Clone)]
pub struct IamState {
    pub service: Arc<IamService>,
    pub access: Arc<IamAccessService>,
    pub system_org_id: Id,
    pub teams: Arc<dyn TeamRepository>,
    pub platform_administrators: Arc<dyn IamPlatformAdministratorRepository>,
    pub password_resets: Arc<dyn PasswordResetRepository>,
    pub instance_settings: Arc<dyn InstanceSettingsRepository>,
    pub sso_state_store: Arc<SsoStateStore>,
    pub sso_jwks_cache: Arc<JwksCache>,
    pub sso_sessions: Arc<dyn SsoSessionRepository>,
    pub sso_providers: Arc<dyn SsoProviderRepository>,
    pub email_sender: Option<Arc<EmailSender>>,
    pub invitations: Arc<dyn InvitationRepository>,
    pub email_domains: Arc<dyn EmailDomainRepository>,
    pub roles: Arc<dyn IamRoleRepository>,
    pub signing_secrets: Arc<dyn SigningSecretRepository>,
    pub api_tokens: Arc<dyn ApiTokenRepository>,
    pub user_preferences: Arc<dyn UserPreferencesRepository>,
    pub workspace_preference_defaults: Arc<dyn WorkspacePreferenceDefaultsRepository>,
    pub audit_events: Arc<dyn AuditEventRepository>,
}

#[derive(Clone)]
pub struct TelemetryState {
    pub streams: Arc<dyn StreamRepository>,
    pub stream_retention_days: u32,
    pub self_telemetry_org_id: Option<Id>,
    pub self_telemetry_runtime: Option<Arc<SelfTelemetryRuntime>>,
    pub self_telemetry_resource: Option<crate::shared::self_telemetry::ResourceIdentity>,
    pub self_telemetry_profiles_enabled: bool,
    pub self_telemetry_cluster_token: Option<Arc<str>>,
    pub profile_storage: Arc<ProfileStorageService>,
    pub profiling_service: Arc<ProfilingService>,
    pub profiling_settings: crate::config::ProfilingSettings,
    pub prometheus_series_admission: Arc<PrometheusSeriesAdmission>,
    pub probe: Arc<Probe>,
    pub system_load_health: TraceSystemLoadHealth,
    pub service_graph: Arc<dyn ServiceGraphRepository>,
    pub rum_replay: Arc<RumReplayWriter>,
    pub apm_query: Arc<crate::app::apm::ApmQueryService>,
    pub apm_runtime: Option<Arc<crate::app::apm::ApmRuntime>>,
    pub trace_policies: Arc<dyn TracePolicyRepository>,
    pub trace_debug_tokens: Arc<dyn TraceDebugTokenRepository>,
    pub tail_sampler: Arc<TailSampler>,
    pub trace_candidates: Arc<TraceCandidateRouter>,
    pub trace_pipeline: Arc<TracePipeline>,
}

#[derive(Clone)]
pub struct StorageState {
    pub object_store: Arc<dyn ObjectStore>,
    pub parquet_file_meta: Arc<dyn ParquetFileMetaRepository>,
    pub cipher_keys: Arc<dyn CipherKeyRepository>,
    pub field_keys: Arc<FieldKeyService>,
    pub connectors: Arc<dyn ConnectorRepository>,
    pub scheduled_pipelines: Arc<dyn ScheduledPipelineRepository>,
    pub extend_kv: Arc<dyn ExtendKvRepository>,
    pub extend_table: Arc<ExtendTable>,
    pub parquet_disk_cache: Option<Arc<ParquetDiskCache>>,
    pub resource_shares: Arc<dyn ResourceShareRepository>,
    pub annotations: Arc<dyn AnnotationRepository>,
    pub search_jobs: Arc<dyn SearchJobRepository>,
    pub functions: Arc<dyn FunctionRepository>,
    pub functions_js_runtime_enabled: bool,
    pub debug_artifacts: Arc<dyn DebugArtifactRepository>,
    pub log_patterns: Arc<dyn LogPatternRepository>,
    pub file_download_tokens: Arc<dyn FileDownloadTokenRepository>,
    pub web_search: Arc<dyn WebSearchRepository>,
    pub investigation_blobs: Arc<dyn InvestigationBlobRepository>,
    pub org_schema_cache: Arc<OrgSchemaCache>,
    pub pipeline_runs: Arc<dyn PipelineRunRepository>,
    pub regex_patterns: Arc<dyn RegexPatternRepository>,
    pub masking: Arc<MaskingService>,
}

#[derive(Clone)]
pub struct ClusterState {
    pub node_id: String,
    pub drain: Arc<DrainController>,
    pub registry: Arc<dyn ClusterRegistry>,
    pub repository: Arc<dyn ClusterNodesRepository>,
    pub remote_clusters: Arc<dyn RemoteClustersRepository>,
    pub event_outbox: Arc<dyn ClusterEventOutboxRepository>,
    pub resource_version: Arc<dyn ClusterResourceVersionRepository>,
    pub org_link: Arc<dyn ClusterOrgLinkRepository>,
    pub seen_events: Arc<dyn SeenEventsRepository>,
    pub federation_cancel: Arc<FederationCancelRegistry>,
    pub secrets: Arc<dyn ClusterSecretRepository>,
}

#[derive(Clone)]
pub struct PlatformState {
    pub saved_view: Arc<dyn SavedViewRepository>,
    pub external_url: String,
    pub license: Arc<dyn LicenseGate>,
    pub license_holder: Arc<LicenseHolder>,
    pub license_versions: Arc<dyn LicenseVersionRepository>,
    pub scheduled_reports: Arc<dyn ScheduledReportRepository>,
    pub report_templates: Arc<dyn ReportTemplateRepository>,
    pub report_renderer: Option<Arc<dyn ReportRenderer>>,
    pub report_renderer_base_url: String,
    pub marketplace: Arc<dyn MarketplaceRepository>,
    pub billing_settings: Arc<dyn BillingSettingsRepository>,
    pub usage: Arc<dyn UsageRepository>,
    pub trials: Arc<dyn TrialRepository>,
    pub billing_enabled: Arc<std::sync::atomic::AtomicBool>,
    pub billing_state_cache: Arc<BillingStateCache>,
    pub quotas: Arc<QuotaLimiter>,
    pub model_prices: Arc<dyn ModelPriceRepository>,
    pub domains: Arc<dyn DomainRepository>,
}

#[derive(Clone)]
pub struct IntelligenceState {
    pub dashboard_authoring: Arc<DashboardAuthoringService>,
    pub chats: Arc<dyn ChatRepository>,
    pub repository: Arc<dyn IntelligenceRepository>,
    pub toolsets: Arc<dyn AgentToolsetRepository>,
    pub tool_control: Arc<dyn ToolControlRepository>,
    pub model_providers: Arc<dyn ModelProviderRepository>,
    pub prompts: Arc<dyn AgentPromptRepository>,
    pub chat_archives: Arc<dyn ChatArchiveRepository>,
    pub incident_rca: Arc<dyn IncidentRcaRepository>,
    pub slow_queries: Arc<dyn SlowQueryRepository>,
}
