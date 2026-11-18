// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Search admission control：节点级 + 集群级并发槽位准入（与 QPS 限流互补）。
//!
//! 保护节点 / 集群资源不被并发查询压垮。按 **work group** 分组（角色→组的可配映射）：
//! - **节点上限** `groups`/`default_max_concurrent`（0=不限）：本进程实时计数。槽位满时查询按
//!   到达顺序排队，至多等 `max_wait`，等不到才拒绝；`max_wait` 为零时满了立刻拒绝。
//! - **集群上限** `cluster_groups`/`cluster_default_max_concurrent`（0=不限）：软上限——本地
//!   实时 in_flight + 周期 sync 得到的「其它节点合计」（[`set_cluster_view`](Self::set_cluster_view)），
//!   最终一致，实时性受 sync 间隔约束（跨节点协调由 bootstrap worker 经 DB 完成）。到上限立刻
//!   拒绝，不排队。
//!
//! 申请返回 RAII [`SlotGuard`]，drop 即释放并交给排在最前面的查询。拒绝时 caller 回 429。
//! 配置可经 [`refresh`](Self::refresh) 热更新。

mod metrics;
mod queue;

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use kernel::Error;
use parking_lot::RwLock;
use serde::Serialize;

use self::queue::{Slots, Turn};

/// 未显式映射的角色落到的默认工作组。
const DEFAULT_GROUP: &str = "default";

#[derive(Debug, Clone, Default)]
pub struct AdmissionConfig {
    /// 未在 `groups` 显式配置的工作组的**节点**并发上限；0 = 不限。
    pub default_max_concurrent: usize,
    /// 工作组名 → 节点并发上限。
    pub groups: HashMap<String, usize>,
    /// 角色名（owner/admin/editor/viewer）→ 工作组名；缺省落 `default`。
    pub role_map: HashMap<String, String>,
    /// 节点槽位满时查询排队最多等多久；零 = 满了立刻拒绝。
    pub max_wait: Duration,
    /// 未在 `cluster_groups` 配置的工作组的**集群**（全节点合计）并发上限；0 = 不限。
    pub cluster_default_max_concurrent: usize,
    /// 工作组名 → 集群并发上限。
    pub cluster_groups: HashMap<String, usize>,
}

struct GroupState {
    name: String,
    slots: Slots,
    acquired_total: AtomicU64,
    rejected_total: AtomicU64,
}

impl GroupState {
    fn refuse(&self, refusal: Refusal) -> Refusal {
        self.rejected_total.fetch_add(1, Ordering::Relaxed);

        metrics::metrics()
            .refusals
            .with_label_values(&[self.name.as_str(), refusal.reason()])
            .inc();

        refusal
    }
}

/// 单个工作组的准入快照（observability / `GET /query/admission`）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdmissionStats {
    pub group: String,
    pub limit: usize,
    pub in_flight: usize,
    /// 在本节点排队等槽位的查询数。
    pub waiting: usize,
    /// 集群上限（0=不限）。
    pub cluster_limit: usize,
    /// 其它节点在该组的 in_flight 合计（上次 sync 视图；不含本节点）。
    pub cluster_in_flight: usize,
    pub acquired_total: u64,
    pub rejected_total: u64,
}

/// 持有一个并发槽位；drop 时还回去，交给排在最前面的查询（RAII）。
pub struct SlotGuard {
    state: Arc<GroupState>,
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        self.state.slots.give_back();
    }
}

/// 没拿到槽位的原因。转成 [`Error`] 是 429（`ResourceExhausted`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// 工作组在本节点满了，且不排队（`max_wait` 为零）。
    Full,
    /// 排队等满 `max_wait` 仍没轮到。
    TimedOut(Duration),
    /// 工作组在集群里满了（软上限，不排队）。
    Cluster,
}

impl Refusal {
    fn reason(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::TimedOut(_) => "timeout",
            Self::Cluster => "cluster",
        }
    }
}

