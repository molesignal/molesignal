// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Service graph flush worker：把 ingest 路径累计的分钟桶边周期落 `service_graph_edges`。
//!
//! [`ServiceGraphAggregator`] 是单进程内存态：`IngestService` 在 Traces 批上旁路喂 span，
//! 配对出 caller→callee 边累计进分钟桶。本 worker 每个 tick：
//! 1. `flush_due(cutoff)` drain 出**已完成分钟**的桶 → `insert_many` 落库；
//! 2. `prune` 清掉超 TTL 仍未配对的 span（防内存累积）。
//!
//! 必须与聚合器**同进程**运行（内存态不跨节点），故按"本进程是否接 ingest"起，不做
//! 单点 role gate。分布式下各 ingest 进程各自 flush；查询侧按 (client,server) 求和汇总。

use std::{sync::Arc, time::Duration};

use tokio::task::JoinHandle;

use crate::{
    domain::iam::InstanceSettingsRepository,
    infra::traces::{ServiceGraphAggregator, ServiceGraphRepository},
    shared::{Result, time::TimestampMicros},
};

/// 分钟桶宽（μs），与 [`ServiceGraphAggregator`] 内部一致。
const BUCKET_SIZE_US: i64 = 60 * 1_000_000;

#[derive(Debug, Clone)]
pub struct ServiceGraphFlushConfig {
    /// flush 间隔秒。
    pub interval_secs: u64,
    /// 未配对 span 的存活上限秒；超过即 prune（兜底乱序/丢失的对端 span）。
    pub span_ttl_secs: u64,
}

impl Default for ServiceGraphFlushConfig {
    fn default() -> Self {
        Self {
            interval_secs: 30,
            span_ttl_secs: 300,
        }
    }
}

pub struct ServiceGraphFlusher {
    agg: Arc<ServiceGraphAggregator>,
    repo: Arc<dyn ServiceGraphRepository>,
    instance_settings: Arc<dyn InstanceSettingsRepository>,
    cfg: ServiceGraphFlushConfig,
}

impl ServiceGraphFlusher {
    pub fn new(
        agg: Arc<ServiceGraphAggregator>,
        repo: Arc<dyn ServiceGraphRepository>,
        instance_settings: Arc<dyn InstanceSettingsRepository>,
        cfg: ServiceGraphFlushConfig,
    ) -> Self {
        Self {
            agg,
            repo,
            instance_settings,
            cfg,
        }
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(self.cfg.interval_secs));
            loop {
                tick.tick().await;
                if let Err(e) = self.flush_once().await {
                    tracing::warn!(error = %e, "service graph flush failed");
                }
            }
        })
    }

    #[tracing::instrument(
        name = "worker.service_graph_flush",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "service_graph_flush")
    )]
    async fn flush_once(&self) -> Result<()> {
        let now = TimestampMicros::now().0;
        // 只 drain 已完成的分钟桶；当前进行中的分钟保留继续累计。
        let cutoff = (now / BUCKET_SIZE_US) * BUCKET_SIZE_US;
        let snaps = self.agg.flush_due(cutoff);
        // storage 模式下由单例重算 worker 落库 → 这里只 drain+丢弃（避免双写），仍 prune 控内存。
        let storage = self
            .instance_settings
            .get()
            .await
            .map(|s| s.service_graph_storage_mode())
            .unwrap_or(false);
        if !storage && !snaps.is_empty() {
            let n = snaps.len();
            self.repo.insert_many(&snaps).await?;
            tracing::debug!(edges = n, "flushed service graph edges");
        }
        // 清理超 TTL 的未配对 span。
        let span_cutoff = now - (self.cfg.span_ttl_secs as i64) * 1_000_000;
        self.agg.prune(span_cutoff);
        Ok(())
    }
}
