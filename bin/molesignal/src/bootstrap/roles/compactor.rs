// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! compactor 角色：周期合并小 parquet + retention 扫描。
//!
//! - 每 `interval_secs` 一次 tick：遍历所有 org / stream，逐个 `sweep_one + retention_sweep`。
//! - 多个 compactor 节点按数据流分担：每个数据流只由它的 owner 节点处理（见
//!   [`WorkOwnership`]），合并、降采样、retention 因此始终在同一个节点上依次执行。
//! - 并发上限 `max_concurrent_groups` 用 `tokio::sync::Semaphore` 控制（同一 tick 内
//!   并行处理多个 stream，单 stream 内部仍串行）。

use std::{sync::Arc, time::Duration};

use object_store::{ObjectStore, ObjectStoreExt, path::Path as ObjPath};
use tokio::{sync::Semaphore, task::JoinHandle};

use crate::{
    app::cluster::WorkOwnership,
    domain::{iam::OrganizationRepository, stream::StreamRepository},
    infra::{
        persistence::repositories::investigation_blobs::InvestigationBlobRepository,
        storage::compactor::Compactor,
    },
    shared::{
        drain::DrainController,
        time::{TimeRange, TimestampMicros},
    },
};

/// 投资栈 ephemeral payload 保留 7 天（web-investigation-shell）。
const INVESTIGATION_BLOB_TTL_SECS: i64 = 7 * 86_400;

/// 归属指标里这项工作的名字。
const WORK: &str = "compaction";

