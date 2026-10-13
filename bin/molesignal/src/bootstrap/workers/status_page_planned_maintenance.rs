// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{sync::Arc, time::Duration};

use tokio::task::JoinHandle;

use super::polling::PollingBackoff;
use crate::app::{
    cluster::{PeerRole, WorkOwnership},
    status_page::StatusPageService,
};

/// 非主节点多久后再检查一次自己是否已成为主节点。
const NOT_PRIMARY_RECHECK: Duration = Duration::from_secs(5);

/// 一轮最多自动开始多少个到点的维护；剩下的留给下一轮。
const START_BATCH: u32 = 100;

/// 计划维护的到点处理：到达计划开始时间时自动转为进行中，并在开始前提醒订阅者。只在 alert
/// 主节点上运行（见 [`WorkOwnership::is_primary`]）。短暂出现两个主节点也不会重复：提醒按计划
/// 开始时间认领一次，开始只对仍处于“已计划”的维护生效。
pub struct StatusPagePlannedMaintenanceWorker {
    service: Arc<StatusPageService>,
    interval: Duration,
    /// 默认不分担（本节点就是主节点）。
    ownership: Arc<WorkOwnership>,
}

impl StatusPagePlannedMaintenanceWorker {
    pub fn new(service: Arc<StatusPageService>) -> Self {
        Self {
            service,
            interval: Duration::from_secs(30),
            ownership: WorkOwnership::solo(PeerRole::Alert),
        }
    }

    /// 与其余 alert 节点分担：只有主节点处理计划维护。
    pub fn with_ownership(mut self, ownership: Arc<WorkOwnership>) -> Self {
        self.ownership = ownership;
        self
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut cadence = PollingBackoff::new(
                self.interval,
                self.interval,
                "status-page-planned-maintenance",
            );

            loop {
                // 非主节点很快再看一眼：刚接任主节点时不必等满一个周期。
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
        name = "worker.status_page_planned_maintenance",
        parent = None,
        skip_all,
        fields(
            otel.kind = "internal",
            molesignal.worker.name = "status_page_planned_maintenance"
        )
    )]
    async fn sweep_once(&self) {
        match self.service.run_planned_maintenance(START_BATCH).await {
            Ok(sweep) if sweep.started > 0 || sweep.reminders_queued > 0 => {
                tracing::info!(
                    started = sweep.started,
                    reminders_queued = sweep.reminders_queued,
                    "planned maintenance sweep acted"
                );
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(error = %error, "planned maintenance sweep failed");
            }
        }
    }
}
