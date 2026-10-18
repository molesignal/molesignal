// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 摄取、WAL、对象存储写入与 compactor 运行时装配。

use std::{sync::Arc, time::Duration};

use super::core::Core;
use crate::{
    app::{
        agent_usage::AgentUsage,
        cluster::{PeerRole, WorkOwnership},
        intake::IntakeService,
        profile_storage::ProfileStorageService,
        profiling::ProfilingService,
    },
    bootstrap::{
        roles::{compactor::spawn_compactor_loop, intake::IntakeWorker},
        workers::storage_maintenance,
    },
    config::{Settings, WalFlushStrategy, WalSettings, WalSyncLevel},
    domain::{
        iam::OrganizationRepository,
        storage::{FileCatalog, QueryFileSource, builtin_registry},
        stream::StreamRepository,
    },
    infra::{
        intake::{BufferPool, DatasetResolver, PrometheusSeriesAdmission, WalPool},
        persistence::repositories::{
            agent::model_providers::ModelProviderRepository,
            file_catalog::PgFileCatalog,
            usage::{PgUsageRepository, UsageRepository},
        },
        query::catalog_source::CatalogQuerySource,
        segment_wal::{FsyncPolicy, SyncLevel},
        storage::{
            compactor::Compactor,
            index_rebuild::IndexRebuildWorker,
            manifest::{PartitionManifestManager, PartitionManifestReader},
            object_gc::ObjectGcWorker,
            parquet::writer::ParquetWriter,
            reconciler::StorageReconciler,
        },
        traces::{ServiceGraphAggregator, ServiceGraphObserverImpl},
    },
    shared::{Error, Result, health::Probe, ids::Id, time::TimestampMicros},
};

pub(super) struct StorageRuntime {
    pub(super) probe: Arc<Probe>,
    pub(super) intake: Arc<IntakeService>,
    pub(super) profile_storage: Arc<ProfileStorageService>,
    pub(super) profiling_service: Arc<ProfilingService>,
    pub(super) prometheus_series_admission: Arc<PrometheusSeriesAdmission>,
    pub(super) function_executor: Arc<dyn crate::app::intake::FunctionExecutor>,
    pub(super) functions_js_runtime_enabled: bool,
    pub(super) service_graph_aggregator: Arc<ServiceGraphAggregator>,
    pub(super) usage: Arc<dyn UsageRepository>,
    pub(super) investigation_blobs: Arc<
        dyn crate::infra::persistence::repositories::investigation_blobs::InvestigationBlobRepository,
    >,
    pub(super) catalog_query: Arc<CatalogQuerySource>,
    pub(super) catalog_files: Arc<dyn QueryFileSource>,
}

impl StorageRuntime {
    pub(super) async fn build(
        settings: &Settings,
        core: &Core,
        model_providers: Arc<dyn ModelProviderRepository>,
        agent_usage: Arc<AgentUsage>,
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

        let wal_pool = {
            let pool = WalPool::new(
                &settings.wal.dir,
                core.node_id.clone(),
                segment_bytes,
                fsync_policy,
            );

            let pool = if settings.wal.encrypt {
                pool.with_cipher(core.cipher_root_key.clone())
            } else {
                pool
            };

            Arc::new(pool)
        };

        let buffer_pool = Arc::new(BufferPool::with_memory_limit_bytes(
            (settings.intake.max_buffer_memory_mb as usize).saturating_mul(1024 * 1024),
        ));

        let prometheus_series_admission = Arc::new(PrometheusSeriesAdmission::new(
            settings.intake.prometheus.cardinality.clone(),
        ));

        let parquet_writer = Arc::new(ParquetWriter::new(core.store.clone()));
        let probe = Arc::new(Probe::new());

        let file_catalog: Arc<dyn FileCatalog> = Arc::new(PgFileCatalog::new(core.pool.clone()));

        let manifest_reader = Arc::new(PartitionManifestReader::new(
            core.object_reader.clone(),
            settings.storage.catalog.manifest_cache_bytes,
        ));

        let dataset_resolver = Arc::new(DatasetResolver::new(
            file_catalog.clone(),
            Arc::new(builtin_registry()),
        ));

        let catalog_query = Arc::new(CatalogQuerySource::new(
            file_catalog.clone(),
            buffer_pool.clone(),
            core.streams.clone(),
            core.object_reader.clone(),
            manifest_reader.clone(),
        ));

        let catalog_files: Arc<dyn QueryFileSource> = catalog_query.clone();
        let replay_byte_cap =
            (settings.wal.max_replay_mb.max(1) as usize).saturating_mul(1024 * 1024);

        let worker = Arc::new(
            IntakeWorker::new(
                wal_pool,
                buffer_pool,
                core.streams.clone(),
                dataset_resolver,
                file_catalog.clone(),
                parquet_writer.clone(),
                probe.clone(),
                settings.intake.clone(),
            )
            .with_replay_byte_cap(replay_byte_cap)
            .with_field_keys(core.field_key_service.clone())
            .with_drain(core.drain_controller.clone()),
        );

        worker.recover_and_replay().await?;
        // 每个节点都 flush 自己缓冲的数据：intake 角色缓冲外部写入，其余角色缓冲
        // 本节点产生的内部数据（派生数据集、pipeline 结果等）。
        let _flush_handle = worker.clone().spawn_flush_loop();

        let _probe_handle = crate::bootstrap::roles::health_probe::spawn_probe(
            core.store.clone(),
            probe.clone(),
            settings.object_store.health_probe_interval_secs,
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

        let js_executor: Option<Arc<dyn crate::app::intake::FunctionExecutor>> = {
            #[cfg(feature = "js-runtime")]
            {
                tracing::info!("JS function runtime enabled (deno_core compiled in)");

                Some(Arc::new(
                    crate::infra::runtime::JsFunctionExecutor::with_defaults(),
                ))
            }

            #[cfg(not(feature = "js-runtime"))]
            {
                None
            }
        };

        let llm_executor: Option<Arc<dyn crate::app::intake::FunctionExecutor>> =
            if settings.functions.llm_eval_enabled {
                tracing::info!("LLM eval function runtime enabled");

                Some(Arc::new(
                    crate::bootstrap::llm_executor::LlmFunctionExecutor::new(
                        model_providers,
                        agent_usage,
                    ),
                ))
            } else {
                None
            };

        let function_executor: Arc<dyn crate::app::intake::FunctionExecutor> =
            Arc::new(crate::infra::runtime::ChainedFunctionExecutor::new(
                vrl_executor,
                js_executor,
                llm_executor,
            ));

        let pipeline_engine = Arc::new(crate::app::intake::PipelineEngine::new(
            pipelines,
            functions,
            function_executor.clone(),
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
                        .add_hourly_intake_bytes(&org_id, received_at.0, bytes as i64)
                        .await
                    {
                        tracing::warn!(
                            org_id = %org_id.0,
                            error = %error,
                            "failed to record internal hourly intake usage"
                        );
                    }
                });
            },
        );