/// 启动后台 compactor tick 任务。
///
/// 返回 `JoinHandle` 由调用方持有（standalone 模式下 bootstrap 阶段 spawn 即可）。
pub fn spawn_compactor_loop(
    compactor: Arc<Compactor>,
    orgs: Arc<dyn OrganizationRepository>,
    streams: Arc<dyn StreamRepository>,
    ownership: Arc<WorkOwnership>,
    investigation_blobs: Arc<dyn InvestigationBlobRepository>,
    object_store: Arc<dyn ObjectStore>,
    drain: Arc<DrainController>,
) -> JoinHandle<()> {
    let settings = compactor.settings();
    let max = settings.max_concurrent_groups.max(1) as usize;
    let sem = Arc::new(Semaphore::new(max));
    let interval = Duration::from_secs(settings.interval_secs.max(1) as u64);
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        // 第一次 tick 立刻触发（spec 没强制 skip），但为避免与 intake replay 抢资源稍微 delay 一下
        ticker.tick().await;
        let mut blob_sweep_at = TimestampMicros::now().0;
        loop {
            ticker.tick().await;
            // 节点退役：停接新活——结束 compaction loop（合并/降采样可中断，下次启动续做）。
            if drain.is_draining() {
                tracing::info!("compactor draining; stopping compaction loop");
                break;
            }
            match run_once(
                &compactor,
                orgs.as_ref(),
                streams.as_ref(),
                &ownership,
                &sem,
            )
            .await
            {
                Ok(owned) => ownership.record_owned(WORK, owned),
                Err(e) => tracing::warn!(error = %e, "compactor tick failed"),
            }
            // 投资栈 blob retention 一天一次即可
            let now = TimestampMicros::now().0;
            if now - blob_sweep_at >= 86_400 * 1_000_000 {
                blob_sweep_at = now;
                let cutoff = now - INVESTIGATION_BLOB_TTL_SECS * 1_000_000;
                match investigation_blobs.delete_older_than(cutoff).await {
                    Ok(keys) if !keys.is_empty() => {
                        tracing::info!(rows = keys.len(), "swept investigation blobs");
                        // 顺带清掉对象存储里对应的 blob 对象，避免孤儿（best-effort）。
                        for key in keys {
                            if let Ok(path) = ObjPath::parse(&key) {
                                let _ = object_store.delete(&path).await;
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!(error = %e, "investigation_blobs sweep failed"),
                }
            }
        }
    })
}

/// 处理本节点拥有的全部数据流，返回其数量。
async fn run_once(
    compactor: &Arc<Compactor>,
    orgs: &dyn OrganizationRepository,
    streams: &dyn StreamRepository,
    ownership: &WorkOwnership,
    sem: &Arc<Semaphore>,
) -> anyhow::Result<usize> {
    let org_list = orgs.list().await?;
    let now = TimestampMicros::now();
    let mut handles = Vec::new();
    for org in org_list {
        let stream_list = streams.list(&org.id).await?;
        for stream in stream_list {
            let permit = sem.clone().acquire_owned().await?;
            // 拿到并发名额后再判定归属：排在前面的数据流可能跑了很久，成员视图已经变了。
            if !ownership.owns(stream.id.as_str()).await {
                continue;
            }
            let compactor = compactor.clone();
            let stream = stream.clone();
            let h = tokio::spawn(async move {
                let _permit = permit;
                let retention_days =
                    stream.effective_retention_days(compactor.settings().retention_days);
                let lookback =
                    TimestampMicros(now.0.saturating_sub(
                        i64::from(retention_days).saturating_mul(86_400 * 1_000_000),
                    ));
                if let Err(e) = compactor
                    .sweep_one(&stream, TimeRange::new(lookback, now))
                    .await
                {
                    tracing::warn!(stream = %stream.name, error = %e, "sweep_one failed");
                }
                // 降采样：把早于阈值的 metrics 预聚合成时间桶（关闭时 no-op）。
                if let Err(e) = compactor.downsample_sweep(&stream).await {
                    tracing::warn!(stream = %stream.name, error = %e, "downsample_sweep failed");
                }
                // Retention runs after derivation so an aggressive cutoff cannot remove raw
                // inputs before an enabled raw-to-rollup transform has a chance to publish.
                if let Err(e) = compactor.retention_sweep(&stream).await {
                    tracing::warn!(stream = %stream.name, error = %e, "retention_sweep failed");
                }
            });
            handles.push(h);
        }
    }
    let owned = handles.len();
    for h in handles {
        let _ = h.await;
    }
    Ok(owned)
}

#[cfg(test)]
mod tests {
    use object_store::memory::InMemory;

    use super::*;
    use crate::{
        app::cluster::{PeerRole, ownership::test_support::Nodes},
        bootstrap::test_support::tenants,
        config::{CompactorSettings, ObjectCacheSettings},
        infra::{
            intake::dataset_resolver::test_support::StubFileCatalog,
            storage::{
                manifest::PartitionManifestReader, object_reader::ObjectReader,
                parquet::writer::ParquetWriter,
            },
        },
    };

    fn compactor() -> Arc<Compactor> {
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let object_reader =
            ObjectReader::build(store.clone(), "local", &ObjectCacheSettings::default()).unwrap();
        Arc::new(Compactor::new(
            Arc::new(StubFileCatalog::default()),
            object_reader.clone(),
            Arc::new(PartitionManifestReader::new(object_reader, 8 * 1024 * 1024)),
            Arc::new(ParquetWriter::new(store.clone())),
            store,
            CompactorSettings::default(),
            3600,
        ))
    }

    async fn swept_by(nodes: &Arc<Nodes>, node_id: &str) -> usize {
        let (orgs, streams) = tenants(4, 10);
        let ownership =
            WorkOwnership::new(nodes.clone(), node_id, PeerRole::Compactor, Duration::ZERO);
        run_once(
            &compactor(),
            &orgs,
            &streams,
            &ownership,
            &Arc::new(Semaphore::new(4)),
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn a_lone_compactor_sweeps_every_stream() {
        let nodes = Nodes::of(&[("compactor-0", PeerRole::Compactor)]);
        assert_eq!(swept_by(&nodes, "compactor-0").await, 40);
        // 不在成员视图里的节点已被接管，不再处理任何数据流。
        assert_eq!(swept_by(&nodes, "compactor-1").await, 0);
    }

    #[tokio::test]
    async fn compactors_split_the_streams_between_them() {
        let nodes = Nodes::of(&[
            ("compactor-0", PeerRole::Compactor),
            ("compactor-1", PeerRole::Compactor),
        ]);
        let first = swept_by(&nodes, "compactor-0").await;
        let second = swept_by(&nodes, "compactor-1").await;
        assert!(first > 0 && second > 0, "{first} + {second}");
        assert_eq!(first + second, 40);
    }
}