impl From<Refusal> for Error {
    fn from(refusal: Refusal) -> Self {
        let message = match refusal {
            Refusal::Full | Refusal::Cluster => {
                "search admission: too many concurrent queries for your work group; retry shortly"
                    .to_string()
            }
            Refusal::TimedOut(wait) => format!(
                "search admission: no query slot of your work group came free within {wait:?}; \
                 retry shortly"
            ),
        };

        Error::resource_exhausted(message)
    }
}

pub struct AdmissionController {
    cfg: RwLock<AdmissionConfig>,
    groups: RwLock<HashMap<String, Arc<GroupState>>>,
    /// 其它节点各工作组 in_flight 合计（周期 sync 刷新；不含本节点）。
    cluster_view: RwLock<HashMap<String, usize>>,
}

impl AdmissionController {
    pub fn new(cfg: AdmissionConfig) -> Self {
        Self {
            cfg: RwLock::new(cfg),
            groups: RwLock::new(HashMap::new()),
            cluster_view: RwLock::new(HashMap::new()),
        }
    }

    /// 角色 → 工作组名。
    pub fn resolve_group(&self, role_key: &str) -> String {
        self.cfg
            .read()
            .role_map
            .get(role_key)
            .cloned()
            .unwrap_or_else(|| DEFAULT_GROUP.to_string())
    }

    /// 节点槽位满时查询排队最多等多久。
    pub fn max_wait(&self) -> Duration {
        self.cfg.read().max_wait
    }

    /// 某工作组当前配置的节点并发上限。
    fn limit_for(&self, name: &str) -> usize {
        let cfg = self.cfg.read();

        cfg.groups
            .get(name)
            .copied()
            .unwrap_or(cfg.default_max_concurrent)
    }

    /// 某工作组当前配置的集群并发上限。
    fn cluster_limit_for(&self, name: &str) -> usize {
        let cfg = self.cfg.read();

        cfg.cluster_groups
            .get(name)
            .copied()
            .unwrap_or(cfg.cluster_default_max_concurrent)
    }

    fn group_state(&self, name: &str) -> Arc<GroupState> {
        if let Some(s) = self.groups.read().get(name) {
            return s.clone();
        }

        let mut g = self.groups.write();

        if let Some(s) = g.get(name) {
            return s.clone();
        }

        let metrics = metrics::metrics();

        let s = Arc::new(GroupState {
            name: name.to_string(),
            slots: Slots::new(
                self.limit_for(name),
                metrics.running.with_label_values(&[name]),
                metrics.waiting.with_label_values(&[name]),
            ),
            acquired_total: AtomicU64::new(0),
            rejected_total: AtomicU64::new(0),
        });

        g.insert(name.to_string(), s.clone());
        s
    }

    /// 申请某工作组的一个并发槽位。本节点满了就排队，至多等 `max_wait`；集群软上限满了立刻
    /// 拒绝。集群检查用「实时本地 + 陈旧的其它节点合计」。
    pub async fn admit(&self, group: &str) -> Result<SlotGuard, Refusal> {
        let state = self.group_state(group);
        let cluster_limit = self.cluster_limit_for(group);
        let others = self.cluster_view.read().get(group).copied().unwrap_or(0);

        if cluster_limit > 0 && others + state.slots.counts().running >= cluster_limit {
            return Err(state.refuse(Refusal::Cluster));
        }

        let wait = self.max_wait();
        let started = Instant::now();
        let turn = state.slots.take(wait).await;

        let outcome = if turn == Turn::Admitted {
            "admitted"
        } else {
            "refused"
        };

        metrics::metrics()
            .wait_seconds
            .with_label_values(&[group, outcome])
            .observe(started.elapsed().as_secs_f64());

        match turn {
            Turn::Admitted => {
                state.acquired_total.fetch_add(1, Ordering::Relaxed);
                Ok(SlotGuard { state })
            }
            Turn::Full => Err(state.refuse(Refusal::Full)),
            Turn::TimedOut => Err(state.refuse(Refusal::TimedOut(wait))),
        }
    }

