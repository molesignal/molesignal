// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 摄取、WAL、对象存储写入与 compactor 运行时装配。

use std::sync::Arc;

use super::core::Core;
use crate::{
    app::{
        ingestion::IngestService, profile_storage::ProfileStorageService,
        profiling::ProfilingService,
    },
    bootstrap::roles::{compactor::spawn_compactor_loop, ingester::IngesterWorker},
    config::{Settings, WalFlushStrategy, WalSettings, WalSyncLevel},
    domain::{iam::OrganizationRepository, stream::StreamRepository},
    infra::{
        caching::{
            DiskCacheSettings as InfraDiskCacheSettings, ParquetDiskCache, ParquetFileMetaCache,
        },
        ingester::{BufferPool, PrometheusSeriesAdmission, WalPool},
        persistence::repositories::{
            intelligence::model_providers::ModelProviderRepository,
            usage::{PgUsageRepository, UsageRepository},
        },
        segment_wal::{FsyncPolicy, StaticTermSource, SyncLevel, TermSource},
        storage::{
            compactor::Compactor,
            parquet::{reader::ParquetReader, writer::ParquetWriter},
        },
        traces::{ServiceGraphAggregator, ServiceGraphObserverImpl},
    },
    shared::{Error, Result, health::Probe, ids::Id, time::TimestampMicros},
};

pub(super) struct StorageRuntime {
    pub(super) worker: Arc<IngesterWorker>,
    pub(super) probe: Arc<Probe>,
    pub(super) ingestion: Arc<IngestService>,
    pub(super) profile_storage: Arc<ProfileStorageService>,
    pub(super) profiling_service: Arc<ProfilingService>,
    pub(super) prometheus_series_admission: Arc<PrometheusSeriesAdmission>,
    pub(super) functions_js_runtime_enabled: bool,
    pub(super) service_graph_aggregator: Arc<ServiceGraphAggregator>,
    pub(super) usage: Arc<dyn UsageRepository>,
    pub(super) investigation_blobs: Arc<
        dyn crate::infra::persistence::repositories::investigation_blobs::InvestigationBlobRepository,
    >,
}

