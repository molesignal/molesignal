// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 分片后台工作的运行准入：先看归属，再认领运行租约。
//!
//! 归属（[`WorkOwnership`]）决定正常情况下由哪个节点跑某个工作单元；租约
//! （[`RunClaimRepository`]）兜住成员变化时两个节点同时认为自己是 owner 的短暂窗口，也让崩溃
//! 节点的单元在租约到期后由别的节点重跑。

use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};

use async_trait::async_trait;
use prometheus::IntCounterVec;

use super::WorkOwnership;
use crate::{
    domain::run_claim::{RunClaimRepository, RunGate, RunPermit},
    shared::{Result, metrics::register_int_counter_vec, time::TimestampMicros},
};

/// 运行租约的默认时长：覆盖一次报告渲染与投递、一次 connector 拉取或一次 pipeline 运行。
/// 持有者崩溃后，别的节点最多再等这么久才会重跑它的单元。
pub const DEFAULT_LEASE: Duration = Duration::from_secs(10 * 60);

pub struct ShardedRunGate {
    ownership: Arc<WorkOwnership>,
    claims: Arc<dyn RunClaimRepository>,
    holder: String,
    lease: Duration,
}

impl ShardedRunGate {
    /// `holder` 是本节点 ID，写进租约里便于排查；同一集群里必须唯一。
    pub fn new(
        ownership: Arc<WorkOwnership>,
        claims: Arc<dyn RunClaimRepository>,
        holder: impl Into<String>,
    ) -> Self {
        Self {
            ownership,
            claims,
            holder: holder.into(),
            lease: DEFAULT_LEASE,
        }
    }

    pub fn with_lease(mut self, lease: Duration) -> Self {
        self.lease = lease;
        self
    }
}

#[async_trait]
impl RunGate for ShardedRunGate {
    async fn admit(&self, kind: &'static str, key: &str) -> Result<Option<RunPermit>> {
        // 不是 owner 的节点不碰数据库：正常运行时每个单元只有一个节点来认领。
        if !self.ownership.owns(key).await {
            return Ok(None);
        }
        let now = TimestampMicros::now();
        let lease_micros = i64::try_from(self.lease.as_micros()).unwrap_or(i64::MAX);
        let until = TimestampMicros(now.0.saturating_add(lease_micros));
        if self
            .claims
            .try_claim(kind, key, &self.holder, now, until)
            .await?
        {
            Ok(Some(RunPermit::claimed(
                self.claims.clone(),
                kind,
                key,
                self.holder.clone(),
            )))
        } else {
            record_overlap_skipped(kind);
            Ok(None)
        }
    }
}

