// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{sync::Arc, time::Duration};

use tokio::task::JoinHandle;

use super::polling::PollingBackoff;
use crate::{app::status_page::StatusPageService, shared::Result};

pub struct StatusPageNotificationWorker {
    service: Arc<StatusPageService>,
    interval: Duration,
    batch_size: u32,
}

impl StatusPageNotificationWorker {
    pub fn new(service: Arc<StatusPageService>) -> Self {
        Self {
            service,
            interval: Duration::from_secs(5),
            batch_size: 100,
        }
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut backoff = PollingBackoff::new(
                self.interval,
                Duration::from_secs(30),
                "status-page-notifications",
            );
            loop {
                match self.sweep_once().await {
                    Ok(processed) if processed > 0 => backoff.reset(),
                    Ok(_) => {}
                    Err(error) => {
                        tracing::warn!(%error, "status-page notification sweep failed");
                    }
                }
                tokio::time::sleep(backoff.next_delay()).await;
            }
        })
    }

    #[tracing::instrument(
        name = "worker.status_page_notifications",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "status_page_notifications")
    )]
    async fn sweep_once(&self) -> Result<u32> {
        self.service
            .process_pending_notifications(self.batch_size)
            .await
    }
}