impl StorageRuntime {
    pub(super) async fn build(
        settings: &Settings,
        core: &Core,
        model_providers: Arc<dyn ModelProviderRepository>,
    ) -> Result<Self> {
        let segment_bytes = (settings.wal.segment_size_mb as usize)
            .saturating_mul(1024 * 1024)
            .max(64 * 1024);
        std::fs::create_dir_all(&settings.wal.dir).map_err(|error| {
            Error::internal(format!("create wal dir {}: {error}", settings.wal.dir))
        })?;
        let fsync_policy = build_fsync_policy(&settings.wal);
        tracing::info!(
            wal_dir = %settings.wal.dir,
            flush_strategy = ?settings.wal.flush_strategy,
            sync_level = ?settings.wal.sync_level,
            batch_max_pending = settings.wal.batch_max_pending,
            batch_max_delay_ms = settings.wal.batch_max_delay_ms,
            "wal fsync policy resolved"
        );

        let term_source: Arc<dyn TermSource> = Arc::new(StaticTermSource(1));
        let wal_pool = {
            let pool = WalPool::new(&settings.wal.dir, segment_bytes, fsync_policy, term_source);
            let pool = if settings.wal.encrypt {
                pool.with_cipher(core.cipher_root_key.clone())
            } else {
                pool
            };
            Arc::new(pool)
        };
        let buffer_pool = Arc::new(BufferPool::with_memory_limit_bytes(
            (settings.ingester.max_buffer_memory_mb as usize).saturating_mul(1024 * 1024),
        ));
        let prometheus_series_admission = Arc::new(PrometheusSeriesAdmission::new(
            settings.ingester.prometheus.cardinality.clone(),
        ));
        let parquet_writer = Arc::new(ParquetWriter::new(core.store.clone()));
        let parquet_reader = Arc::new(ParquetReader::new(core.store.clone()));
        let parquet_file_meta_cache = Arc::new(ParquetFileMetaCache::new(
            settings.cache.parquet_file_meta.clone(),
        ));
        let probe = Arc::new(Probe::new());

        let worker = Arc::new(
            IngesterWorker::new(
                wal_pool,
                buffer_pool,
                core.streams.clone(),
                core.parquet_file_meta.clone(),
                parquet_writer.clone(),
                Some(parquet_file_meta_cache),
                probe.clone(),
                settings.ingester.clone(),
            )
            .with_field_keys(core.field_key_service.clone())
            .with_drain(core.drain_controller.clone()),
        );
        worker.recover_and_replay().await?;
        let _flush_handle = core
            .roles
            .run_ingester
            .then(|| worker.clone().spawn_flush_loop());
        let _probe_handle = crate::bootstrap::roles::health_probe::spawn_probe(
            core.store.clone(),
            probe.clone(),
            settings.store.object.health_probe_interval_secs,
        );

        let functions: Arc<dyn crate::domain::function::FunctionRepository> = Arc::new(
            crate::infra::persistence::repositories::functions::PgFunctionRepository::new(
                core.pool.clone(),
            ),
        );
        let pipelines: Arc<dyn crate::domain::pipeline::PipelineRepository> = Arc::new(
            crate::infra::persistence::repositories::pipelines::PgPipelineRepository::new(
                core.pool.clone(),
            ),
        );
        let vrl_executor = Arc::new(crate::infra::runtime::VrlFunctionExecutor::new());
        let js_executor: Option<Arc<dyn crate::app::ingestion::FunctionExecutor>> = {
            #[cfg(feature = "js-runtime")]
            {
                tracing::info!(
                    "JS function runtime enabled (deno_core; compiled in via --features js-runtime)"
                );
                Some(Arc::new(
                    crate::infra::runtime::JsFunctionExecutor::with_defaults(),
                ))
            }
            #[cfg(not(feature = "js-runtime"))]
            {
                None
            }
        };
        let llm_executor: Option<Arc<dyn crate::app::ingestion::FunctionExecutor>> =
            if settings.functions.llm_eval_enabled {
                tracing::info!("LLM eval function runtime enabled");
                Some(Arc::new(
                    crate::bootstrap::llm_executor::LlmFunctionExecutor::new(model_providers),
                ))
            } else {
                None
            };
        let function_executor: Arc<dyn crate::app::ingestion::FunctionExecutor> =
            Arc::new(crate::infra::runtime::ChainedFunctionExecutor::new(
                vrl_executor,
                js_executor,
                llm_executor,
            ));
        let pipeline_engine = Arc::new(crate::app::ingestion::PipelineEngine::new(
            pipelines,
            functions,
            function_executor,
        ));
        let functions_js_runtime_enabled = cfg!(feature = "js-runtime");

        let service_graph_aggregator = Arc::new(ServiceGraphAggregator::new());
        let usage: Arc<dyn UsageRepository> = Arc::new(PgUsageRepository::new(core.pool.clone()));
        let internal_usage = usage.clone();
        let internal_usage_recorder = Arc::new(
            move |org_id: Id, received_at: TimestampMicros, bytes: u64| {
                let usage = internal_usage.clone();
                tokio::spawn(async move {
                    if let Err(error) = usage
                        .add_hourly_ingest_bytes(&org_id, received_at.0, bytes as i64)
                        .await
                    {
                        tracing::warn!(
                            org_id = %org_id.0,
                            error = %error,
                            "failed to record internal hourly ingest usage"
                        );
                    }
                });
            },
        );
        let ingestion = Arc::new(
            IngestService::new(worker.clone(), core.streams.clone())
                .with_system_org_id(core.system_org.id.clone())
                .with_pipeline(pipeline_engine)
                .with_masking(core.masking_service.clone())
                .with_drain(core.drain_controller.clone())
                .with_internal_usage_recorder(internal_usage_recorder)
                .with_service_graph(Arc::new(ServiceGraphObserverImpl::new(
                    service_graph_aggregator.clone(),
                ))),
        );
        let profile_storage = Arc::new(ProfileStorageService::new(
            core.store.clone(),
            ingestion.clone(),
        ));
        let profiling_service = ProfilingService::new();

        let syslog = &settings.syslog;
        if (core.roles.is_standalone || core.roles.run_ingester)
            && (!syslog.udp_bind.trim().is_empty() || !syslog.tcp_bind.trim().is_empty())
        {
            if syslog.org.trim().is_empty() {
                tracing::warn!(
                    "[syslog] configured but `org` is empty; skipping (syslog has no auth/org context)"
                );
            } else {
                match core.orgs.get_by_slug(syslog.org.trim()).await {
                    Ok(org) => {
                        let _syslog = crate::bootstrap::syslog::SyslogListener::new(
                            ingestion.clone(),
                            org.id,
                            syslog.stream.clone(),
                            syslog.udp_bind.clone(),
                            syslog.tcp_bind.clone(),
                        )
                        .spawn();
                    }
                    Err(error) => tracing::warn!(
                        error = %error,
                        slug = %syslog.org,
                        "[syslog].org slug not found; skipping syslog"
                    ),
                }
            }
        }

        let compactor = Arc::new(Compactor::new(
            core.parquet_file_meta.clone(),
            parquet_reader,
            parquet_writer,
            core.store.clone(),
            settings.compactor.clone(),
        ));
        let investigation_blobs: Arc<
            dyn crate::infra::persistence::repositories::investigation_blobs::InvestigationBlobRepository,
        > = Arc::new(
            crate::infra::persistence::repositories::investigation_blobs::PgInvestigationBlobRepository::new(
                core.pool.clone(),
            ),
        );
        let _compactor_handle = core.roles.run_compactor.then(|| {
            spawn_compactor_loop(
                compactor,
                core.orgs.clone() as Arc<dyn OrganizationRepository>,
                core.streams.clone() as Arc<dyn StreamRepository>,
                investigation_blobs.clone(),
                core.store.clone(),
                settings.compactor.clone(),
                core.drain_controller.clone(),
            )
        });

        let parquet_file_meta_dump_service = Arc::new(
            crate::infra::storage::parquet_file_meta_dump::ParquetFileMetaDumpService::new(
                core.pool.clone(),
                core.store.clone(),
                settings.storage.parquet_file_meta_dump.clone(),
            )
            .with_dump_cache(core.parquet_file_meta_dump_cache.clone()),
        );
        let _parquet_file_meta_dump_handle = core.roles.run_compactor.then(|| {
            crate::bootstrap::workers::parquet_file_meta_dumper::spawn(
                parquet_file_meta_dump_service,
            )
        });

        Ok(Self {
            worker,
            probe,
            ingestion,
            profile_storage,
            profiling_service,
            prometheus_series_admission,
            functions_js_runtime_enabled,
            service_graph_aggregator,
            usage,
            investigation_blobs,
        })
    }
}

