// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};

use prometheus::IntGaugeVec;
use tokio::task::JoinHandle;

use crate::{
    domain::{iam::OrganizationRepository, storage::OrganizationScope, stream::StreamRepository},
    infra::storage::{
        index_rebuild::IndexRebuildWorker, manifest::PartitionManifestManager,
        object_gc::ObjectGcWorker, reconciler::StorageReconciler,
    },
    shared::{Result, drain::DrainController},
};

pub fn spawn(
    indexes: Arc<IndexRebuildWorker>,
    gc: Arc<ObjectGcWorker>,
    reconciler: Arc<StorageReconciler>,
    manifests: Arc<PartitionManifestManager>,
    organizations: Arc<dyn OrganizationRepository>,
    streams: Arc<dyn StreamRepository>,
    drain: Arc<DrainController>,
) -> Vec<JoinHandle<()>> {
    vec![
        spawn_index_loop(indexes, organizations.clone(), drain.clone()),
        spawn_gc_loop(gc, organizations.clone(), drain.clone()),
        spawn_reconciler_loop(reconciler, organizations.clone(), drain.clone()),
        spawn_manifest_loop(manifests, organizations, streams, drain),
    ]
}

fn spawn_index_loop(
    worker: Arc<IndexRebuildWorker>,
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
            let result = for_each_scope("index_rebuild", organizations.as_ref(), |scope| {
                worker.run_scope(scope)
            })
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
            let result = for_each_scope("object_gc", organizations.as_ref(), |scope| {
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
            let result = for_each_scope("storage_reconciler", organizations.as_ref(), |scope| {
                worker.run_scope(scope)
            })
            .await;
            record_health("storage_reconciler", &result);
        }
    })
}

fn spawn_manifest_loop(
    manager: Arc<PartitionManifestManager>,
    organizations: Arc<dyn OrganizationRepository>,
    streams: Arc<dyn StreamRepository>,
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
            let result =
                maintain_manifests(&manager, organizations.as_ref(), streams.as_ref()).await;
            record_health("partition_manifest", &result);
        }
    })
}

#[tracing::instrument(
    name = "worker.storage_maintenance",
    parent = None,
    skip_all,
    fields(otel.kind = "internal", molesignal.worker.name = worker_name)
)]
async fn for_each_scope<F, Fut>(
    worker_name: &'static str,
    organizations: &dyn OrganizationRepository,
    mut operation: F,
) -> Result<usize>
where
    F: FnMut(OrganizationScope) -> Fut,
    Fut: std::future::Future<Output = Result<usize>>,
{
    let mut completed = 0;
    for organization in organizations.list().await? {
        completed += operation(OrganizationScope::new(organization.id)).await?;
    }
    Ok(completed)
}

#[tracing::instrument(
    name = "worker.storage_maintenance",
    parent = None,
    skip_all,
    fields(otel.kind = "internal", molesignal.worker.name = "partition_manifest")
)]
async fn maintain_manifests(
    manager: &PartitionManifestManager,
    organizations: &dyn OrganizationRepository,
    streams: &dyn StreamRepository,
) -> Result<usize> {
    let mut switched = 0;
    for organization in organizations.list().await? {
        for stream in streams.list(&organization.id).await? {
            switched += manager.maintain_stream(&stream).await?;
        }
    }
    Ok(switched)
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
