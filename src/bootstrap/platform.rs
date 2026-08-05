// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 平台控制面、报表、搜索任务、计费与配额运行时装配。

use std::sync::Arc;

use super::{
    core::Core,
    query::QueryRuntime,
    storage::{StorageRuntime, build_parquet_disk_cache},
};
use crate::{
    config::Settings,
    domain::{function::FunctionRepository, rum::DebugArtifactRepository},
    infra::{
        caching::{BillingStateCache, ParquetDiskCache},
        connectors::{ConnectorRepository, PgConnectorRepository},
        persistence::repositories::{
            annotations::{AnnotationRepository, PgAnnotationRepository},
            billing_settings::{BillingSettingsRepository, PgBillingSettingsRepository},
            debug_artifacts::PgDebugArtifactRepository,
            domains::{DomainRepository, PgDomainRepository},
            file_download_tokens::{FileDownloadTokenRepository, PgFileDownloadTokenRepository},
            functions::PgFunctionRepository,
            log_patterns::{LogPatternRepository, PgLogPatternRepository},
            marketplace::{MarketplaceRepository, PgMarketplaceRepository},
            model_prices::{ModelPriceRepository, PgModelPriceRepository},
            pipelines::runs::{PgPipelineRunRepository, PipelineRunRepository},
            report_templates::{PgReportTemplateRepository, ReportTemplateRepository},
            resource_shares::{PgResourceShareRepository, ResourceShareRepository},
            scheduled_reports::{PgScheduledReportRepository, ScheduledReportRepository},
            search::jobs::{PgSearchJobRepository, SearchJobRepository},
            trials::{PgTrialRepository, TrialRepository},
            web_search::{PgWebSearchRepository, WebSearchRepository},
        },
        pipeline::{
            ExtendKvRepository, ExtendTable, PgExtendKvRepository, PgScheduledPipelineRepository,
            ScheduledPipelineRepository,
        },
        quotas::QuotaLimiter,
    },
    shared::{ReportRenderer, Result},
};

pub(super) struct PlatformRuntime {
    pub(super) connectors: Arc<dyn ConnectorRepository>,
    pub(super) scheduled_pipelines: Arc<dyn ScheduledPipelineRepository>,
    pub(super) extend_kv: Arc<dyn ExtendKvRepository>,
    pub(super) extend_table: Arc<ExtendTable>,
    pub(super) parquet_disk_cache: Option<Arc<ParquetDiskCache>>,
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
    pub(super) marketplace: Arc<dyn MarketplaceRepository>,
    pub(super) model_prices: Arc<dyn ModelPriceRepository>,
    pub(super) domains: Arc<dyn DomainRepository>,
    pub(super) billing_settings: Arc<dyn BillingSettingsRepository>,
    pub(super) trials: Arc<dyn TrialRepository>,
    pub(super) billing_enabled: Arc<std::sync::atomic::AtomicBool>,
    pub(super) billing_state_cache: Arc<BillingStateCache>,
    pub(super) pipeline_runs: Arc<dyn PipelineRunRepository>,
    pub(super) quotas: Arc<QuotaLimiter>,
}

