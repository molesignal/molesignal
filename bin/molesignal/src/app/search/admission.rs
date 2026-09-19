// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Search admission control：节点级 + 集群级并发槽位准入（与 QPS 限流互补）。
//!
//! 与 `infra::quotas`（per-org QPS 速率限制）不同，本控制器限制**同时执行**的查询数，
//! 保护节点 / 集群资源不被并发搜索压垮。按 **work group** 分组（角色→组的可配映射）：
//! - **节点上限** `groups`/`default_max_concurrent`（0=不限）：硬实时（本进程原子计数）；
//! - **集群上限** `cluster_groups`/`cluster_default_max_concurrent`（0=不限）：软上限——本地
//!   实时 in_flight + 周期 sync 得到的「其它节点合计」（[`set_cluster_view`](Self::set_cluster_view)），
//!   最终一致，实时性受 sync 间隔约束（跨节点协调由 bootstrap worker 经 DB 完成）。
//!
//! 申请返回 RAII [`SlotGuard`]，drop 即释放。满则拒绝（caller 回 429）。配置可经
//! [`refresh`](Self::refresh) 热更新。纯内存 + domain-only 依赖，可单测。

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
};

use parking_lot::RwLock;
use serde::Serialize;

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
    /// 未在 `cluster_groups` 配置的工作组的**集群**（全节点合计）并发上限；0 = 不限。
    pub cluster_default_max_concurrent: usize,
    /// 工作组名 → 集群并发上限。
    pub cluster_groups: HashMap<String, usize>,
}

struct GroupState {
    /// 节点并发上限（可经 refresh 就地热更新）；0 = 不限。
    limit: AtomicUsize,
    in_flight: AtomicUsize,
    acquired_total: AtomicU64,
    rejected_total: AtomicU64,
}

/// 单个工作组的准入快照（observability / `GET /query/admission`）。
#[derive(Debug, Clone, Serialize)]
pub struct AdmissionStats {
    pub group: String,
    pub limit: usize,
    pub in_flight: usize,
    /// 集群上限（0=不限）。
    pub cluster_limit: usize,
    /// 其它节点在该组的 in_flight 合计（上次 sync 视图；不含本节点）。
    pub cluster_in_flight: usize,
    pub acquired_total: u64,
    pub rejected_total: u64,
}

