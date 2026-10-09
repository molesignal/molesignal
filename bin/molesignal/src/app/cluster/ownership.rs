// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 后台工作的节点归属。
//!
//! 同一角色的多个节点分担一批周期性后台工作。每个工作单元有一个稳定的 key（数据流 ID、
//! 组织 ID……），owner 由「存活成员视图 + rendezvous 哈希」决定：视图相同的节点得出同一个
//! owner，成员增减只迁移该成员名下的 key，不需要协调表或租约。
//!
//! 各节点看到成员变化的时刻并不一致，所以归属只负责把重复工作压到最低，不是互斥锁。成员
//! 变化的短暂窗口里两个节点可能同时处理同一个 key，调用方的写入必须自带冲突保护（原子替换、
//! CAS、唯一索引、认领）。
//!
//! - 只有出现在视图里的节点才拥有 key：心跳过期的节点已被其余节点接管，不再处理任何工作。
//! - 节点进入视图后先预热一个存活窗口再接管 key，让其余节点先看到它并让出这些 key。

use std::{
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use prometheus::IntGaugeVec;
use tokio::{task::JoinHandle, time::MissedTickBehavior};

use super::{ClusterRegistry, PeerRole};
use crate::shared::{metrics::register_int_gauge_vec, rendezvous};

/// 一个角色名下后台工作的归属判定。
pub struct WorkOwnership {
    registry: Arc<dyn ClusterRegistry>,
    local_node_id: String,
    role: PeerRole,
    warmup: Duration,
    /// 本节点连续出现在成员视图中的起点；被其余节点判定离开后清空，回来要重新预热。
    present_since: Mutex<Option<Instant>>,
}

impl WorkOwnership {
    /// 承担 `role` 的全部存活节点（含 `standalone`）分担这批工作；`warmup` 取节点存活窗口。
    pub fn new(
        registry: Arc<dyn ClusterRegistry>,
        local_node_id: impl Into<String>,
        role: PeerRole,
        warmup: Duration,
    ) -> Arc<Self> {
        Arc::new(Self {
            registry,
            local_node_id: local_node_id.into(),
            role,
            warmup,
            present_since: Mutex::new(None),
        })
    }

    /// 本节点此刻是否拥有 `key`。每个工作单元开始前调用一次，耗时很长的一轮扫描才能在成员
    /// 变化后及时让出或接管。
    pub async fn owns(&self, key: &str) -> bool {
        self.view().await.owns(key)
    }

    /// 读取当前成员视图。
    pub async fn view(&self) -> OwnershipView {
        self.view_at(Instant::now()).await
    }

    async fn view_at(&self, now: Instant) -> OwnershipView {
        let peers = self.registry.list_all().await;
        let discovered = !peers.is_empty();
        let mut members = peers
            .into_iter()
            .filter(|peer| {
                peer.roles
                    .iter()
                    .any(|role| *role == self.role || *role == PeerRole::Standalone)
            })
            .map(|peer| peer.node_id)
            .collect::<Vec<_>>();
        members.sort_unstable();
        members.dedup();

        let active = {
            let mut present_since = self.present_since.lock();
            if members.contains(&self.local_node_id) {
                now.saturating_duration_since(*present_since.get_or_insert(now)) >= self.warmup
            } else {
                // 一个节点都没发现说明节点发现本身失败，判断不了成员关系：这一次不拥有任何
                // key，但保留预热进度，恢复后直接继续。
                if discovered {
                    *present_since = None;
                }
                false
            }
        };
        OwnershipView {
            members,
            local_node_id: self.local_node_id.clone(),
            active,
        }
    }

    /// 记录本节点在最近一轮 `work` 中拥有的工作单元数。
    pub fn record_owned(&self, work: &'static str, units: usize) {
        owned_units()
            .with_label_values(&[role_label(self.role), work])
            .set(i64::try_from(units).unwrap_or(i64::MAX));
    }

    /// 周期观察成员视图：让预热从心跳可见那一刻开始计时，并在成员变化时留下日志与指标。
    pub fn spawn_watch(self: &Arc<Self>, period: Duration) -> JoinHandle<()> {
        let ownership = self.clone();
        tokio::spawn(async move {
            let role = role_label(ownership.role);
            let mut ticker = tokio::time::interval(period.max(Duration::from_secs(1)));
            ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
            let mut last = None;
            loop {
                ticker.tick().await;
                let view = ownership.view().await;
                members_gauge()
                    .with_label_values(&[role])
                    .set(i64::try_from(view.members.len()).unwrap_or(i64::MAX));
                active_gauge()
                    .with_label_values(&[role])
                    .set(i64::from(view.active));
                if last.as_ref() != Some(&view) {
                    tracing::info!(
                        role,
                        node = %view.local_node_id,
                        members = ?view.members,
                        active = view.active,
                        "work ownership view changed"
                    );
                    last = Some(view);
                }
            }
        })
    }
}

/// 某一时刻的成员视图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnershipView {
    members: Vec<String>,
    local_node_id: String,
    active: bool,
}

