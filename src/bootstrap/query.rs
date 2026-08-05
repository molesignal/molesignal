// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 本地、分布式与联邦查询运行时装配。

use std::sync::Arc;

use super::{core::Core, iam::IamRuntime};
use crate::{
    app::{cluster::PeerRole, query::QueryService},
    config::Settings,
    domain::query::QueryEngine,
    infra::{
        caching::QueryResultCache,
        cluster::{
            ClusterSecretRepository, PgClusterSecretRepository, PgRemoteClustersRepository,
            RemoteClustersRepository,
        },
        persistence::repositories::cluster::events::{
            ClusterEventOutboxRepository, ClusterOrgLinkRepository,
            ClusterResourceVersionRepository, PgClusterEventOutboxRepository,
            PgClusterOrgLinkRepository, PgClusterResourceVersionRepository, PgSeenEventsRepository,
            SeenEventsRepository,
        },
        query::{
            distributed::DistributedDataFusionEngine, federated::FederatedDistributedEngine,
            promql::PromQLEngine, tantivy_pruner::TantivyPruner,
        },
        search::{datafusion_engine::DataFusionEngine, tantivy_index::IndexHandle},
    },
};

pub(super) struct QueryRuntime {
    pub(super) query: Arc<QueryService>,
    pub(super) sql_engine: Arc<dyn QueryEngine>,
    pub(super) remote_clusters: Arc<dyn RemoteClustersRepository>,
    pub(super) cluster_event_outbox: Arc<dyn ClusterEventOutboxRepository>,
    pub(super) cluster_resource_version: Arc<dyn ClusterResourceVersionRepository>,
    pub(super) cluster_org_link: Arc<dyn ClusterOrgLinkRepository>,
    pub(super) seen_events: Arc<dyn SeenEventsRepository>,
    pub(super) federation_cancel:
        Arc<crate::infra::query::federation_cancel::FederationCancelRegistry>,
    pub(super) cluster_secrets: Arc<dyn ClusterSecretRepository>,
}