        let intake_service = IntakeService::new(worker.clone(), core.streams.clone())
            .with_system_org_id(core.system_org.id.clone())
            .with_pipeline(pipeline_engine)
            .with_realtime(crate::app::intake::realtime::RealtimeEngine {
                repository: Arc::new(
                    crate::infra::pipeline::realtime::PgRealtimePipelineRepository::new(
                        core.pool.clone(),
                    ),
                ),
                executor: function_executor.clone(),
            })
            .with_masking(core.masking_service.clone())
            .with_drain(core.drain_controller.clone())
            .with_realtime_alerts(core.realtime_alerts.clone())
            .with_internal_usage_recorder(internal_usage_recorder)
            .with_service_graph(Arc::new(ServiceGraphObserverImpl::new(
                service_graph_aggregator.clone(),
            )));

        // 没有 intake 角色的节点（router、alert 等）把公共入口收到的写入转发给 intake 节点。
        let intake_service = if core.roles.run_intake {
            intake_service
        } else {
            intake_service.with_forwarder(Arc::new(
                crate::infra::cluster::GrpcIntakeForwarder::new(core.registry.clone()),
            ))
        };

        let intake = Arc::new(intake_service);

        let profile_storage = Arc::new(ProfileStorageService::new(
            core.store.clone(),
            intake.clone(),
        ));

        let profiling_service = ProfilingService::new();

        let compactor = Arc::new(Compactor::new(
            file_catalog.clone(),
            core.object_reader.clone(),
            manifest_reader.clone(),
            parquet_writer,
            core.store.clone(),
            settings.compactor.clone(),
            settings.storage.gc.grace_period_secs,
        ));

        let investigation_blobs: Arc<
            dyn crate::infra::persistence::repositories::investigation_blobs::InvestigationBlobRepository,
        > = Arc::new(
            crate::infra::persistence::repositories::investigation_blobs::PgInvestigationBlobRepository::new(
                core.pool.clone(),
            ),
        );

        // 多个 compactor 节点按数据流、组织分担后台工作；新节点预热一个存活窗口后才接管。
        let compactor_ownership = core.roles.run_compactor.then(|| {
            let ownership = WorkOwnership::new(
                core.registry.clone(),
                core.node_id.clone(),
                PeerRole::Compactor,
                Duration::from_secs(u64::from(settings.cluster.peer_timeout_secs.max(1))),
            );

            let _watch = ownership.spawn_watch(Duration::from_secs(u64::from(
                settings.cluster.heartbeat_interval_secs.max(1),
            )));

            ownership
        });

        let _compactor_handle = compactor_ownership.clone().map(|ownership| {
            spawn_compactor_loop(
                compactor,
                core.orgs.clone() as Arc<dyn OrganizationRepository>,
                core.streams.clone() as Arc<dyn StreamRepository>,
                ownership,
                investigation_blobs.clone(),
                core.store.clone(),
                core.drain_controller.clone(),
            )
        });

        let _storage_maintenance_handles = compactor_ownership.map(|ownership| {
            storage_maintenance::spawn(
                Arc::new(IndexRebuildWorker::new(
                    file_catalog.clone(),
                    core.streams.clone(),
                    core.object_reader.clone(),
                    core.store.clone(),
                    settings.storage.index.clone(),
                    &settings.storage.gc,
                )),
                Arc::new(ObjectGcWorker::new(
                    file_catalog.clone(),
                    core.store.clone(),
                    settings.storage.gc.clone(),
                )),
                Arc::new(StorageReconciler::new(
                    file_catalog.clone(),
                    core.object_reader.clone(),
                    manifest_reader.clone(),
                    core.store.clone(),
                    settings.storage.reconciler.clone(),
                )),
                Arc::new(PartitionManifestManager::new(
                    file_catalog,
                    manifest_reader.clone(),
                    core.store.clone(),
                    settings.storage.catalog.clone(),
                    &settings.storage.gc,
                    settings.compactor.retention_days,
                )),
                core.orgs.clone() as Arc<dyn OrganizationRepository>,
                core.streams.clone() as Arc<dyn StreamRepository>,
                ownership,
                core.drain_controller.clone(),
            )
        });

        Ok(Self {
            probe,
            intake,
            profile_storage,
            profiling_service,
            prometheus_series_admission,
            function_executor,
            functions_js_runtime_enabled,
            service_graph_aggregator,
            usage,
            investigation_blobs,
            catalog_query,
            catalog_files,
        })
    }
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