impl OwnershipView {
    /// 本节点在该视图下是否拥有 `key`。
    pub fn owns(&self, key: &str) -> bool {
        self.active && self.owner(key) == Some(self.local_node_id.as_str())
    }

    /// `key` 在该视图下的 owner；没有成员时为 `None`。
    pub fn owner(&self, key: &str) -> Option<&str> {
        rendezvous::owner(key, self.members.iter().map(String::as_str))
    }

    /// 分担这批工作的存活节点，按节点 ID 排序。
    pub fn members(&self) -> &[String] {
        &self.members
    }

    /// 本节点在视图中且已完成预热。
    pub fn is_active(&self) -> bool {
        self.active
    }
}

fn role_label(role: PeerRole) -> &'static str {
    match role {
        PeerRole::Standalone => "standalone",
        PeerRole::Router => "router",
        PeerRole::Intake => "intake",
        PeerRole::Querier => "querier",
        PeerRole::Compactor => "compactor",
        PeerRole::Alert => "alert",
    }
}

fn members_gauge() -> &'static IntGaugeVec {
    static METRIC: OnceLock<IntGaugeVec> = OnceLock::new();
    METRIC.get_or_init(|| {
        register_int_gauge_vec(
            "work_ownership_members",
            "Alive nodes sharing the background work of a role",
            &["role"],
        )
    })
}

fn active_gauge() -> &'static IntGaugeVec {
    static METRIC: OnceLock<IntGaugeVec> = OnceLock::new();
    METRIC.get_or_init(|| {
        register_int_gauge_vec(
            "work_ownership_active",
            "Whether this node owns work of a role (1) or is absent or warming up (0)",
            &["role"],
        )
    })
}

