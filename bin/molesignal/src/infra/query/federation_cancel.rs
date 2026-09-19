// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 跨集群查询取消表（进程内）。
//!
//! coordinator 发起联邦查询时给每条查询一个 `federation_query_id`，随 `QueryShard`
//! 下发到远端。远端 Flight `do_get` 收到带 id 的分片时在此表登记一个 cancel 标志，并把
//! 子查询执行与该标志 race（合作式取消，与本地 [`crate`] 的查询中断同机制）。
//!
//! coordinator 取消联邦查询时，除了丢弃本地 future（gRPC 流断开兜底），还会对曾参与的
//! 远端集群发 `EventService.CancelQuery(fed_id)`，远端 handler 经 [`FederationCancelRegistry::cancel`]
//! 置位对应标志 —— 即便远端没及时感知流断开，也能被显式中断。

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use parking_lot::RwLock;

#[derive(Default)]
pub struct FederationCancelRegistry {
    /// 远端侧：fed_id → 本地子查询的 cancel 标志（`do_get` 登记，`CancelQuery` 置位）。
    inner: RwLock<HashMap<String, Arc<AtomicBool>>>,
    /// coordinator 侧：fed_id → 实际派发到的远端集群 id 列表（cancel 路由据此**只向参与者**
    /// fan-out，不广播全网）。查询结束即清理。
    dispatch: RwLock<HashMap<String, Vec<String>>>,
}

impl FederationCancelRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 远端 `do_get` 登记一条联邦子查询，返回其 cancel 标志。完成后调 [`Self::deregister`]。
    pub fn register(&self, fed_id: &str) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.inner.write().insert(fed_id.to_string(), flag.clone());
        flag
    }

    pub fn deregister(&self, fed_id: &str) {
        self.inner.write().remove(fed_id);
    }

    /// `CancelQuery` handler：置位 `fed_id` 的标志；返回是否命中（本节点确有该在跑查询）。
    pub fn cancel(&self, fed_id: &str) -> bool {
        match self.inner.read().get(fed_id) {
            Some(f) => {
                f.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    /// coordinator：记录某联邦查询实际派发到的远端集群 id（查询期间有效，结束 [`Self::clear_dispatch`]）。
    pub fn track_dispatch(&self, fed_id: &str, cluster_ids: Vec<String>) {
        if fed_id.is_empty() {
            return;
        }
        self.dispatch
            .write()
            .insert(fed_id.to_string(), cluster_ids);
    }

    /// cancel 路由：取某 fed_id 的参与集群 id（None = 未跟踪 / 查询已结束）。
    pub fn dispatched(&self, fed_id: &str) -> Option<Vec<String>> {
        self.dispatch.read().get(fed_id).cloned()
    }

    pub fn clear_dispatch(&self, fed_id: &str) {
        self.dispatch.write().remove(fed_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_cancel_deregister() {
        let reg = FederationCancelRegistry::new();
        let flag = reg.register("fed-1");
        assert!(!flag.load(Ordering::Relaxed));
        // 命中并置位。
        assert!(reg.cancel("fed-1"));
        assert!(flag.load(Ordering::Relaxed));
        // 未知 id → 不命中。
        assert!(!reg.cancel("nope"));
        // 注销后不再命中。
        reg.deregister("fed-1");
        assert!(!reg.cancel("fed-1"));
    }

    #[test]
    fn dispatch_tracking() {
        let reg = FederationCancelRegistry::new();
        assert!(reg.dispatched("fed-1").is_none());
        reg.track_dispatch("fed-1", vec!["cl-a".into(), "cl-b".into()]);
        assert_eq!(reg.dispatched("fed-1").unwrap(), vec!["cl-a", "cl-b"]);
        // 空 fed_id 不记录。
        reg.track_dispatch("", vec!["x".into()]);
        assert!(reg.dispatched("").is_none());
        reg.clear_dispatch("fed-1");
        assert!(reg.dispatched("fed-1").is_none());
    }
}
