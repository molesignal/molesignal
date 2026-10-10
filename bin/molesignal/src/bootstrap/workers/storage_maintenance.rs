// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 存储维护循环：索引重建、延迟对象删除、目录对账与分区 manifest 维护。
//!
//! 多个 compactor 节点之间的分工（见 [`WorkOwnership`]）：
//! - 索引重建与目录对账按组织归属，分区 manifest 按数据流归属，各由 owner 节点执行；
//! - 延迟对象删除靠 GC 队列的租约认领去重，每个节点都执行。

use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};

use prometheus::IntGaugeVec;
use tokio::task::JoinHandle;

use crate::{
    app::cluster::WorkOwnership,
    domain::{
        iam::OrganizationRepository,
        storage::OrganizationScope,
        stream::{StreamDefinition, StreamRepository},
    },
    infra::storage::{
        index_rebuild::IndexRebuildWorker, manifest::PartitionManifestManager,
        object_gc::ObjectGcWorker, reconciler::StorageReconciler,
    },
    shared::{Result, drain::DrainController},
};

#[allow(clippy::too_many_arguments)]
pub fn spawn(
    indexes: Arc<IndexRebuildWorker>,
    gc: Arc<ObjectGcWorker>,
    reconciler: Arc<StorageReconciler>,
    manifests: Arc<PartitionManifestManager>,
    organizations: Arc<dyn OrganizationRepository>,
    streams: Arc<dyn StreamRepository>,
    ownership: Arc<WorkOwnership>,
    drain: Arc<DrainController>,
) -> Vec<JoinHandle<()>> {
    vec![
        spawn_index_loop(
            indexes,
            organizations.clone(),
            ownership.clone(),
            drain.clone(),
        ),
        spawn_gc_loop(gc, organizations.clone(), drain.clone()),
        spawn_reconciler_loop(
            reconciler,
            organizations.clone(),
            ownership.clone(),
            drain.clone(),
        ),
        spawn_manifest_loop(manifests, organizations, streams, ownership, drain),
    ]
}

fn spawn_index_loop(
    worker: Arc<IndexRebuildWorker>,
    organizations: Arc<dyn OrganizationRepository>,
    ownership: Arc<WorkOwnership>,
    drain: Arc<DrainController>,
) -> JoinHandle<()> {
    let interval = Duration::from_secs(worker.settings().interval_secs.max(1) as u64);

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.tick().await;

        loop {
            ticker.tick().await;

            if drain.is_draining() {
                break;
            }

            let result = for_each_scope(
                "index_rebuild",
                organizations.as_ref(),
                Some(ownership.as_ref()),
                |scope| worker.run_scope(scope),
            )
            .await;

            record_health("index_rebuild", &result);
        }
    })
}

fn spawn_gc_loop(
    worker: Arc<ObjectGcWorker>,
    organizations: Arc<dyn OrganizationRepository>,
    drain: Arc<DrainController>,
) -> JoinHandle<()> {
    let interval = Duration::from_secs(worker.settings().interval_secs.max(1) as u64);

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.tick().await;

        loop {
            ticker.tick().await;

            if drain.is_draining() {
                break;
            }

            // GC 条目逐条租约认领，多个节点同时扫同一个组织也不会重复删除。
            let result = for_each_scope("object_gc", organizations.as_ref(), None, |scope| {
                worker.run_scope(scope)
            })
            .await;

            record_health("object_gc", &result);
        }
    })
}

fn spawn_reconciler_loop(
    worker: Arc<StorageReconciler>,
    organizations: Arc<dyn OrganizationRepository>,
    ownership: Arc<WorkOwnership>,
    drain: Arc<DrainController>,
) -> JoinHandle<()> {
    let interval = Duration::from_secs(worker.settings().interval_secs.max(1) as u64);

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.tick().await;

        loop {
            ticker.tick().await;

            if drain.is_draining() {
                break;
            }

            let result = for_each_scope(
                "storage_reconciler",
                organizations.as_ref(),
                Some(ownership.as_ref()),
                |scope| worker.run_scope(scope),
            )
            .await;

            record_health("storage_reconciler", &result);
        }
    })
}

