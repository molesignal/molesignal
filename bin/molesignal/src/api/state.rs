// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::Arc;

use object_store::ObjectStore;

use crate::{
    agent::{
        automations::AutomationRepository, currencies::CurrencyRepository,
        feedback::AgentFeedbackRepository, im::ImRepository, inbound_mcp::InboundMcpRepository,
        knowledge::KnowledgeStore, memory::MemoryStore, model::AgentRepository,
        network::NetworkPolicyStore, runs::RunRepository, scoring::ScoringRepository,
        tool_control::ToolControlRepository,
    },
    app::{
        agent_usage::AgentUsage,
        alerting::{AlertSources, AlertingService, RuleEvaluator},
        cluster::ClusterRegistry,
        dashboard::{DashboardService, authoring::DashboardAuthoringService},
        iam::{IamAccessService, IamService, signin_limit::SigninLimiter},
        intake::{
            FunctionExecutor, IntakeService,
            usage::{IntakeSourceDirectory, IntakeUsageMeter},
        },
        log_inspection::LogInspectionService,
        mcp::McpService,
        notify::{NotifyEngine, NotifyService},
        profile_storage::ProfileStorageService,
        profiling::ProfilingService,
        query::QueryService,
        self_telemetry::SelfTelemetryRuntime,
        skills::SkillService,
        status_page::StatusPageService,
        synthetics::{ProbeControlService, SyntheticService},
        tools::ToolRuntime,
        trace::{TracePipeline, candidate_router::TraceCandidateRouter},
    },
    domain::{
        alerting::{
            incident_group::IncidentGroupRepository, mute::MuteRuleRepository,
            repositories::IncidentRcaRepository, semantic_group::SemanticGroupRepository,
        },
        function::FunctionRepository,
        i18n_language::I18nLanguageRepository,
        iam::{
            IamPlatformAdministratorRepository, InstanceSettingsRepository, SsoProviderRepository,
            TeamRepository, api_token::ApiTokenRepository,
            service_account::ServiceAccountRepository,
        },
        intake::usage::{budget::IntakeBudgetRepository, source::IntakeUsageRepository},
        metrics::MetricDirectory,
        query::{PromqlEngine, QueryEngine, SlowQueryRepository},
        rum::{
            DebugArtifactRepository, RumAlertSource, RumApplicationRepository, RumIssueRepository,
            RumPlaylistRepository, RumReplayCommentRepository, RumSessionSummaryRepository,
        },
        saved_view::SavedViewRepository,
        storage::QueryFileSource,
        stream::StreamRepository,
        trace_policy::{TraceDebugTokenRepository, TracePolicyRepository},
    },
    infra::{
        caching::OrgSchemaCache,
        cipher::{CipherKeyRepository, FieldKeyService},
        cluster::{ClusterSecretRepository, RemoteClustersRepository},
        connectors::ConnectorRepository,
        enrichment::GeoLookup,
        intake::PrometheusSeriesAdmission,
        masking::{FieldMaskingService, MaskingService},
        mfa::{
            MfaChallengeRepository, MfaFactorRepository, MfaLockoutRepository, PasskeyRepository,
            SigninPolicyRepository,
        },
        notify::EmailSender,
        persistence::repositories::{
            agent::{
                chat_archives::ChatArchiveRepository, chats::ChatRepository,
                model_providers::ModelProviderRepository, prompts::AgentPromptRepository,
                toolsets::AgentToolsetRepository,
            },
            annotations::AnnotationRepository,
            audit_events::AuditEventRepository,
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
            iam::{policies::IamPolicyRepository, roles::IamRoleRepository},
            investigation_blobs::InvestigationBlobRepository,
            invitations::InvitationRepository,
            log_patterns::LogPatternRepository,
            notify::NotifyTemplateManagementRepository,
            password_resets::PasswordResetRepository,
            pipelines::runs::PipelineRunRepository,
            regex_patterns::RegexPatternRepository,
            report_templates::ReportTemplateRepository,
            resource_shares::ResourceShareRepository,
            scheduled_reports::ScheduledReportRepository,
            search::jobs::SearchJobRepository,
            signing_secrets::SigningSecretRepository,
            user_preferences::UserPreferencesRepository,
            user_ui_preference::UserUiPreferenceRepository,
            web_search::WebSearchRepository,
            workspace_preference_defaults::WorkspacePreferenceDefaultsRepository,
        },
        pipeline::{ExtendKvRepository, ExtendTable, ScheduledPipelineRepository},
        query::{catalog_source::CatalogQuerySource, federation_cancel::FederationCancelRegistry},
        rum::{intake::RumIntake, replay::RumReplayWriter},
        signin::SignupRequestRepository,
        sso::{
            JwksCache, SsoIdentityRepository, SsoLinkRequestRepository, SsoSigninTestRepository,
            SsoStateStore,
        },
        traces::ServiceGraphRepository,
    },
    shared::{
        ReportRenderer, drain::DrainController, health::Probe, ids::Id, tail_sampling::TailSampler,
    },
};

