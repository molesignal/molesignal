// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 平台控制面、报表、搜索任务、计费与配额运行时装配。

use std::sync::Arc;

use super::{
    core::Core,
    iam::IamRuntime,
    query::QueryRuntime,
    roles::{HttpKind, plan_servers},
    storage::StorageRuntime,
};
use crate::{
    config::Settings,
    domain::{function::FunctionRepository, rum::DebugArtifactRepository},
    infra::{
        connectors::{ConnectorRepository, PgConnectorRepository},
        enrichment::{GeoIp, GeoLookup},
        persistence::repositories::{
            annotations::{AnnotationRepository, PgAnnotationRepository},
            debug_artifacts::PgDebugArtifactRepository,
            domains::{DomainRepository, PgDomainRepository},
            file_download_tokens::{FileDownloadTokenRepository, PgFileDownloadTokenRepository},
            functions::PgFunctionRepository,
            log_patterns::{LogPatternRepository, PgLogPatternRepository},
            pipelines::runs::{PgPipelineRunRepository, PipelineRunRepository},
            report_templates::{PgReportTemplateRepository, ReportTemplateRepository},
            resource_shares::{PgResourceShareRepository, ResourceShareRepository},
            scheduled_reports::{PgScheduledReportRepository, ScheduledReportRepository},
            search::jobs::{PgSearchJobRepository, SearchJobRepository},
            web_search::{PgWebSearchRepository, WebSearchRepository},
        },
        pipeline::{
            ExtendKvRepository, ExtendTable, PgExtendKvRepository, PgScheduledPipelineRepository,
            ScheduledPipelineRepository,
        },
    },
    shared::{ReportRenderer, Result},
};

pub(super) struct PlatformRuntime {
    pub(super) connectors: Arc<dyn ConnectorRepository>,
    pub(super) scheduled_pipelines: Arc<dyn ScheduledPipelineRepository>,
    pub(super) extend_kv: Arc<dyn ExtendKvRepository>,
    pub(super) extend_table: Arc<ExtendTable>,
    pub(super) resource_shares: Arc<dyn ResourceShareRepository>,
    pub(super) annotations: Arc<dyn AnnotationRepository>,
    pub(super) search_jobs: Arc<dyn SearchJobRepository>,
    pub(super) functions: Arc<dyn FunctionRepository>,
    pub(super) debug_artifacts: Arc<dyn DebugArtifactRepository>,
    pub(super) log_patterns: Arc<dyn LogPatternRepository>,
    pub(super) scheduled_reports: Arc<dyn ScheduledReportRepository>,
    pub(super) report_templates: Arc<dyn ReportTemplateRepository>,
    pub(super) report_renderer: Option<Arc<dyn ReportRenderer>>,
    pub(super) report_renderer_base_url: String,
    pub(super) file_download_tokens: Arc<dyn FileDownloadTokenRepository>,
    pub(super) web_search: Arc<dyn WebSearchRepository>,
    pub(super) domains: Arc<dyn DomainRepository>,
    pub(super) pipeline_runs: Arc<dyn PipelineRunRepository>,
    pub(super) geoip: Arc<dyn GeoLookup>,
}

