// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{sync::Arc, time::Duration};

use tokio::task::JoinHandle;

use super::polling::PollingBackoff;
use crate::{
    app::{
        cluster::{PeerRole, WorkOwnership},
        status_page::StatusPageService,
    },
    infra::status_page::StatusPageAssetCleaner,
};

/// 非主节点多久后再检查一次自己是否已成为主节点。
const NOT_PRIMARY_RECHECK: Duration = Duration::from_secs(5);

/// 状态页维护（域名校验、过期数据清理）：只在 alert 主节点上运行（见
/// [`WorkOwnership::is_primary`]）。
pub struct StatusPageMaintenanceWorker {
    service: Arc<StatusPageService>,
    assets: Arc<StatusPageAssetCleaner>,
    interval: Duration,
    /// 默认不分担（本节点就是主节点）。
    ownership: Arc<WorkOwnership>,
}

impl StatusPageMaintenanceWorker {
    pub fn new(service: Arc<StatusPageService>, assets: Arc<StatusPageAssetCleaner>) -> Self {
        Self {
            service,
            assets,
            interval: Duration::from_secs(5 * 60),
            ownership: WorkOwnership::solo(PeerRole::Alert),
        }
    }

    /// 与其余 alert 节点分担：只有主节点做维护。
    pub fn with_ownership(mut self, ownership: Arc<WorkOwnership>) -> Self {
        self.ownership = ownership;
        self
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut cadence =
                PollingBackoff::new(self.interval, self.interval, "status-page-maintenance");

            loop {
                // 非主节点很快再看一眼：刚启动或刚接任主节点时不必等满一个维护周期。
                let delay = if self.ownership.is_primary().await {
                    self.sweep_once().await;
                    cadence.next_delay()
                } else {
                    NOT_PRIMARY_RECHECK
                };

                tokio::time::sleep(delay).await;
                cadence.reset();
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

        if let Err(error) = self.service.purge_abandoned_slack_app_subscriptions().await {
            tracing::warn!(error = %error, "status-page Slack request sweep failed");
        }
    }
}