#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceSystemLoadHealth {
    pub system_org: bool,
    pub trace_policy: bool,
}

/// 注入到 axum 的应用状态。顶层只保留核心 application service 与功能状态，
/// 具体 repository 不再平铺为全局字段。
#[derive(Clone)]
pub struct AppState {
    pub intake: Arc<IntakeService>,
    pub query: Arc<QueryService>,
    pub search_jobs: Arc<crate::app::query::jobs::SearchJobService>,
    pub dashboard: Arc<DashboardService>,
    pub status_pages: Arc<StatusPageService>,
    pub synthetics: Arc<SyntheticService>,
    pub probe_control: Option<Arc<ProbeControlService>>,
    pub alerting: AlertingState,
    pub iam: IamState,
    pub telemetry: TelemetryState,
    pub storage: StorageState,
    pub cluster: ClusterState,
    pub platform: PlatformState,
    pub agent: AgentState,
    /// 协议中立的平台工具执行入口；Agent 与入站 MCP adapter 共用。
    pub tools: Arc<ToolRuntime>,
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
    /// 告警抑制：规则，以及谁正压着谁。
    pub inhibitions: Arc<crate::app::alerting::Inhibitions>,
    pub evaluator: Arc<RuleEvaluator>,
    /// The RUM figures rules watch, read the way the RUM pages read them.
    pub rum: Arc<dyn RumAlertSource>,
    /// 告警来源：管理来源，接入外部系统发来的告警。
    pub sources: Arc<AlertSources>,
    /// 路由：路由规则、团队的值班策略和「试一试」。
    pub routing: Arc<crate::app::alerting::AlertRouting>,
    /// 人对事件的处理：转派、升级、暂缓、合并、备注、批量处理与时间线。
    pub actions: Arc<crate::app::alerting::IncidentActions>,
    /// 短信回复、电话按键的回调：认出回复的是哪条消息，回调 Token。
    pub reply_callbacks: Arc<crate::app::notify::ReplyCallbacks>,
    /// 洞察里人的部分：响应（MTTA、升级率、各级确认率）和值班负担。
    pub insights: Arc<crate::app::alerting::AlertInsights>,
    /// 像这个事件的过去的事件，和它触发前后的变更。
    pub related: Arc<crate::app::alerting::RelatedIncidents>,
    /// 值班日历的订阅：订阅地址和日历内容。
    pub calendar: Arc<crate::app::alerting::CalendarFeeds>,
    /// 换班申请：发起、答复、批准、撤回。
    pub swaps: Arc<crate::app::alerting::ShiftSwaps>,
    /// 心跳规则的打卡地址与最后一次打卡。
    pub heartbeats: Arc<dyn crate::domain::alerting::heartbeat::HeartbeatRepository>,
    /// 修复动作：定义、在事件上发起、审批后执行与验证。
    pub remediation: Arc<crate::app::alerting::RemediationService>,
    /// 工单：Jira 与飞书项目的连接器、建单、同步与评论。
    pub tickets: Arc<crate::app::alerting::TicketService>,
    /// 重大事件：宣布、角色、作战室、任务、进展与组织设置。
    pub response: Arc<crate::app::alerting::IncidentResponseService>,
    /// 内部复盘与改进项。
    pub postmortems: Arc<crate::app::alerting::PostmortemService>,
    /// 实时规则在本节点的窗口计数：内部 gRPC 把它交给负责汇总的 alert 节点。
    pub realtime: Arc<crate::app::alerting::realtime::RealtimeAlerts>,
    /// 实时规则在本集群各节点的计数之和：联邦里规则的汇总集群来拉。
    pub realtime_counts: Arc<dyn crate::app::alerting::realtime::RealtimeCounts>,
    /// 自动发现找到的问题。
    pub discovery: Arc<dyn crate::domain::alerting::discovery::DiscoveryRepository>,
    /// 节假日日历。
    pub holidays: Arc<crate::app::alerting::HolidayCalendars>,
    /// 组织的优先级与紧急度档位。
    pub importance: Arc<crate::app::alerting::ImportanceService>,
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
    pub sso_identities: Arc<dyn SsoIdentityRepository>,
    pub sso_link_requests: Arc<dyn SsoLinkRequestRepository>,
    pub sso_signin_tests: Arc<dyn SsoSigninTestRepository>,
    pub sso_providers: Arc<dyn SsoProviderRepository>,
    pub email_sender: Option<Arc<EmailSender>>,
    pub invitations: Arc<dyn InvitationRepository>,
    pub email_domains: Arc<dyn EmailDomainRepository>,
    pub roles: Arc<dyn IamRoleRepository>,
    pub policies: Arc<dyn IamPolicyRepository>,
    pub signing_secrets: Arc<dyn SigningSecretRepository>,
    pub api_tokens: Arc<dyn ApiTokenRepository>,
    pub service_accounts: Arc<dyn ServiceAccountRepository>,
    pub user_preferences: Arc<dyn UserPreferencesRepository>,
    pub user_ui_preference: Arc<dyn UserUiPreferenceRepository>,
    pub workspace_preference_defaults: Arc<dyn WorkspacePreferenceDefaultsRepository>,
    pub audit_events: Arc<dyn AuditEventRepository>,
    /// 账号的第二因素：身份验证器与恢复码。
    pub mfa_factors: Arc<dyn MfaFactorRepository>,
    /// 第二步验证输错的次数。
    pub mfa_lockouts: Arc<dyn MfaLockoutRepository>,
    /// 等第二步验证的登录、正在设置的身份验证器和进行中的 Passkey 操作。
    pub mfa_challenges: Arc<dyn MfaChallengeRepository>,
    /// 账号的 Passkey。
    pub passkeys: Arc<dyn PasskeyRepository>,
    pub signin_policies: Arc<dyn SigninPolicyRepository>,
    /// 密码登录限速。
    pub signin_limiter: Arc<SigninLimiter>,
    /// 等邮箱验证的注册。
    pub signup_requests: Arc<dyn SignupRequestRepository>,
}