impl PlatformRuntime {
    pub(super) fn build(
        settings: &Settings,
        core: &Core,
        query: &QueryRuntime,
        storage: &StorageRuntime,
        iam: &IamRuntime,
    ) -> Result<Self> {
        let connectors: Arc<dyn ConnectorRepository> =
            Arc::new(PgConnectorRepository::new(core.pool.clone()));

        // pull 类 connector 同一时刻只能有一个节点在拉：多个 alert 节点按 connector 分担，
        // 开拉前先过运行准入。
        let _connector_runner = core.roles.run_alert.then(|| {
            crate::infra::connectors::ConnectorRunner::new(
                connectors.clone(),
                storage.intake.clone(),
                30,
            )
            .with_gate(core.alert_run_gate.clone())
            .spawn()
        });

        let scheduled_pipelines: Arc<dyn ScheduledPipelineRepository> =
            Arc::new(PgScheduledPipelineRepository::new(core.pool.clone()));
        let extend_kv: Arc<dyn ExtendKvRepository> =
            Arc::new(PgExtendKvRepository::new(core.pool.clone()));
        let extend_table = Arc::new(ExtendTable::new());

        let resource_shares: Arc<dyn ResourceShareRepository> = Arc::new(
            PgResourceShareRepository::new(core.pool.clone(), core.cipher_root_key.clone()),
        );

        let annotations: Arc<dyn AnnotationRepository> =
            Arc::new(PgAnnotationRepository::new(core.pool.clone()));
        let search_job_repository = Arc::new(PgSearchJobRepository::new(core.pool.clone()));
        let search_jobs: Arc<dyn SearchJobRepository> = search_job_repository.clone();
        let search_jobs_settings = &settings.search_jobs;

        // 搜索 job 由 querier 角色（含 standalone）的节点领取并执行；其它节点只创建与读取 job。
        let _search_jobs_handles = core.roles.run_querier.then(|| {
            let mut handles = Arc::new(
                crate::bootstrap::workers::search_jobs::SearchJobScheduler::new(
                    search_jobs.clone(),
                    query.query.clone(),
                    core.store.clone(),
                    scheduled_pipelines.clone(),
                    storage.intake.clone(),
                    connectors.clone(),
                    Arc::new(crate::infra::connectors::EgressDispatcher),
                    crate::bootstrap::workers::search_jobs::SearchJobSchedulerConfig {
                        max_concurrent_jobs: search_jobs_settings.workers.max(1) as usize,
                        idle_poll_secs: search_jobs_settings.idle_poll_secs,
                        cleanup_interval_secs: search_jobs_settings.cleanup_interval_secs,
                    },
                ),
            )
            .spawn();

            handles.push(
                search_job_repository
                    .clone()
                    .spawn_notification_listener(settings.meta_database.dsn.clone()),
            );

            Arc::new(handles)
        });

        let functions: Arc<dyn FunctionRepository> =
            Arc::new(PgFunctionRepository::new(core.pool.clone()));
        let debug_artifacts: Arc<dyn DebugArtifactRepository> =
            Arc::new(PgDebugArtifactRepository::new(core.pool.clone()));
        let log_patterns: Arc<dyn LogPatternRepository> =
            Arc::new(PgLogPatternRepository::new(core.pool.clone()));

        let scheduled_reports: Arc<dyn ScheduledReportRepository> =
            Arc::new(PgScheduledReportRepository::new(core.pool.clone()));
        let report_templates: Arc<dyn ReportTemplateRepository> =
            Arc::new(PgReportTemplateRepository::new(core.pool.clone()));

        let report_renderer_base_url = settings
            .scheduled_reports
            .renderer
            .base_url
            .trim_end_matches('/')
            .to_string();

        let report_renderer: Option<Arc<dyn ReportRenderer>> = {
            let renderer = &settings.scheduled_reports.renderer;
            tracing::info!(
                base_url = %report_renderer_base_url,
                concurrent_renders = renderer.concurrent_renders,
                render_timeout_secs = renderer.render_timeout_secs,
                "scheduled-reports headless renderer configured"
            );

            let config = crate::report_renderer::RendererConfig {
                concurrent_renders: renderer.concurrent_renders as usize,
                render_timeout_secs: renderer.render_timeout_secs as u64,
                viewport: crate::shared::Viewport {
                    width: renderer.viewport_width,
                    height: renderer.viewport_height,
                },
            };

            Some(Arc::new(
                crate::report_renderer::HeadlessChromeRenderer::new(config),
            ))
        };

        // 定时报告的渲染需要无头 Chrome，由 alert 角色（含 standalone）的节点承担；多个 alert 节点
        // 按报告分担，开跑前先过运行准入。报表页以最后保存报表的用户的身份取数。
        let _scheduled_reports_handle = core.roles.run_alert.then(|| {
            Arc::new(
                crate::bootstrap::workers::scheduled_reports::ScheduledReportRunner::new(
                    scheduled_reports.clone(),
                    core.store.clone(),
                    report_renderer.clone(),
                    report_renderer_base_url.clone(),
                    Arc::new(crate::app::reporting::ScheduledReportIdentity::new(
                        iam.iam.clone(),
                        iam.access.clone(),
                    )),
                )
                .with_gate(core.alert_run_gate.clone()),
            )
            .spawn()
        });

        let file_download_tokens: Arc<dyn FileDownloadTokenRepository> =
            Arc::new(PgFileDownloadTokenRepository::new(core.pool.clone()));
        let web_search: Arc<dyn WebSearchRepository> =
            Arc::new(PgWebSearchRepository::new(core.pool.clone()));

        let domains: Arc<dyn DomainRepository> =
            Arc::new(PgDomainRepository::new(core.pool.clone()));

        let pipeline_runs: Arc<dyn PipelineRunRepository> =
            Arc::new(PgPipelineRunRepository::new(core.pool.clone()));

        let _pipeline_runner_handle = core.roles.run_alert.then(|| {
            let executor: Arc<dyn crate::infra::pipeline::PipelineExecutor> = Arc::new(
                crate::bootstrap::workers::pipeline_exec::BootstrapPipelineExecutor::new(
                    query.query.clone(),
                    storage.intake.clone(),
                    connectors.clone(),
                    Arc::new(crate::infra::connectors::EgressDispatcher),
                ),
            );

            let runner = Arc::new(
                crate::infra::pipeline::ScheduledPipelineRunner::with_runs(
                    scheduled_pipelines.clone(),
                    pipeline_runs.clone(),
                )
                .with_executor(executor)
                .with_gate(core.alert_run_gate.clone()),
            );

            crate::bootstrap::workers::pipeline_exec::spawn_runner(runner, 15)
        });

        // RUM 写入按客户端 IP 查国家。只有提供完整 HTTP API 的节点（standalone / router）接收
        // RUM 写入，其它节点不加载这份约 120 MB 的库。
        let receives_rum = plan_servers(&settings.node.roles).http == HttpKind::Full;

        let geoip: Arc<dyn GeoLookup> = if receives_rum {
            GeoIp::open_or_noop(std::path::Path::new(&settings.mmdb.db_path))
        } else {
            GeoIp::noop()
        };

        Ok(Self {
            connectors,
            scheduled_pipelines,
            extend_kv,
            extend_table,
            resource_shares,
            annotations,
            search_jobs,
            functions,
            debug_artifacts,
            log_patterns,
            scheduled_reports,
            report_templates,
            report_renderer,
            report_renderer_base_url,
            file_download_tokens,
            web_search,

            domains,

            pipeline_runs,
            geoip,
        })
    }
}
