// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! HeartbeatTask：每 5s 把 (node_id, role, advertise_addr) upsert 到
//! `cluster_nodes` 表。
//!
//! standalone 模式直接走 repo（无 gRPC 跳转）；分布式模式下也可直接走 repo（PG 共享）。

use std::{sync::Arc, time::Duration};

use tokio::task::JoinHandle;

use crate::{
    app::cluster::PeerRole,
    infra::persistence::repositories::cluster::nodes::ClusterNodesRepository,
    shared::{drain::DrainController, time::TimestampMicros},
};

pub struct HeartbeatTask {
    repo: Arc<dyn ClusterNodesRepository>,
    node_id: String,
    roles: Vec<PeerRole>,
    advertise_addr: String,
    interval: Duration,
    /// 节点 drain 状态；退役时注销自己的 cluster_nodes 行 + 停止心跳，让集群路由在短缓存
    /// 过期后摘除本节点。
    drain: Option<Arc<DrainController>>,
}

impl HeartbeatTask {
    pub fn new(
        repo: Arc<dyn ClusterNodesRepository>,
        node_id: String,
        roles: Vec<PeerRole>,
        advertise_addr: String,
        interval_secs: u32,
    ) -> Self {
        Self {
            repo,
            node_id,
            roles,
            advertise_addr,
            interval: Duration::from_secs(interval_secs.max(1) as u64),
            drain: None,
        }
    }

    /// 注入节点 drain 状态；退役时注销 + 停心跳。
    pub fn with_drain(mut self, drain: Arc<DrainController>) -> Self {
        self.drain = Some(drain);
        self
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            // 首次立即 upsert，后续按 interval。
            if let Err(e) = self
                .repo
                .upsert(
                    &self.node_id,
                    &self.roles,
                    &self.advertise_addr,
                    TimestampMicros::now(),
                )
                .await
            {
                tracing::warn!(error = %e, "first heartbeat upsert failed");
            }
            let mut ticker = tokio::time::interval(self.interval);
            ticker.tick().await;
            loop {
                ticker.tick().await;
                // 退役中：注销自己的 cluster_nodes 行 + 停止心跳，使 coordinator 的
                // `list_alive` 在短缓存过期后不再把本节点选为 querier/ingester。
                if let Some(drain) = &self.drain
                    && drain.is_draining()
                {
                    if let Err(e) = self.repo.delete(&self.node_id).await {
                        tracing::warn!(error = %e, "heartbeat deregister on drain failed");
                    } else {
                        tracing::info!(node = %self.node_id, "node draining; deregistered from cluster registry");
                    }
                    break;
                }
                if let Err(e) = self
                    .repo
                    .upsert(
                        &self.node_id,
                        &self.roles,
                        &self.advertise_addr,
                        TimestampMicros::now(),
                    )
                    .await
                {
                    tracing::warn!(error = %e, "heartbeat upsert failed");
                }
            }
        })
    }
}

/// sweeper：每 60s 调 `sweep_stale(now - 5min)`，回收脏 cluster_nodes 行。
pub fn spawn_sweeper(repo: Arc<dyn ClusterNodesRepository>) -> JoinHandle<()> {
    let stale_window_us: i64 = 5 * 60 * 1_000_000;
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(60));
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let older_than = TimestampMicros(TimestampMicros::now().0 - stale_window_us);
            match repo.sweep_stale(older_than).await {
                Ok(n) if n > 0 => tracing::info!(swept = n, "cluster sweeper removed stale nodes"),
                Ok(_) => {}
                Err(e) => tracing::warn!(error = %e, "cluster sweeper failed"),
            }
        }
    })
}