/// 持有一个并发槽位；drop 时释放（RAII）。
pub struct SlotGuard {
    state: Arc<GroupState>,
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        self.state.in_flight.fetch_sub(1, Ordering::Relaxed);
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
        let s = Arc::new(GroupState {
            limit: AtomicUsize::new(self.limit_for(name)),
            in_flight: AtomicUsize::new(0),
            acquired_total: AtomicU64::new(0),
            rejected_total: AtomicU64::new(0),
        });
        g.insert(name.to_string(), s.clone());
        s
    }

    /// 申请某工作组的一个并发槽位；满（节点或集群上限）则返回 `None`（caller 应回 429）。
    /// 节点检查用实时本地计数；集群检查用「实时本地 + 陈旧的其它节点合计」（软上限）。
    pub fn acquire(&self, group: &str) -> Option<SlotGuard> {
        let state = self.group_state(group);
        let limit = state.limit.load(Ordering::Relaxed);
        let cluster_limit = self.cluster_limit_for(group);
        let others = self.cluster_view.read().get(group).copied().unwrap_or(0);
        loop {
            let cur = state.in_flight.load(Ordering::Relaxed);
            if limit > 0 && cur >= limit {
                state.rejected_total.fetch_add(1, Ordering::Relaxed);
                return None;
            }
            if cluster_limit > 0 && others + cur >= cluster_limit {
                state.rejected_total.fetch_add(1, Ordering::Relaxed);
                return None;
            }
            if state
                .in_flight
                .compare_exchange_weak(cur, cur + 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                state.acquired_total.fetch_add(1, Ordering::Relaxed);
                return Some(SlotGuard {
                    state: state.clone(),
                });
            }
        }
    }

    /// 按角色解析工作组并申请槽位。
    pub fn acquire_for_role(&self, role_key: &str) -> Option<SlotGuard> {
        let group = self.resolve_group(role_key);
        self.acquire(&group)
    }

    /// 热更新配置（无需重启）：替换 cfg，并把已存在工作组的节点上限就地刷新；in_flight
    /// 计数保留，`role_map` / 集群上限变更在下次 resolve / acquire 生效。多节点下仅更新本节点。
    pub fn refresh(&self, cfg: AdmissionConfig) {
        *self.cfg.write() = cfg;
        let groups = self.groups.read();
        for (name, state) in groups.iter() {
            state.limit.store(self.limit_for(name), Ordering::Relaxed);
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
            .map(|(name, s)| (name.clone(), s.in_flight.load(Ordering::Relaxed) as i64))
            .collect()
    }

    /// 各工作组当前快照（按组名排序）。
    pub fn stats(&self) -> Vec<AdmissionStats> {
        let view = self.cluster_view.read();
        let g = self.groups.read();
        let mut out: Vec<AdmissionStats> = g
            .iter()
            .map(|(name, s)| AdmissionStats {
                group: name.clone(),
                limit: s.limit.load(Ordering::Relaxed),
                in_flight: s.in_flight.load(Ordering::Relaxed),
                cluster_limit: self.cluster_limit_for(name),
                cluster_in_flight: view.get(name).copied().unwrap_or(0),
                acquired_total: s.acquired_total.load(Ordering::Relaxed),
                rejected_total: s.rejected_total.load(Ordering::Relaxed),
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

    #[test]
    fn rejects_when_group_full_and_releases_on_drop() {
        let c = ctrl(0, &[("g", 2)]);
        let a = c.acquire("g").unwrap();
        let b = c.acquire("g").unwrap();
        assert!(c.acquire("g").is_none()); // 满
        drop(a);
        assert!(c.acquire("g").is_some()); // 释放后可再申请
        drop(b);
    }

    #[test]
    fn unlimited_group_never_rejects() {
        let c = ctrl(0, &[]);
        let guards: Vec<_> = (0..100).filter_map(|_| c.acquire("default")).collect();
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

    #[test]
    fn stats_reports_inflight_and_totals() {
        let c = ctrl(0, &[("g", 1)]);
        let _a = c.acquire("g").unwrap();
        assert!(c.acquire("g").is_none());
        let stats = c.stats();
        let g = stats.iter().find(|x| x.group == "g").unwrap();
        assert_eq!(g.in_flight, 1);
        assert_eq!(g.limit, 1);
        assert_eq!(g.acquired_total, 1);
        assert_eq!(g.rejected_total, 1);
    }

    #[test]
    fn refresh_updates_limits_in_place() {
        let c = ctrl(0, &[("g", 1)]);
        let _a = c.acquire("g").unwrap();
        assert!(c.acquire("g").is_none()); // 上限 1，满
        c.refresh(AdmissionConfig {
            groups: [("g".to_string(), 3usize)].into_iter().collect(),
            ..Default::default()
        });
        let _b = c.acquire("g").unwrap();
        let _d = c.acquire("g").unwrap();
        assert!(c.acquire("g").is_none()); // 又满（in_flight=3）
    }

    #[test]
    fn cluster_soft_limit_blocks_with_other_nodes_load() {
        // 节点上限不限，集群上限 5；其它节点已占 4。
        let c = AdmissionController::new(AdmissionConfig {
            cluster_groups: [("g".to_string(), 5usize)].into_iter().collect(),
            ..Default::default()
        });
        c.set_cluster_view([("g".to_string(), 4usize)].into_iter().collect());
        // 本地 0 + 其它 4 < 5 → 放行第 1 个（本地变 1，合计 5）。
        let _a = c.acquire("g").unwrap();
        // 本地 1 + 其它 4 = 5 ≥ 5 → 拒。
        assert!(c.acquire("g").is_none());
    }

    #[test]
    fn local_load_snapshots_per_group_inflight() {
        let c = ctrl(0, &[("g", 10)]);
        let _a = c.acquire("g").unwrap();
        let _b = c.acquire("g").unwrap();
        let load: HashMap<String, i64> = c.local_load().into_iter().collect();
        assert_eq!(load.get("g").copied(), Some(2));
    }
}
