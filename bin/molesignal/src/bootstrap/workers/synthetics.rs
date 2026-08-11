// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{sync::Arc, time::Duration};

use tokio::task::JoinHandle;

use crate::{
    app::{status_page::StatusPageService, synthetics::SyntheticService},
    shared::{Result, time::TimestampMicros},
};

mod embedded;

pub use embedded::EmbeddedProbeRunner;

pub struct SyntheticAutomationWorker {
    synthetics: Arc<SyntheticService>,
    status_pages: Arc<StatusPageService>,
    interval: Duration,
}

impl SyntheticAutomationWorker {
    pub fn new(synthetics: Arc<SyntheticService>, status_pages: Arc<StatusPageService>) -> Self {
        Self {
            synthetics,
            status_pages,
            interval: Duration::from_secs(2),
        }
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(self.interval);
            tick.tick().await;
            loop {
                tick.tick().await;
                if let Err(error) = self.sweep_once().await {
                    tracing::warn!(error = %error, "synthetic automation sweep failed");
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
    async fn sweep_once(&self) -> Result<()> {
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
        Ok(())
    }
}