/// 根据 `[cache.disk_cache]` 实例化 [`ParquetDiskCache`]。
///
/// `enabled=false` 或 `max_size_gb=0` 视为整层关闭：返回 `None`，缓存目录不会被
/// 创建，`ProductionObjectStore` 走纯 inner 路径。
pub(super) fn build_parquet_disk_cache(
    settings: &crate::config::DiskCacheSettings,
) -> Result<Option<Arc<ParquetDiskCache>>> {
    if !settings.is_effectively_enabled() {
        tracing::info!("parquet disk cache: disabled");
        return Ok(None);
    }
    let cache = ParquetDiskCache::new(InfraDiskCacheSettings {
        dir: settings.dir.clone(),
        max_bytes: settings.max_size_bytes(),
    })?;
    tracing::info!(
        dir = %settings.dir.display(),
        max_size_gb = settings.max_size_gb,
        "parquet disk cache: enabled"
    );
    Ok(Some(Arc::new(cache)))
}

/// 把 `[wal]` settings 字段映射到 `SegmentWal` 的 [`FsyncPolicy`]。
pub(super) fn build_fsync_policy(wal: &WalSettings) -> FsyncPolicy {
    let sync_level = match wal.sync_level {
        WalSyncLevel::None => SyncLevel::NONE,
        WalSyncLevel::Data => SyncLevel::DATA,
        WalSyncLevel::All => SyncLevel::ALL,
    };
    match wal.flush_strategy {
        WalFlushStrategy::None => FsyncPolicy::None { sync_level },
        WalFlushStrategy::EveryWrite => FsyncPolicy::EveryWrite { sync_level },
        WalFlushStrategy::Batch => FsyncPolicy::Batch {
            max_pending: wal.batch_max_pending.max(1) as usize,
            max_delay_ms: wal.batch_max_delay_ms as u64,
            sync_level,
        },
    }
}
