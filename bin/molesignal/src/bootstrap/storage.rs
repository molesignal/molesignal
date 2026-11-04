// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 摄取、WAL、对象存储写入与 compactor 运行时装配。

use std::{sync::Arc, time::Duration};

use super::core::Core;
use crate::{
    app::{
        agent_usage::AgentUsage,
        cluster::{PeerRole, WorkOwnership},
        intake::{
            IntakeService,
            realtime::StepState,
            usage::{IntakeReceipt, IntakeUsageMeter},
        },
        profile_storage::ProfileStorageService,
        profiling::ProfilingService,
    },
    bootstrap::{
        roles::{compactor::spawn_compactor_loop, intake::IntakeWorker},
        workers::{
            audit_retention::AuditRetentionWorker, intake_usage, pipeline_steps, series_limits,
            storage_maintenance, synthetic_retention::SyntheticRetentionWorker,
        },
    },
    config::{Settings, WalFlushStrategy, WalSettings, WalSyncLevel},
    domain::{
        iam::{
            DEFAULT_MAX_ACTIVE_SERIES_PER_METRIC, DEFAULT_MAX_ACTIVE_SERIES_PER_ORG,
            InstanceSettingsRepository, OrganizationRepository,
        },
        intake::usage::source::IntakeUsageRepository,
        storage::{FileCatalog, QueryFileSource, builtin_registry},
        stream::StreamRepository,
    },
    infra::{
        intake::{BufferPool, DatasetResolver, PrometheusSeriesAdmission, WalPool},
        persistence::repositories::{
            agent::model_providers::ModelProviderRepository,
            audit_events::PgAuditEventRepository,
            file_catalog::PgFileCatalog,
            instance_settings::PgInstanceSettingsRepository,
            rum_applications::PgRumApplicationRepository,
            rum_issues::{PgRumIssueRepository, PgRumVersionRepository},
            synthetics::PgSyntheticRepository,
            usage::PgIntakeUsageRepository,
        },
        pipeline::realtime::limit_usage::PgLimitUsageLedger,
        query::catalog_source::CatalogQuerySource,
        rum::{issues::IssueTracker, session_state::SessionMerger},
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
    shared::{Error, Result, health::Probe, ids::Id},
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
    pub(super) usage: Arc<dyn IntakeUsageRepository>,
    pub(super) intake_meter: Arc<IntakeUsageMeter>,
    /// What realtime pipelines' metric, quota and throttle steps hold between events.
    pub(super) pipeline_steps: Arc<StepState>,
    pub(super) investigation_blobs: Arc<
        dyn crate::infra::persistence::repositories::investigation_blobs::InvestigationBlobRepository,
    >,
    pub(super) catalog_query: Arc<CatalogQuerySource>,
    pub(super) catalog_files: Arc<dyn QueryFileSource>,
    /// Filled with the query service once it is built: sessions are rebuilt by reading back.
    pub(super) rum_query: Arc<super::rum::QuerySlot>,
}

impl StorageRuntime {
    pub(super) async fn build(
        settings: &Settings,
        core: &Core,
        model_providers: Arc<dyn ModelProviderRepository>,
        agent_usage: Arc<AgentUsage>,
    ) -> Result<Self> {
        let segment_bytes = (settings.wal.segment_size as usize)
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
            (settings.intake.max_buffer_memory_size as usize).saturating_mul(1024 * 1024),
        ));

        // 组织与指标的上限是平台设置：先用默认值，读到平台设置后换上，之后定期刷新。
        let prometheus_series_admission = Arc::new(PrometheusSeriesAdmission::new(
            settings.intake.prometheus.cardinality.clone(),
            DEFAULT_MAX_ACTIVE_SERIES_PER_ORG as usize,
            DEFAULT_MAX_ACTIVE_SERIES_PER_METRIC as usize,
        ));

        let _series_limits_handle = series_limits::spawn(
            prometheus_series_admission.clone(),
            Arc::new(PgInstanceSettingsRepository::new(core.pool.clone())),
            core.drain_controller.clone(),
        );

        let parquet_writer = Arc::new(ParquetWriter::new(core.store.clone()));
        let probe = Arc::new(Probe::new());

        let file_catalog: Arc<dyn FileCatalog> = Arc::new(PgFileCatalog::new(core.pool.clone()));

        let manifest_reader = Arc::new(PartitionManifestReader::new(
            core.object_reader.clone(),
            settings.storage.catalog.manifest_cache_bytes(),
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
            (settings.wal.max_replay_size.max(1) as usize).saturating_mul(1024 * 1024);

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

        let functions_js_runtime_enabled = cfg!(feature = "js-runtime");

        let service_graph_aggregator = Arc::new(ServiceGraphAggregator::new());
        let usage: Arc<dyn IntakeUsageRepository> =
            Arc::new(PgIntakeUsageRepository::new(core.pool.clone()));
        let intake_meter = Arc::new(IntakeUsageMeter::new(usage.clone()));

        let _intake_meter_handle = intake_usage::spawn_meter(
            intake_meter.clone(),
            Arc::new(PgInstanceSettingsRepository::new(core.pool.clone())),
        );

        let internal_usage = intake_meter.clone();

        let internal_usage_recorder =
            Arc::new(move |org_id: &Id, receipt: IntakeReceipt, accepted: u64| {
                internal_usage.record(org_id, receipt, accepted);
            });

        let rum_query = Arc::new(super::rum::QuerySlot::default());

        let rum_sessions = Arc::new(SessionMerger::new(Arc::new(
            super::rum::SqlSessionHistory::new(rum_query.clone()),
        )));

        let rum_issues = Arc::new(IssueTracker::new(
            Arc::new(PgRumIssueRepository::new(core.pool.clone())),
            Arc::new(PgRumVersionRepository::new(core.pool.clone())),
            Arc::new(PgRumApplicationRepository::new(core.pool.clone())),
        ));

        let pipeline_steps = Arc::new(StepState::new(Arc::new(PgLimitUsageLedger::new(
            core.pool.clone(),
        ))));

        let intake_service = IntakeService::new(worker.clone(), core.streams.clone())
            .with_system_org_id(core.system_org.id.clone())
            .with_metric_directory(core.metric_directory.clone())
            .with_rum_sessions(rum_sessions)
            .with_rum_issues(rum_issues)
            .with_realtime(crate::app::intake::realtime::RealtimeEngine {
                repository: Arc::new(
                    crate::infra::pipeline::realtime::PgRealtimePipelineRepository::new(
                        core.pool.clone(),
                    ),
                ),
                executor: function_executor.clone(),
                steps: pipeline_steps.clone(),
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
        let _pipeline_steps_handle = pipeline_steps::spawn(pipeline_steps.clone(), intake.clone());

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

        // 数据流没有设置保留天数时的天数在平台设置里，compactor 与清单维护每一轮读一次。
        let instance_settings: Arc<dyn InstanceSettingsRepository> =
            Arc::new(PgInstanceSettingsRepository::new(core.pool.clone()));

        let _compactor_handle = compactor_ownership.clone().map(|ownership| {
            spawn_compactor_loop(
                compactor,
                core.orgs.clone() as Arc<dyn OrganizationRepository>,
                core.streams.clone() as Arc<dyn StreamRepository>,
                core.metric_directory.clone(),
                instance_settings.clone(),
                ownership,
                investigation_blobs.clone(),
                core.store.clone(),
                core.drain_controller.clone(),
            )
        });

        // 审计记录保留：compactor 主节点每天删除过了保留期的审计记录。
        let _audit_retention_handle = compactor_ownership.clone().map(|ownership| {
            AuditRetentionWorker::new(
                Arc::new(PgAuditEventRepository::new(core.pool.clone())),
                Arc::new(PgInstanceSettingsRepository::new(core.pool.clone())),
                ownership,
                core.drain_controller.clone(),
            )
            .spawn()
        });

        // 接入用量保留：compactor 主节点每天删除过了保留期的小时记录。
        let _intake_usage_retention_handle = compactor_ownership.clone().map(|ownership| {
            intake_usage::spawn_retention(
                usage.clone(),
                instance_settings.clone(),
                ownership,
                core.drain_controller.clone(),
            )
        });

        // 拨测结果保留：compactor 主节点每天删除过了保留期的结果、过期的附件与已结束的任务。
        let _synthetic_retention_handle = compactor_ownership.clone().map(|ownership| {
            SyntheticRetentionWorker::new(
                Arc::new(PgSyntheticRepository::new(
                    core.pool.clone(),
                    core.cipher_root_key.clone(),
                )),
                instance_settings.clone(),
                core.store.clone(),
                ownership,
                core.drain_controller.clone(),
            )
            .spawn()
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
                    core.object_reader.clone(),
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
                )),
                core.orgs.clone() as Arc<dyn OrganizationRepository>,
                core.streams.clone() as Arc<dyn StreamRepository>,
                instance_settings,
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
            intake_meter,
            pipeline_steps,
            investigation_blobs,
            catalog_query,
            catalog_files,
            rum_query,
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