    /// 按角色解析工作组并申请槽位。
    pub async fn admit_for_role(&self, role_key: &str) -> Result<SlotGuard, Refusal> {
        let group = self.resolve_group(role_key);
        self.admit(&group).await
    }

    /// 热更新配置（无需重启）：替换 cfg，并把已存在工作组的节点上限就地刷新——调高时立刻放行
    /// 排队的查询。in_flight 计数保留，`role_map` / 集群上限 / `max_wait` 变更在下次申请生效。
    /// 多节点下仅更新本节点。
    pub fn refresh(&self, cfg: AdmissionConfig) {
        *self.cfg.write() = cfg;
        let groups = self.groups.read();

        for (name, state) in groups.iter() {
            state.slots.set_limit(self.limit_for(name));
        }
    }

    /// 设置「其它节点合计」集群视图（bootstrap sync worker 周期调用）。
    pub fn set_cluster_view(&self, view: HashMap<String, usize>) {
        *self.cluster_view.write() = view;
    }

    /// 本节点各工作组当前 in_flight 快照（sync worker 发布到集群用）。
    pub fn local_load(&self) -> Vec<(String, i64)> {
        self.groups
            .read()
            .iter()
            .map(|(name, s)| (name.clone(), s.slots.counts().running as i64))
            .collect()
    }

