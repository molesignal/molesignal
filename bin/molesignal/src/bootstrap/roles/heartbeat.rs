// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 节点在 `cluster_nodes` 表里的登记。
//!
//! - [`HeartbeatTask`]：每 `[cluster].heartbeat_interval_secs` 把 (node_id, roles, advertise_addr)
//!   upsert 到 `cluster_nodes`，其余节点据此发现本节点。
//! - 离开集群时删除自己的行：关停一开始由 [`Heartbeat::leave`] 立即注销；管理接口触发的退役
//!   在下一次心跳时注销，进程继续运行。其余节点在节点发现缓存过期（2 秒）后就不再把本节点
//!   当作成员，不必等 `[cluster].peer_timeout_secs` 超时。
//! - [`spawn_sweeper`]：清理静默超过 5 分钟的行。

use std::{sync::Arc, time::Duration};

use tokio::{sync::Mutex, task::JoinHandle};
use tokio_util::sync::CancellationToken;

use crate::{
    app::cluster::PeerRole,
    config::Role,
    infra::persistence::repositories::cluster::nodes::ClusterNodesRepository,
    shared::{drain::DrainController, time::TimestampMicros},
};

/// 注销最多等这么久：元数据库不可达时不拖住关停，其余节点在心跳超时后摘除本节点。
const LEAVE_TIMEOUT: Duration = Duration::from_secs(5);

pub struct HeartbeatTask {
    repo: Arc<dyn ClusterNodesRepository>,
    node_id: String,
    roles: Vec<PeerRole>,
    advertise_addr: String,
    interval: Duration,
    /// 节点 drain 状态；管理接口触发退役时注销自己的 cluster_nodes 行并停止心跳。
    drain: Option<Arc<DrainController>>,
}

/// 运行中的心跳。
pub struct Heartbeat {
    stop: CancellationToken,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl Heartbeat {
    /// 停止心跳并注销本节点，返回时注销已经执行过。可以重复调用。
    pub async fn leave(&self) {
        self.stop.cancel();

        let Some(mut task) = self.task.lock().await.take() else {
            return;
        };

        if tokio::time::timeout(LEAVE_TIMEOUT, &mut task)
            .await
            .is_err()
        {
            task.abort();
            tracing::warn!(
                "cluster registry deregistration timed out; peers drop this node after the peer timeout"
            );
        }
    }
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

    pub fn spawn(self) -> Arc<Heartbeat> {
        let stop = CancellationToken::new();
        let task = tokio::spawn(self.run(stop.clone()));

        Arc::new(Heartbeat {
            stop,
            task: Mutex::new(Some(task)),
        })
    }

    async fn run(self, stop: CancellationToken) {
        // 首次立即 upsert，后续按 interval。
        self.beat().await;
        let mut ticker = tokio::time::interval(self.interval);
        ticker.tick().await;

        loop {
            tokio::select! {
                biased;
                _ = stop.cancelled() => break,
                _ = ticker.tick() => {}
            }

            // 管理接口触发的退役：注销后停止心跳，进程继续运行。
            if self.drain.as_ref().is_some_and(|drain| drain.is_draining()) {
                break;
            }

            self.beat().await;
        }

        match self.repo.delete(&self.node_id).await {
            Ok(()) => tracing::info!(node = %self.node_id, "node left the cluster registry"),
            Err(e) => tracing::warn!(error = %e, "cluster registry deregistration failed"),
        }
    }

    async fn beat(&self) {
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
}

/// `[node].roles` 对应登记到 `cluster_nodes` 的角色集；没有配置角色时按 `standalone` 登记。
pub fn peer_roles(roles: &[Role]) -> Vec<PeerRole> {
    let mut peer_roles: Vec<PeerRole> = roles
        .iter()
        .map(|role| match role {
            Role::Router => PeerRole::Router,
            Role::Intake => PeerRole::Intake,
            Role::Querier => PeerRole::Querier,
            Role::Compactor => PeerRole::Compactor,
            Role::Alert => PeerRole::Alert,
            Role::Standalone => PeerRole::Standalone,
        })
        .collect();

    if peer_roles.is_empty() {
        peer_roles.push(PeerRole::Standalone);
    }

    peer_roles
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

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use super::*;
    use crate::{infra::persistence::repositories::cluster::nodes::ClusterNodeRow, shared::Result};

    /// 按顺序记录心跳与注销的成员表。
    #[derive(Default)]
    struct Registrations(parking_lot::Mutex<Vec<&'static str>>);

    impl Registrations {
        fn calls(&self) -> Vec<&'static str> {
            self.0.lock().clone()
        }

        async fn wait_for(&self, calls: usize) {
            for _ in 0..300 {
                if self.calls().len() >= calls {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("expected {calls} registry calls, saw {:?}", self.calls());
        }
    }

    #[async_trait]
    impl ClusterNodesRepository for Registrations {
        async fn upsert(
            &self,
            _node_id: &str,
            _roles: &[PeerRole],
            _advertise_addr: &str,
            _ts: TimestampMicros,
        ) -> Result<()> {
            self.0.lock().push("upsert");
            Ok(())
        }

        async fn list_alive(&self, _since: TimestampMicros) -> Result<Vec<ClusterNodeRow>> {
            Ok(Vec::new())
        }

        async fn sweep_stale(&self, _older_than: TimestampMicros) -> Result<u64> {
            Ok(0)
        }

        async fn delete(&self, _node_id: &str) -> Result<()> {
            self.0.lock().push("delete");
            Ok(())
        }
    }

    fn start(
        repo: &Arc<Registrations>,
        drain: &Arc<DrainController>,
        interval_secs: u32,
    ) -> Arc<Heartbeat> {
        HeartbeatTask::new(
            repo.clone(),
            "compactor-0".into(),
            vec![PeerRole::Compactor],
            "127.0.0.1:5082".into(),
            interval_secs,
        )
        .with_drain(drain.clone())
        .spawn()
    }

    #[tokio::test]
    async fn leaving_deregisters_at_once_instead_of_on_the_next_heartbeat() {
        let repo = Arc::new(Registrations::default());
        let drain = Arc::new(DrainController::new());
        // 心跳间隔远长于测试：注销不能等下一次心跳。
        let heartbeat = start(&repo, &drain, 3600);
        repo.wait_for(1).await;

        tokio::time::timeout(Duration::from_secs(1), heartbeat.leave())
            .await
            .expect("leave returns without waiting for a heartbeat");
        assert_eq!(repo.calls(), ["upsert", "delete"]);

        heartbeat.leave().await;
        assert_eq!(repo.calls(), ["upsert", "delete"]);
    }

    #[tokio::test]
    async fn an_admin_drain_deregisters_on_the_next_heartbeat_and_stops_beating() {
        let repo = Arc::new(Registrations::default());
        let drain = Arc::new(DrainController::new());
        let heartbeat = start(&repo, &drain, 1);
        repo.wait_for(1).await;

        drain.begin_drain();
        repo.wait_for(2).await;
        assert_eq!(repo.calls(), ["upsert", "delete"]);

        // 心跳已经停止，关停时的 leave 直接返回。
        tokio::time::timeout(Duration::from_secs(1), heartbeat.leave())
            .await
            .expect("an already stopped heartbeat leaves at once");
        assert_eq!(repo.calls(), ["upsert", "delete"]);
    }

    #[test]
    fn nodes_without_configured_roles_register_as_standalone() {
        assert_eq!(peer_roles(&[]), [PeerRole::Standalone]);
        assert_eq!(
            peer_roles(&[Role::Intake, Role::Querier]),
            [PeerRole::Intake, PeerRole::Querier]
        );
    }
}