/// 记一次「另一个节点已经在这个周期里跑过它」而被跳过的工作单元。
pub fn record_overlap_skipped(work: &str) {
    static METRIC: OnceLock<IntCounterVec> = OnceLock::new();
    METRIC
        .get_or_init(|| {
            register_int_counter_vec(
                "work_overlap_skipped_total",
                "Work units skipped because another node already ran them in the same period",
                &["work"],
            )
        })
        .with_label_values(&[work])
        .inc();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::cluster::{PeerRole, ownership::test_support::Nodes},
        domain::run_claim::MemoryRunClaims,
    };

    const KIND: &str = "scheduled_report";

    fn gate(nodes: &Arc<Nodes>, node_id: &str, claims: &Arc<MemoryRunClaims>) -> ShardedRunGate {
        let ownership = WorkOwnership::new(nodes.clone(), node_id, PeerRole::Alert, Duration::ZERO);
        ShardedRunGate::new(ownership, claims.clone(), node_id)
    }

    fn keys() -> Vec<String> {
        (0..40).map(|index| format!("report-{index}")).collect()
    }

    #[tokio::test]
    async fn each_unit_is_admitted_on_exactly_one_member() {
        let nodes = Nodes::of(&[("a", PeerRole::Alert), ("b", PeerRole::Alert)]);
        let claims = Arc::new(MemoryRunClaims::default());
        let (a, b) = (gate(&nodes, "a", &claims), gate(&nodes, "b", &claims));

        let (mut on_a, mut on_b) = (0, 0);
        for key in keys() {
            let permit_a = a.admit(KIND, &key).await.unwrap();
            let permit_b = b.admit(KIND, &key).await.unwrap();
            assert!(permit_a.is_some() ^ permit_b.is_some(), "{key}");
            on_a += usize::from(permit_a.is_some());
            on_b += usize::from(permit_b.is_some());
        }
        assert!(on_a > 0 && on_b > 0, "{on_a} + {on_b}");
    }

    #[tokio::test]
    async fn an_overlapping_owner_is_turned_away_until_the_lease_is_released() {
        // 两个节点各自只看到自己，都认为自己拥有全部单元：成员变化窗口里的双跑。
        let claims = Arc::new(MemoryRunClaims::default());
        let a = gate(&Nodes::of(&[("a", PeerRole::Alert)]), "a", &claims);
        let b = gate(&Nodes::of(&[("b", PeerRole::Alert)]), "b", &claims);

        let permit = a
            .admit(KIND, "report-1")
            .await
            .unwrap()
            .expect("a claims it");
        assert!(b.admit(KIND, "report-1").await.unwrap().is_none());
        // 别的单元不受影响。
        assert!(b.admit(KIND, "report-2").await.unwrap().is_some());

        permit.release().await.unwrap();
        assert!(b.admit(KIND, "report-1").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn a_node_outside_the_view_admits_nothing() {
        let nodes = Nodes::of(&[("a", PeerRole::Alert)]);
        let claims = Arc::new(MemoryRunClaims::default());
        let stranger = gate(&nodes, "stranger", &claims);
        for key in keys() {
            assert!(stranger.admit(KIND, &key).await.unwrap().is_none());
        }
    }

    #[tokio::test]
    async fn leases_of_different_kinds_do_not_collide() {
        let claims = Arc::new(MemoryRunClaims::default());
        let a = gate(&Nodes::of(&[("a", PeerRole::Alert)]), "a", &claims);
        let b = gate(&Nodes::of(&[("b", PeerRole::Alert)]), "b", &claims);

        let _report = a
            .admit("scheduled_report", "same-id")
            .await
            .unwrap()
            .unwrap();
        assert!(
            b.admit("scheduled_pipeline", "same-id")
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn the_primary_is_a_single_member_and_moves_when_it_leaves() {
        let nodes = Nodes::of(&[
            ("a", PeerRole::Alert),
            ("b", PeerRole::Alert),
            ("c", PeerRole::Alert),
        ]);
        let ownership = |node_id: &str| {
            WorkOwnership::new(nodes.clone(), node_id, PeerRole::Alert, Duration::ZERO)
        };
        let members = ["a", "b", "c"].map(ownership);

        let mut primaries = Vec::new();
        for (index, member) in members.iter().enumerate() {
            if member.is_primary().await {
                primaries.push(index);
            }
        }
        assert_eq!(primaries.len(), 1);

        // 主节点离开后，剩下的节点里恰有一个接任。
        let departed = ["a", "b", "c"][primaries[0]];
        nodes.leave(departed);
        let mut successors = 0;
        for member in &members {
            successors += usize::from(member.is_primary().await);
        }
        assert_eq!(successors, 1);
    }

    #[tokio::test]
    async fn a_solo_node_owns_everything_and_is_always_primary() {
        let solo = WorkOwnership::solo(PeerRole::Alert);
        assert!(solo.is_primary().await);
        for key in keys() {
            assert!(solo.owns(&key).await);
        }
    }

    #[tokio::test]
    async fn memory_claims_expire_and_allow_the_holder_to_reclaim() {
        let claims = MemoryRunClaims::default();
        let at = |micros: i64| TimestampMicros(micros);

        assert!(
            claims
                .try_claim(KIND, "k", "a", at(0), at(10))
                .await
                .unwrap()
        );
        // 租约有效期内别人认领不了，持有者自己可以接着认领。
        assert!(
            !claims
                .try_claim(KIND, "k", "b", at(5), at(15))
                .await
                .unwrap()
        );
        assert!(
            claims
                .try_claim(KIND, "k", "a", at(5), at(15))
                .await
                .unwrap()
        );
        // 到期（`<= now`）后别人可以接手；原持有者的释放不能动别人的租约。
        assert!(
            claims
                .try_claim(KIND, "k", "b", at(15), at(25))
                .await
                .unwrap()
        );
        claims.release(KIND, "k", "a").await.unwrap();
        assert!(
            !claims
                .try_claim(KIND, "k", "c", at(16), at(26))
                .await
                .unwrap()
        );
    }
}
