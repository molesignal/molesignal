// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{sync::Arc, time::Duration};

use tokio::task::JoinHandle;

use crate::{app::status_page::StatusPageService, infra::status_page::StatusPageAssetCleaner};

pub struct StatusPageMaintenanceWorker {
    service: Arc<StatusPageService>,
    assets: Arc<StatusPageAssetCleaner>,
    interval: Duration,
}

impl StatusPageMaintenanceWorker {
    pub fn new(service: Arc<StatusPageService>, assets: Arc<StatusPageAssetCleaner>) -> Self {
        Self {
            service,
            assets,
            interval: Duration::from_secs(5 * 60),
        }
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(self.interval);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tick.tick().await;
                self.sweep_once().await;
            }
        })
    }

    #[tracing::instrument(
        name = "worker.status_page_maintenance",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "status_page_maintenance")
    )]
    async fn sweep_once(&self) {
        if let Err(error) = self.service.verify_domains_due(100).await {
            tracing::warn!(error = %error, "status-page domain sweep failed");
        }
        if let Err(error) = self
            .service
            .purge_terminal_notification_deliveries(1000)
            .await
        {
            tracing::warn!(error = %error, "status-page delivery retention sweep failed");
        }
        match self.service.purge_archived_pages(100).await {
            Ok(pages) => {
                for page in pages {
                    if let Err(error) = self.assets.delete_page_assets(&page).await {
                        tracing::warn!(
                            status_page_id = %page.id,
                            error = %error,
                            "status-page archived asset cleanup failed"
                        );
                    }
                }
            }
            Err(error) => {
                tracing::warn!(error = %error, "status-page archive retention sweep failed");
            }
        }
        if let Err(error) = self.service.purge_expired_access_artifacts(1000).await {
            tracing::warn!(error = %error, "status-page private-access sweep failed");
        }
    }
}