fn spawn_manifest_loop(
    manager: Arc<PartitionManifestManager>,
    organizations: Arc<dyn OrganizationRepository>,
    streams: Arc<dyn StreamRepository>,
    ownership: Arc<WorkOwnership>,
    drain: Arc<DrainController>,
) -> JoinHandle<()> {
    let interval = Duration::from_secs(manager.settings().interval_secs.max(1) as u64);

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.tick().await;

        loop {
            ticker.tick().await;

            if drain.is_draining() {
                break;
            }

            let manifests = manager.as_ref();

            let result = for_each_stream(
                "partition_manifest",
                organizations.as_ref(),
                streams.as_ref(),
                ownership.as_ref(),
                |stream| async move { manifests.maintain_stream(&stream).await },
            )
            .await;

            record_health("partition_manifest", &result);
        }
    })
}

/// 对每个组织执行一次 `operation`。给了 `ownership` 时只处理本节点拥有的组织。
#[tracing::instrument(
    name = "worker.storage_maintenance",
    parent = None,
    skip_all,
    fields(otel.kind = "internal", molesignal.worker.name = worker_name)
)]
async fn for_each_scope<F, Fut>(
    worker_name: &'static str,
    organizations: &dyn OrganizationRepository,
    ownership: Option<&WorkOwnership>,
    mut operation: F,
) -> Result<usize>
where
    F: FnMut(OrganizationScope) -> Fut,
    Fut: std::future::Future<Output = Result<usize>>,
{
    let mut completed = 0;
    let mut units = 0;

    for organization in organizations.list().await? {
        if let Some(ownership) = ownership
            && !ownership.owns(organization.id.as_str()).await
        {
            continue;
        }

        units += 1;
        completed += operation(OrganizationScope::new(organization.id)).await?;
    }

    if let Some(ownership) = ownership {
        ownership.record_owned(worker_name, units);
    }

    Ok(completed)
}

/// 对本节点拥有的每个数据流执行一次 `operation`。
#[tracing::instrument(
    name = "worker.storage_maintenance",
    parent = None,
    skip_all,
    fields(otel.kind = "internal", molesignal.worker.name = worker_name)
)]
async fn for_each_stream<F, Fut>(
    worker_name: &'static str,
    organizations: &dyn OrganizationRepository,
    streams: &dyn StreamRepository,
    ownership: &WorkOwnership,
    mut operation: F,
) -> Result<usize>
where
    F: FnMut(StreamDefinition) -> Fut,
    Fut: std::future::Future<Output = Result<usize>>,
{
    let mut completed = 0;
    let mut units = 0;

    for organization in organizations.list().await? {
        for stream in streams.list(&organization.id).await? {
            if !ownership.owns(stream.id.as_str()).await {
                continue;
            }

            units += 1;
            completed += operation(stream).await?;
        }
    }

    ownership.record_owned(worker_name, units);
    Ok(completed)
}

fn record_health(name: &'static str, result: &Result<usize>) {
    worker_health()
        .with_label_values(&[name])
        .set(if result.is_ok() { 1 } else { 0 });

    match result {
        Ok(completed) => tracing::debug!(worker = name, completed, "storage worker tick completed"),
        Err(error) => tracing::warn!(worker = name, %error, "storage worker tick failed"),
    }
}