#[derive(Clone)]
pub struct TelemetryState {
    pub streams: Arc<dyn StreamRepository>,
    /// Which metrics each metrics stream holds.
    pub metric_directory: Arc<dyn MetricDirectory>,
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
    /// `POST /rum/events`：解码、校验、展平并富化事件，再写进各类事件的流。
    pub rum_intake: Arc<RumIntake>,
    /// 发送 RUM 数据的应用；事件只能写进已存在的应用。
    pub rum_applications: Arc<dyn RumApplicationRepository>,
    /// 应用的错误归并成的 issue：状态、指派与出现的历史；列表和改状态读写它。
    pub rum_issues: Arc<dyn RumIssueRepository>,
    /// 会话播放列表：团队留着要依次看完的会话，各自带一个值得从那里开始看的时刻。
    pub rum_playlists: Arc<dyn RumPlaylistRepository>,
    /// 回放上的评论：每条落在会话的某个时刻。
    pub rum_comments: Arc<dyn RumReplayCommentRepository>,
    /// 模型写的会话摘要和章节。
    pub rum_summaries: Arc<dyn RumSessionSummaryRepository>,
    /// RUM 写入按服务端解析的客户端 IP 查位置；不接收 RUM 写入的节点是不查询的空库。
    pub geoip: Arc<dyn GeoLookup>,
    pub apm_query: Arc<crate::app::apm::ApmQueryService>,
    /// What people keep about services: definitions and error states.
    pub apm_catalog: Arc<dyn crate::domain::apm::ApmCatalogRepository>,
    pub apm_runtime: Option<Arc<crate::app::apm::ApmRuntime>>,
    pub trace_policies: Arc<dyn TracePolicyRepository>,
    pub trace_debug_tokens: Arc<dyn TraceDebugTokenRepository>,
    pub tail_sampler: Arc<TailSampler>,
    pub trace_candidates: Arc<TraceCandidateRouter>,
    pub trace_pipeline: Arc<TracePipeline>,
}

#[derive(Clone)]
pub struct StorageState {
    /// Origin store for object writers and lifecycle operations.
    pub object_store: Arc<dyn ObjectStore>,
    /// Catalog-aware read store; remote backends pass through the shared block cache.
    pub read_store: Arc<dyn ObjectStore>,
    pub catalog_files: Arc<dyn QueryFileSource>,
    pub catalog_query: Arc<CatalogQuerySource>,
    pub cipher_keys: Arc<dyn CipherKeyRepository>,
    pub field_keys: Arc<FieldKeyService>,
    pub connectors: Arc<dyn ConnectorRepository>,
    pub scheduled_pipelines: Arc<dyn ScheduledPipelineRepository>,
    pub extend_kv: Arc<dyn ExtendKvRepository>,
    pub extend_table: Arc<ExtendTable>,
    pub resource_shares: Arc<dyn ResourceShareRepository>,
    pub annotations: Arc<dyn AnnotationRepository>,
    pub search_jobs: Arc<dyn SearchJobRepository>,
    pub functions: Arc<dyn FunctionRepository>,
    pub function_executor: Arc<dyn FunctionExecutor>,
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
    pub field_masking_rules: Arc<dyn crate::domain::masking::FieldMaskingRuleRepository>,
    pub field_masking: Arc<FieldMaskingService>,
}

