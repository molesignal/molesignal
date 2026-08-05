// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! compactor 角色：周期合并小 parquet + retention 扫描。
//!
//! - 每 `interval_secs` 一次 tick：遍历所有 org / stream，逐个 `sweep_one + retention_sweep`。
//! - 并发上限 `max_concurrent_groups` 用 `tokio::sync::Semaphore` 控制（同一 tick 内
//!   并行处理多个 stream，单 stream 内部仍串行）。

use std::{sync::Arc, time::Duration};

use object_store::{ObjectStore, ObjectStoreExt, path::Path as ObjPath};
use tokio::{sync::Semaphore, task::JoinHandle};

use crate::{
    config::CompactorSettings,
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

/// 启动后台 compactor tick 任务。
///
/// 返回 `JoinHandle` 由调用方持有（standalone 模式下 bootstrap 阶段 spawn 即可）。
pub fn spawn_compactor_loop(
    compactor: Arc<Compactor>,
    orgs: Arc<dyn OrganizationRepository>,
    streams: Arc<dyn StreamRepository>,
    investigation_blobs: Arc<dyn InvestigationBlobRepository>,
    object_store: Arc<dyn ObjectStore>,
    settings: CompactorSettings,
    drain: Arc<DrainController>,
) -> JoinHandle<()> {
    let max = settings.max_concurrent_groups.max(1) as usize;
    let sem = Arc::new(Semaphore::new(max));
    let interval = Duration::from_secs(settings.interval_secs.max(1) as u64);
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        // 第一次 tick 立刻触发（spec 没强制 skip），但为避免与 ingester replay 抢资源稍微 delay 一下
        ticker.tick().await;
        let mut blob_sweep_at = TimestampMicros::now().0;
        loop {
            ticker.tick().await;
            // 节点退役：停接新活——结束 compaction loop（合并/降采样可中断，下次启动续做）。
            if drain.is_draining() {
                tracing::info!("compactor draining; stopping compaction loop");
                break;
            }
            if let Err(e) = run_once(&compactor, orgs.as_ref(), streams.as_ref(), &sem).await {
                tracing::warn!(error = %e, "compactor tick failed");
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

async fn run_once(
    compactor: &Arc<Compactor>,
    orgs: &dyn OrganizationRepository,
    streams: &dyn StreamRepository,
    sem: &Arc<Semaphore>,
) -> anyhow::Result<()> {
    let org_list = orgs.list().await?;
    let now = TimestampMicros::now();
    let mut handles = Vec::new();
    for org in org_list {
        let stream_list = streams.list(&org.id).await?;
        for stream in stream_list {
            let permit = sem.clone().acquire_owned().await?;
            let compactor = compactor.clone();
            let stream = stream.clone();
            let h = tokio::spawn(async move {
                let _permit = permit;
                let retention_days =
                    stream.effective_retention_days(compactor.settings().retention_days);
                let lookback =
                    TimestampMicros(now.0 - (retention_days as i64) * 86_400 * 1_000_000);
                if let Err(e) = compactor
                    .sweep_one(&stream, TimeRange::new(lookback, now))
                    .await
                {
                    tracing::warn!(stream = %stream.name, error = %e, "sweep_one failed");
                }
                if let Err(e) = compactor.retention_sweep(&stream).await {
                    tracing::warn!(stream = %stream.name, error = %e, "retention_sweep failed");
                }
                // 降采样：把早于阈值的 metrics 预聚合成时间桶（关闭时 no-op）。
                if let Err(e) = compactor.downsample_sweep(&stream).await {
                    tracing::warn!(stream = %stream.name, error = %e, "downsample_sweep failed");
                }
            });
            handles.push(h);
        }
    }
    for h in handles {
        let _ = h.await;
    }
    Ok(())
}