impl QueryRuntime {
    pub(super) async fn build(settings: &Settings, core: &Core) -> Self {
        let index_handle_cache = Arc::new(crate::infra::caching::ParquetMetaCache::<
            Arc<IndexHandle>,
        >::new(settings.cache.parquet_meta.clone()));
        let tantivy_pruner = Arc::new(TantivyPruner::new(index_handle_cache, core.store.clone()));
        let log_patterns: Arc<
            dyn crate::infra::persistence::repositories::log_patterns::LogPatternRepository,
        > = Arc::new(
            crate::infra::persistence::repositories::log_patterns::PgLogPatternRepository::new(
                core.pool.clone(),
            ),
        );
        let local_engine = Arc::new(
            DataFusionEngine::new(core.parquet_file_meta.clone(), core.store.clone())
                .with_tantivy_pruner(tantivy_pruner)
                .with_streams(core.streams.clone())
                .with_log_patterns(log_patterns.clone())
                .with_regex_patterns(core.regex_patterns.clone())
                .with_max_result_rows(settings.search.max_result_rows)
                .with_field_keys(core.field_key_service.clone()),
        );

        core.spawn_cluster_maintenance(settings);

        let peers = core.registry.list_role(PeerRole::Querier).await;
        let sql_engine: Arc<dyn QueryEngine> = if peers.len() >= 2 {
            Arc::new(DistributedDataFusionEngine::new(
                local_engine.clone(),
                core.registry.clone(),
                core.parquet_file_meta.clone(),
                core.store.clone(),
            ))
        } else {
            local_engine
        };

        let remote_clusters: Arc<dyn RemoteClustersRepository> =
            Arc::new(PgRemoteClustersRepository::new(core.pool.clone()));
        let cluster_event_outbox: Arc<dyn ClusterEventOutboxRepository> =
            Arc::new(PgClusterEventOutboxRepository::new(core.pool.clone()));
        let cluster_resource_version: Arc<dyn ClusterResourceVersionRepository> =
            Arc::new(PgClusterResourceVersionRepository::new(core.pool.clone()));
        let cluster_org_link: Arc<dyn ClusterOrgLinkRepository> =
            Arc::new(PgClusterOrgLinkRepository::new(core.pool.clone()));
        let seen_events: Arc<dyn SeenEventsRepository> =
            Arc::new(PgSeenEventsRepository::new(core.pool.clone()));
        let federation_cancel =
            Arc::new(crate::infra::query::federation_cancel::FederationCancelRegistry::new());
        let cluster_secrets: Arc<dyn ClusterSecretRepository> = Arc::new(
            PgClusterSecretRepository::new(core.pool.clone(), core.cipher_root_key.clone()),
        );
        let federated_engine: Arc<dyn QueryEngine> = Arc::new(
            FederatedDistributedEngine::new(
                sql_engine.clone(),
                remote_clusters.clone(),
                core.parquet_file_meta.clone(),
                core.store.clone(),
            )
            .with_secrets(cluster_secrets.clone())
            .with_field_keys(core.field_key_service.clone())
            .with_regex_patterns(core.regex_patterns.clone())
            .with_log_patterns(log_patterns)
            .with_cancel_registry(federation_cancel.clone())
            .with_streams(core.streams.clone()),
        );
        let promql_engine = {
            let engine = PromQLEngine::new(core.parquet_file_meta.clone(), core.store.clone())
                .with_streams(core.streams.clone());
            let settings = &settings.search.stream_agg_cache;
            let engine = if settings.capacity > 0 {
                let cache = Arc::new(crate::infra::caching::StreamingAggCache::new(settings));
                engine.with_streaming_cache(
                    cache,
                    std::time::Duration::from_secs(settings.safe_lookback_secs),
                )
            } else {
                engine
            };
            Arc::new(engine)
        };
        let admission = Arc::new(crate::app::search::AdmissionController::new(
            crate::app::search::AdmissionConfig {
                default_max_concurrent: settings.search.admission.default_max_concurrent,
                groups: settings.search.admission.groups.clone(),
                role_map: settings.search.admission.role_map.clone(),
                cluster_default_max_concurrent: settings
                    .search
                    .admission
                    .cluster_default_max_concurrent,
                cluster_groups: settings.search.admission.cluster_groups.clone(),
            },
        ));
        let query_result_cache =
            Arc::new(QueryResultCache::new(settings.cache.query_result.clone()));
        let mut query_service = QueryService::new(federated_engine, promql_engine, admission);
        if settings.cache.query_result.capacity > 0 {
            query_service = query_service.with_result_cache(query_result_cache);
        }
        let query = Arc::new(query_service);

        let cluster_admission_enabled = settings.search.admission.cluster_default_max_concurrent
            > 0
            || !settings.search.admission.cluster_groups.is_empty();
        let _admission_sync = (core.roles.run_querier && cluster_admission_enabled).then(|| {
            let repository: Arc<
                dyn crate::infra::persistence::repositories::search::admission_load::SearchAdmissionLoadRepository,
            > = Arc::new(
                crate::infra::persistence::repositories::search::admission_load::PgSearchAdmissionLoadRepository::new(
                    core.pool.clone(),
                ),
            );
            crate::bootstrap::workers::admission_load_sync::AdmissionLoadSync::new(
                core.node_id.clone(),
                query.admission(),
                repository,
                settings.search.admission.cluster_sync_interval_secs,
                settings.search.admission.cluster_stale_secs,
            )
            .spawn()
        });

        Self {
            query,
            sql_engine,
            remote_clusters,
            cluster_event_outbox,
            cluster_resource_version,
            cluster_org_link,
            seen_events,
            federation_cancel,
            cluster_secrets,
        }
    }

    pub(super) fn spawn_federation_workers(
        &self,
        settings: &Settings,
        core: &Core,
        iam: &IamRuntime,
    ) {
        let _cluster_event_sync = core.roles.run_alert_manager.then(|| {
            crate::bootstrap::workers::cluster::event_sync::ClusterEventSync::new(
                iam.instance_settings.clone(),
                self.remote_clusters.clone(),
                self.cluster_org_link.clone(),
                self.cluster_event_outbox.clone(),
                self.seen_events.clone(),
                self.cluster_secrets.clone(),
                (settings.grpc.max_message_size_mb as usize).saturating_mul(1024 * 1024),
            )
            .spawn()
        });
        let _cluster_gossip = core.roles.run_alert_manager.then(|| {
            crate::bootstrap::workers::cluster::gossip::ClusterGossip::new(
                iam.instance_settings.clone(),
                self.remote_clusters.clone(),
                self.cluster_org_link.clone(),
                self.cluster_secrets.clone(),
            )
            .spawn()
        });
    }
}