fn owned_units() -> &'static IntGaugeVec {
    static METRIC: OnceLock<IntGaugeVec> = OnceLock::new();
    METRIC.get_or_init(|| {
        register_int_gauge_vec(
            "work_ownership_owned_units",
            "Work units this node owned in its latest pass",
            &["role", "work"],
        )
    })
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::Arc;

    use async_trait::async_trait;
    use parking_lot::Mutex;

    use crate::app::cluster::{ClusterRegistry, PeerInfo, PeerRole};

    /// 单测用的可变成员表，代替 `cluster_nodes` 心跳。
    #[derive(Default)]
    pub(crate) struct Nodes(Mutex<Vec<PeerInfo>>);

    impl Nodes {
        pub(crate) fn of(peers: &[(&str, PeerRole)]) -> Arc<Self> {
            let nodes = Arc::new(Self::default());
            for (node_id, role) in peers {
                nodes.join(node_id, *role);
            }
            nodes
        }

        pub(crate) fn join(&self, node_id: &str, role: PeerRole) {
            self.0.lock().push(PeerInfo {
                node_id: node_id.into(),
                advertise_addr: format!("{node_id}:5082"),
                roles: vec![role],
            });
        }

        pub(crate) fn leave(&self, node_id: &str) {
            self.0.lock().retain(|peer| peer.node_id != node_id);
        }

        /// 模拟节点发现失败：返回被清空的成员，供 [`Self::restore`] 恢复。
        pub(crate) fn fail_discovery(&self) -> Vec<PeerInfo> {
            std::mem::take(&mut *self.0.lock())
        }

        pub(crate) fn restore(&self, peers: Vec<PeerInfo>) {
            *self.0.lock() = peers;
        }
    }

    #[async_trait]
    impl ClusterRegistry for Nodes {
        async fn list_role(&self, _role: PeerRole) -> Vec<PeerInfo> {
            self.0.lock().clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{test_support::Nodes, *};

    const WARMUP: Duration = Duration::from_secs(15);

    fn compactor(nodes: &Arc<Nodes>, node_id: &str) -> Arc<WorkOwnership> {
        WorkOwnership::new(nodes.clone(), node_id, PeerRole::Compactor, WARMUP)
    }

    /// 已在视图中待满预热期的节点；返回它完成预热的时刻。
    async fn warmed(nodes: &Arc<Nodes>, node_id: &str) -> (Arc<WorkOwnership>, Instant) {
        let ownership = compactor(nodes, node_id);
        let joined = Instant::now();
        ownership.view_at(joined).await;
        (ownership, joined + WARMUP)
    }

    fn keys() -> Vec<String> {
        (0..200).map(|index| format!("stream-{index}")).collect()
    }

    fn owned(view: &OwnershipView) -> Vec<String> {
        keys().into_iter().filter(|key| view.owns(key)).collect()
    }

    #[tokio::test]
    async fn a_member_owns_nothing_until_it_has_been_visible_for_the_warmup() {
        let nodes = Nodes::of(&[("a", PeerRole::Compactor)]);
        let ownership = compactor(&nodes, "a");
        let joined = Instant::now();

        let view = ownership.view_at(joined).await;
        assert!(!view.is_active());
        assert!(owned(&view).is_empty());

        let view = ownership
            .view_at(joined + WARMUP - Duration::from_millis(1))
            .await;
        assert!(owned(&view).is_empty());

        let view = ownership.view_at(joined + WARMUP).await;
        assert!(view.is_active());
        assert_eq!(owned(&view), keys());
    }

    #[tokio::test]
    async fn members_agree_on_exactly_one_owner_per_key() {
        let nodes = Nodes::of(&[
            ("a", PeerRole::Compactor),
            ("b", PeerRole::Compactor),
            ("c", PeerRole::Compactor),
        ]);
        let mut views = Vec::new();
        for node_id in ["a", "b", "c"] {
            let (ownership, ready) = warmed(&nodes, node_id).await;
            views.push(ownership.view_at(ready).await);
        }

        for key in keys() {
            let owners = views.iter().filter(|view| view.owns(&key)).count();
            assert_eq!(owners, 1, "{key}");
        }
        assert!(views.iter().all(|view| !owned(view).is_empty()));
    }

    #[tokio::test]
    async fn only_nodes_running_the_role_share_the_work() {
        let nodes = Nodes::of(&[
            ("querier", PeerRole::Querier),
            ("compactor", PeerRole::Compactor),
            ("alert", PeerRole::Alert),
            ("standalone", PeerRole::Standalone),
        ]);

        let view = compactor(&nodes, "compactor").view().await;
        assert_eq!(view.members(), ["compactor", "standalone"]);

        // 不承担该角色的节点永远不是 owner。
        let (querier, ready) = warmed(&nodes, "querier").await;
        assert!(owned(&querier.view_at(ready).await).is_empty());
    }

    #[tokio::test]
    async fn survivors_take_over_the_keys_of_a_departed_member() {
        let nodes = Nodes::of(&[("a", PeerRole::Compactor), ("b", PeerRole::Compactor)]);
        let (ownership, ready) = warmed(&nodes, "a").await;
        let shared = owned(&ownership.view_at(ready).await);
        assert!(!shared.is_empty() && shared.len() < keys().len());

        nodes.leave("b");
        assert_eq!(owned(&ownership.view_at(ready).await), keys());
    }

    #[tokio::test]
    async fn existing_members_yield_before_a_newcomer_starts_working() {
        let nodes = Nodes::of(&[("a", PeerRole::Compactor)]);
        let (existing, ready) = warmed(&nodes, "a").await;
        assert_eq!(owned(&existing.view_at(ready).await), keys());

        nodes.join("b", PeerRole::Compactor);
        let newcomer = compactor(&nodes, "b");
        let joined = ready;
        let kept = owned(&existing.view_at(joined).await);
        assert!(!kept.is_empty() && kept.len() < keys().len());
        // 新节点预热期间，它名下的 key 暂时无人处理，而不是两边同时处理。
        assert!(owned(&newcomer.view_at(joined).await).is_empty());

        let taken = owned(&newcomer.view_at(joined + WARMUP).await);
        assert_eq!(kept.len() + taken.len(), keys().len());
        assert!(taken.iter().all(|key| !kept.contains(key)));
    }

    #[tokio::test]
    async fn a_node_dropped_from_the_view_owns_nothing_and_warms_up_again() {
        let nodes = Nodes::of(&[("a", PeerRole::Compactor), ("b", PeerRole::Compactor)]);
        let (ownership, ready) = warmed(&nodes, "a").await;
        assert!(!owned(&ownership.view_at(ready).await).is_empty());

        // 心跳过期：其余节点已经接管，本节点不再处理任何 key。
        nodes.leave("a");
        let view = ownership.view_at(ready).await;
        assert!(!view.is_active());
        assert!(owned(&view).is_empty());

        nodes.join("a", PeerRole::Compactor);
        let returned = ready + Duration::from_secs(1);
        assert!(owned(&ownership.view_at(returned).await).is_empty());
        assert!(!owned(&ownership.view_at(returned + WARMUP).await).is_empty());
    }

    #[tokio::test]
    async fn failed_discovery_pauses_ownership_without_restarting_the_warmup() {
        let nodes = Nodes::of(&[("a", PeerRole::Compactor)]);
        let (ownership, ready) = warmed(&nodes, "a").await;
        assert_eq!(owned(&ownership.view_at(ready).await), keys());

        let peers = nodes.fail_discovery();
        let view = ownership.view_at(ready).await;
        assert!(view.members().is_empty());
        assert!(owned(&view).is_empty());

        nodes.restore(peers);
        assert_eq!(owned(&ownership.view_at(ready).await), keys());
    }
}
