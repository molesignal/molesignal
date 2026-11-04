// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 实时规则在接收写入的节点上的后台任务：周期性重新加载规则，规则一改马上重新加载，让匹配表跟上
//! 规则的改动。开事件与解决由负责汇总的 alert 节点做（[`super::RealtimeCoordinator`]）。
//!
//! 接上快照仓库时（[`RealtimeAlertWorker::with_snapshots`]），第一次加载规则后读回本节点重启前存的
//! 计数，之后每 [`SNAPSHOT_EVERY`] 存一份；崩溃最多丢最后这么久的计数。存得太久的快照（已经整个
//! 出了最长的窗口）每 [`PURGE_EVERY`] 清一次。

use std::{sync::Arc, time::Duration};

use tokio::task::JoinHandle;

use super::{MAX_WINDOW_SECS, RealtimeAlerts};
use crate::{
    domain::alerting::{
        realtime_snapshot::RealtimeSnapshotRepository, repositories::AlertRuleRepository,
    },
    shared::time::TimestampMicros,
};

/// 重新加载规则的间隔：让其它节点上的规则变更在这个时间内生效。
const RELOAD_EVERY: Duration = Duration::from_secs(10);

/// 多久存一份计数快照。
const SNAPSHOT_EVERY: Duration = Duration::from_secs(5);

/// 多久清一次过期的快照。
const PURGE_EVERY: Duration = Duration::from_secs(10 * 60);

/// 快照所在的节点和存到哪里。
struct Snapshots {
    node_id: String,
    repository: Arc<dyn RealtimeSnapshotRepository>,
}

pub struct RealtimeAlertWorker {
    alerts: Arc<RealtimeAlerts>,
    rules: Arc<dyn AlertRuleRepository>,
    snapshots: Option<Snapshots>,
}

impl RealtimeAlertWorker {
    pub fn new(alerts: Arc<RealtimeAlerts>, rules: Arc<dyn AlertRuleRepository>) -> Self {
        Self {
            alerts,
            rules,
            snapshots: None,
        }
    }

    /// 计数存快照，重启时读回。
    pub fn with_snapshots(
        mut self,
        node_id: String,
        repository: Arc<dyn RealtimeSnapshotRepository>,
    ) -> Self {
        self.snapshots = Some(Snapshots {
            node_id,
            repository,
        });
        self
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(self.run())
    }

    #[tracing::instrument(
        name = "worker.realtime_alerts",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "realtime_alerts")
    )]
    async fn run(self) {
        // 没接快照时没有要读回的；读回之前不存，免得空窗口盖掉重启前的那份。
        let mut restored = self.snapshots.is_none();
        let saving = self.snapshots.is_some();

        let mut reload = tokio::time::interval(RELOAD_EVERY);
        let mut snapshot = tokio::time::interval(SNAPSHOT_EVERY);
        let mut purge = tokio::time::interval(PURGE_EVERY);

        loop {
            tokio::select! {
                _ = reload.tick() => self.reload(&mut restored).await,
                _ = self.alerts.wait_for_change() => self.reload(&mut restored).await,
                _ = snapshot.tick(), if restored && saving => self.save().await,
                _ = purge.tick(), if saving => self.purge().await,
            }
        }
    }

    /// 重新加载规则；第一次加载成功后读回重启前的计数。
    async fn reload(&self, restored: &mut bool) {
        match self.rules.list_enabled().await {
            Ok(rules) => {
                self.alerts.reload(rules);

                if !*restored {
                    *restored = self.restore().await;
                }
            }
            Err(error) => tracing::warn!(%error, "real-time alert rules reload failed"),
        }
    }

    /// 读回本节点重启前存的计数；读不出来时返回 `false`，下次加载规则后再试。
    async fn restore(&self) -> bool {
        let Some(snapshots) = &self.snapshots else {
            return true;
        };

        match snapshots.repository.load_node(&snapshots.node_id).await {
            Ok(saved) => {
                let restored = self.alerts.restore(&saved);

                if restored > 0 {
                    tracing::info!(
                        restored,
                        "real-time rule counts restored from the last snapshot"
                    );
                }

                true
            }
            Err(error) => {
                tracing::warn!(%error, "real-time rule counts could not be restored");
                false
            }
        }
    }

    async fn save(&self) {
        let Some(snapshots) = &self.snapshots else {
            return;
        };

        let taken = self
            .alerts
            .snapshots(&snapshots.node_id, TimestampMicros::now());

        if let Err(error) = snapshots
            .repository
            .replace_node(&snapshots.node_id, &taken)
            .await
        {
            tracing::debug!(%error, "real-time rule counts could not be saved");
        }
    }

    async fn purge(&self) {
        let Some(snapshots) = &self.snapshots else {
            return;
        };

        let horizon = i64::from(MAX_WINDOW_SECS) * 1_000_000;
        let before = TimestampMicros(TimestampMicros::now().0 - horizon);

        if let Err(error) = snapshots.repository.delete_before(before).await {
            tracing::debug!(%error, "old real-time rule snapshots could not be removed");
        }
    }
}
