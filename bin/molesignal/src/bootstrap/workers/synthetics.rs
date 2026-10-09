// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{sync::Arc, time::Duration};

use tokio::task::JoinHandle;

use super::polling::PollingBackoff;
use crate::{
    app::{
        cluster::{PeerRole, WorkOwnership},
        status_page::StatusPageService,
        synthetics::SyntheticService,
    },
    shared::{Result, time::TimestampMicros},
};

mod embedded;

pub use embedded::EmbeddedProbeRunner;

/// 合成监控自动化：只在 alert 主节点上运行（见 [`WorkOwnership::is_primary`]）。
pub struct SyntheticAutomationWorker {
    synthetics: Arc<SyntheticService>,
    status_pages: Arc<StatusPageService>,
    interval: Duration,
    /// 默认不分担（本节点就是主节点）。
    ownership: Arc<WorkOwnership>,
}

impl SyntheticAutomationWorker {
    pub fn new(synthetics: Arc<SyntheticService>, status_pages: Arc<StatusPageService>) -> Self {
        Self {
            synthetics,
            status_pages,
            interval: Duration::from_secs(2),
            ownership: WorkOwnership::solo(PeerRole::Alert),
        }
    }

    /// 与其余 alert 节点分担：只有主节点做自动化。
    pub fn with_ownership(mut self, ownership: Arc<WorkOwnership>) -> Self {
        self.ownership = ownership;
        self
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut backoff = PollingBackoff::new(
                self.interval,
                Duration::from_secs(10),
                "synthetics-automation",
            );
            loop {
                tokio::time::sleep(backoff.next_delay()).await;
                if !self.ownership.is_primary().await {
                    // 非主节点保持基础轮询间隔，接任主节点后立刻按正常节奏工作。
                    backoff.reset();
                    continue;
                }
                match self.sweep_once().await {
                    Ok(work) if work > 0 => backoff.reset(),
                    Ok(_) => {}
                    Err(error) => {
                        tracing::warn!(%error, "synthetic automation sweep failed");
                    }
                }
            }
        })
    }

    #[tracing::instrument(
        name = "worker.synthetics",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "synthetics")
    )]
    async fn sweep_once(&self) -> Result<u64> {
        let now = TimestampMicros::now();
        let expired = self.synthetics.expire_task_leases(now, 500).await?;
        let tasks = self.synthetics.materialize_due_tasks(now, 200).await?;
        let candidates = self
            .status_pages
            .process_due_automation_candidates(200)
            .await?;
        let outbox = self.status_pages.process_automation_outbox(200).await?;
        tracing::debug!(
            expired_leases = expired,
            materialized_tasks = tasks.len(),
            due_candidates = candidates,
            processed_outbox = outbox,
            "synthetic automation sweep completed"
        );
        Ok(expired
            .saturating_add(u64::try_from(tasks.len()).unwrap_or(u64::MAX))
            .saturating_add(u64::from(candidates))
            .saturating_add(u64::from(outbox)))
    }
}
