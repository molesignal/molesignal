// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 跨节点 search admission 同步 worker：周期把本节点 per-work-group 并发 in_flight 发布到
//! `search_admission_load`，并读回「集群（除自身）各组合计」喂给本地 [`AdmissionController`]。
//!
//! 这样集群级软上限（`AdmissionConfig.cluster_*`）得以跨 querier 协调：本地 in_flight 实时、
//! 其它节点合计按本 worker 间隔最终一致。仅在承担查询的节点（querier / standalone）起。

use std::{sync::Arc, time::Duration};

use tokio::task::JoinHandle;

use crate::{
    app::search::AdmissionController,
    infra::persistence::repositories::search::admission_load::SearchAdmissionLoadRepository,
    shared::{Result, time::TimestampMicros},
};

pub struct AdmissionLoadSync {
    node_id: String,
    controller: Arc<AdmissionController>,
    repo: Arc<dyn SearchAdmissionLoadRepository>,
    interval_secs: u64,
    stale_secs: u64,
}

impl AdmissionLoadSync {
    pub fn new(
        node_id: String,
        controller: Arc<AdmissionController>,
        repo: Arc<dyn SearchAdmissionLoadRepository>,
        interval_secs: u64,
        stale_secs: u64,
    ) -> Self {
        Self {
            node_id,
            controller,
            repo,
            interval_secs: if interval_secs == 0 { 5 } else { interval_secs },
            stale_secs: if stale_secs == 0 { 30 } else { stale_secs },
        }
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(self.interval_secs));
            loop {
                tick.tick().await;
                if let Err(e) = self.sync_once().await {
                    tracing::warn!(error = %e, "admission load sync failed");
                }
            }
        })
    }

    #[tracing::instrument(
        name = "worker.admission_load_sync",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "admission_load_sync")
    )]
    async fn sync_once(&self) -> Result<()> {
        let now = TimestampMicros::now();
        // 1) 发布本节点各组当前 in_flight。
        let load = self.controller.local_load();
        if !load.is_empty() {
            self.repo.publish(&self.node_id, &load, now).await?;
        }
        // 2) 读回集群（除自身）各组合计，刷新本地视图。
        let since = TimestampMicros(now.0 - (self.stale_secs as i64) * 1_000_000);
        let totals = self
            .repo
            .cluster_totals_excluding(&self.node_id, since)
            .await?;
        let view = totals
            .into_iter()
            .map(|(g, n)| (g, n.max(0) as usize))
            .collect();
        self.controller.set_cluster_view(view);
        Ok(())
    }
}