fn worker_health() -> &'static IntGaugeVec {
    static METRIC: OnceLock<IntGaugeVec> = OnceLock::new();

    METRIC.get_or_init(|| {
        crate::shared::metrics::register_int_gauge_vec(
            "storage_worker_healthy",
            "Last storage maintenance tick status",
            &["worker"],
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::cluster::{PeerRole, ownership::test_support::Nodes},
        bootstrap::test_support::{FixedOrganizations, FixedStreams, tenants},
    };

    const NODE_IDS: [&str; 2] = ["compactor-0", "compactor-1"];

    fn cluster() -> Arc<Nodes> {
        Nodes::of(&NODE_IDS.map(|node_id| (node_id, PeerRole::Compactor)))
    }

    fn member(nodes: &Arc<Nodes>, node_id: &str) -> Arc<WorkOwnership> {
        WorkOwnership::new(nodes.clone(), node_id, PeerRole::Compactor, Duration::ZERO)
    }

    async fn organizations_handled(
        organizations: &FixedOrganizations,
        ownership: Option<&WorkOwnership>,
    ) -> Vec<String> {
        let mut handled = Vec::new();
        for_each_scope("index_rebuild", organizations, ownership, |scope| {
            handled.push(scope.organization_id.as_str().to_owned());
            async { Ok(1) }
        })
        .await
        .unwrap();
        handled
    }

    async fn streams_handled(
        organizations: &FixedOrganizations,
        streams: &FixedStreams,
        ownership: &WorkOwnership,
    ) -> Vec<String> {
        let mut handled = Vec::new();
        for_each_stream(
            "partition_manifest",
            organizations,
            streams,
            ownership,
            |stream| {
                handled.push(stream.id.as_str().to_owned());
                async { Ok(1) }
            },
        )
        .await
        .unwrap();
        handled
    }

    fn sorted(mut ids: Vec<String>) -> Vec<String> {
        ids.sort();
        ids
    }

    #[tokio::test]
    async fn owned_organization_work_runs_on_exactly_one_node() {
        let nodes = cluster();
        let (organizations, _) = tenants(40, 0);
        let mut handled = Vec::new();
        for node_id in NODE_IDS {
            let share = organizations_handled(&organizations, Some(&member(&nodes, node_id))).await;
            assert!(!share.is_empty(), "{node_id} owns no organization");
            handled.extend(share);
        }
        let all = organizations
            .0
            .iter()
            .map(|org| org.id.as_str().to_owned())
            .collect();
        assert_eq!(sorted(handled), sorted(all));
    }

    #[tokio::test]
    async fn owned_stream_work_runs_on_exactly_one_node() {
        let nodes = cluster();
        let (organizations, streams) = tenants(4, 10);
        let mut handled = Vec::new();
        for node_id in NODE_IDS {
            let share = streams_handled(&organizations, &streams, &member(&nodes, node_id)).await;
            assert!(!share.is_empty(), "{node_id} owns no stream");
            handled.extend(share);
        }
        let all = streams
            .0
            .iter()
            .map(|stream| stream.id.as_str().to_owned())
            .collect();
        assert_eq!(sorted(handled), sorted(all));
    }

    #[tokio::test]
    async fn a_lone_node_does_all_owned_work_and_an_absent_node_does_none() {
        let nodes = Nodes::of(&[(NODE_IDS[0], PeerRole::Compactor)]);
        let (organizations, streams) = tenants(4, 10);

        let present = member(&nodes, NODE_IDS[0]);
        assert_eq!(
            organizations_handled(&organizations, Some(&present))
                .await
                .len(),
            4
        );
        assert_eq!(
            streams_handled(&organizations, &streams, &present)
                .await
                .len(),
            40
        );

        let absent = member(&nodes, NODE_IDS[1]);
        assert!(
            organizations_handled(&organizations, Some(&absent))
                .await
                .is_empty()
        );
        assert!(
            streams_handled(&organizations, &streams, &absent)
                .await
                .is_empty()
        );
    }

    #[tokio::test]
    async fn work_without_ownership_runs_on_every_node() {
        let (organizations, _) = tenants(4, 0);
        assert_eq!(organizations_handled(&organizations, None).await.len(), 4);
    }
}