impl PlatformRuntime {
    pub(super) async fn build(
        settings: &Settings,
        core: &Core,
        query: &QueryRuntime,
        storage: &StorageRuntime,
    ) -> Result<Self> {
        let connectors: Arc<dyn ConnectorRepository> =
            Arc::new(PgConnectorRepository::new(core.pool.clone()));
        let _connector_runner = core.roles.run_alert_manager.then(|| {
            crate::infra::connectors::ConnectorRunner::new(
                connectors.clone(),
                storage.ingestion.clone(),
                30,
            )
            .spawn()
        });

        let scheduled_pipelines: Arc<dyn ScheduledPipelineRepository> =
            Arc::new(PgScheduledPipelineRepository::new(core.pool.clone()));
        let extend_kv: Arc<dyn ExtendKvRepository> =
            Arc::new(PgExtendKvRepository::new(core.pool.clone()));
        let extend_table = Arc::new(ExtendTable::new());
        let parquet_disk_cache = build_parquet_disk_cache(&settings.cache.disk_cache)?;

        let resource_shares: Arc<dyn ResourceShareRepository> = Arc::new(
            PgResourceShareRepository::new(core.pool.clone(), core.cipher_root_key.clone()),
        );
        let annotations: Arc<dyn AnnotationRepository> =
            Arc::new(PgAnnotationRepository::new(core.pool.clone()));
        let search_jobs: Arc<dyn SearchJobRepository> =
            Arc::new(PgSearchJobRepository::new(core.pool.clone()));
        let search_jobs_settings = &settings.search_jobs;
        let _search_jobs_handles = Arc::new(
            crate::bootstrap::workers::search_jobs::SearchJobScheduler::new(
                search_jobs.clone(),
                query.query.clone(),
                core.store.clone(),
                scheduled_pipelines.clone(),
                storage.worker.clone(),
                connectors.clone(),
                Arc::new(crate::infra::connectors::EgressDispatcher),
                crate::bootstrap::workers::search_jobs::SearchJobSchedulerConfig {
                    workers: search_jobs_settings.workers.max(1) as usize,
                    idle_poll_secs: search_jobs_settings.idle_poll_secs,
                    cleanup_interval_secs: search_jobs_settings.cleanup_interval_secs,
                },
            ),
        )
        .spawn();

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
        let _scheduled_reports_handle = Arc::new(
            crate::bootstrap::workers::scheduled_reports::ScheduledReportRunner::new(
                scheduled_reports.clone(),
                core.store.clone(),
                report_renderer.clone(),
                report_renderer_base_url.clone(),
            ),
        )
        .spawn();
        let file_download_tokens: Arc<dyn FileDownloadTokenRepository> =
            Arc::new(PgFileDownloadTokenRepository::new(core.pool.clone()));
        let web_search: Arc<dyn WebSearchRepository> =
            Arc::new(PgWebSearchRepository::new(core.pool.clone()));

        let marketplace: Arc<dyn MarketplaceRepository> =
            Arc::new(PgMarketplaceRepository::new(core.pool.clone()));
        let model_prices: Arc<dyn ModelPriceRepository> =
            Arc::new(PgModelPriceRepository::new(core.pool.clone()));
        let domains: Arc<dyn DomainRepository> =
            Arc::new(PgDomainRepository::new(core.pool.clone()));
        let billing_settings: Arc<dyn BillingSettingsRepository> = Arc::new(
            PgBillingSettingsRepository::new(core.pool.clone(), core.cipher_root_key.clone()),
        );
        let trials: Arc<dyn TrialRepository> = Arc::new(PgTrialRepository::new(core.pool.clone()));
        let billing_enabled = Arc::new(std::sync::atomic::AtomicBool::new(
            billing_settings
                .get()
                .await
                .map(|settings| settings.enabled)
                .unwrap_or(false),
        ));
        let billing_state_cache = Arc::new(BillingStateCache::new());
        let _trial_sweeper = core.roles.run_alert_manager.then(|| {
            crate::bootstrap::workers::trial_sweeper::TrialSweeper::new(
                trials.clone(),
                marketplace.clone(),
                crate::bootstrap::workers::trial_sweeper::TrialSweeperConfig::default(),
            )
            .spawn()
        });
        let pipeline_runs: Arc<dyn PipelineRunRepository> =
            Arc::new(PgPipelineRunRepository::new(core.pool.clone()));
        let _pipeline_runner_handle = core.roles.run_alert_manager.then(|| {
            let executor: Arc<dyn crate::infra::pipeline::PipelineExecutor> = Arc::new(
                crate::bootstrap::workers::pipeline_exec::BootstrapPipelineExecutor::new(
                    query.query.clone(),
                    storage.worker.clone(),
                    connectors.clone(),
                    Arc::new(crate::infra::connectors::EgressDispatcher),
                ),
            );
            let runner = Arc::new(
                crate::infra::pipeline::ScheduledPipelineRunner::with_runs(
                    scheduled_pipelines.clone(),
                    pipeline_runs.clone(),
                )
                .with_executor(executor),
            );
            crate::bootstrap::workers::pipeline_exec::spawn_runner(runner, 15)
        });

        let mmdb = crate::infra::enrichment::mmdb_downloader::MmdbDownloader::new(
            crate::infra::enrichment::mmdb_downloader::MmdbConfig {
                license_key: Some(settings.mmdb.license_key.clone()).filter(|key| !key.is_empty()),
                db_path: std::path::PathBuf::from(&settings.mmdb.db_path),
                refresh_interval_secs: settings.mmdb.refresh_interval_secs,
            },
        );
        let _ = mmdb.ensure_ready().await;
        let _mmdb_refresh = mmdb.spawn_refresh();

        let quotas = Arc::new(QuotaLimiter::new());
        {
            let quotas = quotas.clone();
            let quota_repo =
                crate::infra::persistence::repositories::quotas::PgQuotaRepository::new(
                    core.pool.clone(),
                );
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
                loop {
                    tick.tick().await;
                    match quota_repo.load_quotas().await {
                        Ok(map) => quotas.refresh(map),
                        Err(error) => {
                            tracing::warn!(error = %error, "quota limits refresh failed")
                        }
                    }
                    match quota_repo.storage_usage().await {
                        Ok(usage) => {
                            for (org, bytes) in usage {
                                quotas.update_storage_usage(&org, bytes);
                            }
                        }
                        Err(error) => {
                            tracing::warn!(error = %error, "quota storage usage refresh failed")
                        }
                    }
                }
            });
        }

        Ok(Self {
            connectors,
            scheduled_pipelines,
            extend_kv,
            extend_table,
            parquet_disk_cache,
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
            marketplace,
            model_prices,
            domains,
            billing_settings,
            trials,
            billing_enabled,
            billing_state_cache,
            pipeline_runs,
            quotas,
        })
    }
}