/// 本节点用本地数据执行查询的引擎。querier 角色经内部 gRPC 把它们提供给其它节点。
#[derive(Clone)]
pub struct LocalQueryEngines {
    pub sql: Arc<dyn QueryEngine>,
    pub promql: Arc<dyn PromqlEngine>,
}

#[derive(Clone)]
pub struct ClusterState {
    pub node_id: String,
    pub query_engines: LocalQueryEngines,
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
    /// `[http].gzip`：内置网页由本进程压缩；前面有反向代理负责压缩时为 false。
    pub http_gzip: bool,
    /// Text the backend writes itself, such as email subjects and bodies.
    pub i18n_language: Arc<dyn I18nLanguageRepository>,
    pub scheduled_reports: Arc<dyn ScheduledReportRepository>,
    /// 定时日志巡检：保存、启停、手动执行与读取报告。
    pub log_inspections: Arc<LogInspectionService>,
    pub report_templates: Arc<dyn ReportTemplateRepository>,
    pub report_renderer: Option<Arc<dyn ReportRenderer>>,
    pub report_renderer_base_url: String,
    /// Reads what each source has taken in.
    pub usage: Arc<dyn IntakeUsageRepository>,
    /// Counts what this node takes in.
    pub intake_meter: Arc<IntakeUsageMeter>,
    /// What realtime pipelines' metric, quota and throttle steps hold between events, and the
    /// ledger of what the limits let through.
    pub pipeline_steps: Arc<crate::app::intake::realtime::StepState>,
    pub intake_budgets: Arc<dyn IntakeBudgetRepository>,
    pub intake_sources: Arc<IntakeSourceDirectory>,
    pub domains: Arc<dyn DomainRepository>,
}

#[derive(Clone)]
pub struct AgentState {
    pub dashboard_authoring: Arc<DashboardAuthoringService>,
    pub chats: Arc<dyn ChatRepository>,
    pub repository: Arc<dyn AgentRepository>,
    pub toolsets: Arc<dyn AgentToolsetRepository>,
    pub tool_control: Arc<dyn ToolControlRepository>,
    pub inbound_mcp: Arc<dyn InboundMcpRepository>,
    pub(crate) inbound_mcp_request_state_key: [u8; 32],
    pub model_providers: Arc<dyn ModelProviderRepository>,
    /// 模型服务单价可选的币种。
    pub currencies: Arc<dyn CurrencyRepository>,
    pub prompts: Arc<dyn AgentPromptRepository>,
    pub chat_archives: Arc<dyn ChatArchiveRepository>,
    pub incident_rca: Arc<dyn IncidentRcaRepository>,
    pub slow_queries: Arc<dyn SlowQueryRepository>,
    /// 每次模型调用的用量记账与组织的每日预算。
    pub usage: Arc<AgentUsage>,
    pub feedback: Arc<dyn AgentFeedbackRepository>,
    /// 内置、实例目录、上传与仓库来源的 Skill。
    pub skills: Arc<SkillService>,
    /// 团队成员与个人建的连接可以连的内网主机与网段。
    pub network: Arc<dyn NetworkPolicyStore>,
    /// 外部 MCP 服务器：作用域、连接、同步与调用。
    pub mcp: Arc<McpService>,
    /// 一轮回答 = 一个 Run：排队、执行、事件与续读。
    pub runs: Arc<dyn RunRepository>,
    /// 组织与各团队的知识文件。
    pub knowledge: Arc<dyn KnowledgeStore>,
    /// 个人与团队的记忆。
    pub memory: Arc<dyn MemoryStore>,
    /// 自动化规则与它们的运行记录。
    pub automations: Arc<dyn AutomationRepository>,
    /// 质量：评审、打分队列、分数、待审与 Prompt 晋级检查。
    pub scoring: Arc<dyn ScoringRepository>,
    /// 在 IM 里回答：机器人、话题对应的会话、收到的消息与绑定链接。
    pub im: Arc<dyn ImRepository>,
}