    /// 各工作组当前快照（按组名排序）。
    pub fn stats(&self) -> Vec<AdmissionStats> {
        let view = self.cluster_view.read();
        let g = self.groups.read();

        let mut out: Vec<AdmissionStats> = g
            .iter()
            .map(|(name, s)| {
                let counts = s.slots.counts();

                AdmissionStats {
                    group: name.clone(),
                    limit: counts.limit,
                    in_flight: counts.running,
                    waiting: counts.waiting,
                    cluster_limit: self.cluster_limit_for(name),
                    cluster_in_flight: view.get(name).copied().unwrap_or(0),
                    acquired_total: s.acquired_total.load(Ordering::Relaxed),
                    rejected_total: s.rejected_total.load(Ordering::Relaxed),
                }
            })
            .collect();

        out.sort_by(|a, b| a.group.cmp(&b.group));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctrl(default: usize, groups: &[(&str, usize)]) -> AdmissionController {
        AdmissionController::new(AdmissionConfig {
            default_max_concurrent: default,
            groups: groups.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
            ..Default::default()
        })
    }

    fn waiting(c: &AdmissionController, group: &str) -> usize {
        c.stats()
            .iter()
            .find(|stats| stats.group == group)
            .map_or(0, |stats| stats.waiting)
    }

    #[tokio::test]
    async fn rejects_when_group_full_and_releases_on_drop() {
        let c = ctrl(0, &[("g", 2)]);
        let a = c.admit("g").await.unwrap();
        let b = c.admit("g").await.unwrap();
        assert_eq!(c.admit("g").await.err(), Some(Refusal::Full)); // 满，且不排队
        drop(a);
        assert!(c.admit("g").await.is_ok()); // 释放后可再申请
        drop(b);
    }

    #[tokio::test]
    async fn a_full_group_queues_until_a_slot_frees_or_the_wait_ends() {
        let c = Arc::new(AdmissionController::new(AdmissionConfig {
            groups: [("g".to_string(), 1usize)].into_iter().collect(),
            max_wait: Duration::from_secs(10),
            ..Default::default()
        }));

        let held = c.admit("g").await.unwrap();

        let queued = tokio::spawn({
            let c = c.clone();
            async move { c.admit("g").await.map(drop) }
        });

        while waiting(&c, "g") == 0 {
            tokio::task::yield_now().await;
        }

        drop(held);
        assert_eq!(queued.await.unwrap(), Ok(()));

        let _held = c.admit("g").await.unwrap();

        c.refresh(AdmissionConfig {
            groups: [("g".to_string(), 1usize)].into_iter().collect(),
            max_wait: Duration::from_millis(20),
            ..Default::default()
        });

        let refused = c.admit("g").await.err();
        assert_eq!(refused, Some(Refusal::TimedOut(Duration::from_millis(20))));
        assert_eq!(waiting(&c, "g"), 0);

        let stats = c.stats();
        let g = stats.iter().find(|x| x.group == "g").unwrap();
        assert_eq!((g.acquired_total, g.rejected_total), (3, 1));
    }

    #[test]
    fn a_refusal_answers_429() {
        let timed_out = Error::from(Refusal::TimedOut(Duration::from_secs(60)));
        assert_eq!(timed_out.http_status_code(), 429);
        assert!(timed_out.to_string().contains("60s"), "{timed_out}");
        assert_eq!(Error::from(Refusal::Full).http_status_code(), 429);
    }

    #[tokio::test]
    async fn unlimited_group_never_rejects() {
        let c = ctrl(0, &[]);
        let mut guards = Vec::new();

        for _ in 0..100 {
            guards.push(c.admit("default").await.unwrap());
        }

        assert_eq!(guards.len(), 100);
    }

    #[test]
    fn resolve_group_maps_role_then_falls_back_to_default() {
        let mut role_map = HashMap::new();
        role_map.insert("viewer".to_string(), "readonly".to_string());
        let c = AdmissionController::new(AdmissionConfig {
            role_map,
            ..Default::default()
        });
        assert_eq!(c.resolve_group("viewer"), "readonly");
        assert_eq!(c.resolve_group("admin"), "default");
    }

    #[tokio::test]
    async fn stats_reports_inflight_and_totals() {
        let c = ctrl(0, &[("g", 1)]);
        let _a = c.admit("g").await.unwrap();
        assert!(c.admit("g").await.is_err());
        let stats = c.stats();
        let g = stats.iter().find(|x| x.group == "g").unwrap();
        assert_eq!(g.in_flight, 1);
        assert_eq!(g.waiting, 0);
        assert_eq!(g.limit, 1);
        assert_eq!(g.acquired_total, 1);
        assert_eq!(g.rejected_total, 1);
    }

    #[tokio::test]
    async fn refresh_updates_limits_in_place() {
        let c = ctrl(0, &[("g", 1)]);
        let _a = c.admit("g").await.unwrap();
        assert!(c.admit("g").await.is_err()); // 上限 1，满
        c.refresh(AdmissionConfig {
            groups: [("g".to_string(), 3usize)].into_iter().collect(),
            ..Default::default()
        });
        let _b = c.admit("g").await.unwrap();
        let _d = c.admit("g").await.unwrap();
        assert!(c.admit("g").await.is_err()); // 又满（in_flight=3）
    }

    #[tokio::test]
    async fn cluster_soft_limit_blocks_with_other_nodes_load() {
        // 节点上限不限，集群上限 5；其它节点已占 4。排队也不等集群里的槽位。
        let c = AdmissionController::new(AdmissionConfig {
            cluster_groups: [("g".to_string(), 5usize)].into_iter().collect(),
            max_wait: Duration::from_secs(10),
            ..Default::default()
        });
        c.set_cluster_view([("g".to_string(), 4usize)].into_iter().collect());
        // 本地 0 + 其它 4 < 5 → 放行第 1 个（本地变 1，合计 5）。
        let _a = c.admit("g").await.unwrap();
        // 本地 1 + 其它 4 = 5 ≥ 5 → 拒。
        assert_eq!(c.admit("g").await.err(), Some(Refusal::Cluster));
    }

    #[tokio::test]
    async fn local_load_snapshots_per_group_inflight() {
        let c = ctrl(0, &[("g", 10)]);
        let _a = c.admit("g").await.unwrap();
        let _b = c.admit("g").await.unwrap();
        let load: HashMap<String, i64> = c.local_load().into_iter().collect();
        assert_eq!(load.get("g").copied(), Some(2));
    }
}
