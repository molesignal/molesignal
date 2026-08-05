// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 进程级 health probe：被 `/api/v1/healthz` 端点读，被 ingester / object_store probe 写。
//!
//! 当前覆盖：
//! - `replay_done`：ingester WAL replay 完成前，整进程对外返 503。
//! - `object_store_degraded`：连续 N 次对象存储 round-trip 失败时 set（design / spec 16.9）。

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Debug)]
pub struct Probe {
    replay_done: AtomicBool,
    object_store_degraded: AtomicBool,
}

impl Default for Probe {
    fn default() -> Self {
        Self::new()
    }
}

impl Probe {
    pub fn new() -> Self {
        Self {
            replay_done: AtomicBool::new(false),
            object_store_degraded: AtomicBool::new(false),
        }
    }

    /// 没有 ingester role 的进程（如 standalone querier-only）启动即 ready：
    /// 此 helper 直接构造一个 `replay_done=true` 的 probe。
    pub fn ready() -> Self {
        let p = Self::new();
        p.set_replay_done(true);
        p
    }

    pub fn shared() -> Arc<Self> {
        Arc::new(Self::new())
    }

    pub fn set_replay_done(&self, b: bool) {
        self.replay_done.store(b, Ordering::Release);
    }

    pub fn is_replay_done(&self) -> bool {
        self.replay_done.load(Ordering::Acquire)
    }

    pub fn set_object_store_degraded(&self, b: bool) {
        self.object_store_degraded.store(b, Ordering::Release);
    }

    pub fn is_object_store_degraded(&self) -> bool {
        self.object_store_degraded.load(Ordering::Acquire)
    }

    /// 综合健康状态：replay 未完或 object_store degraded → unhealthy。
    pub fn is_healthy(&self) -> bool {
        self.is_replay_done() && !self.is_object_store_degraded()
    }

    /// 返回 (healthy, reason)：reason 在 unhealthy 时给出主因。
    pub fn snapshot(&self) -> (bool, Option<&'static str>) {
        if !self.is_replay_done() {
            return (false, Some("ingester wal replay in progress"));
        }
        if self.is_object_store_degraded() {
            return (false, Some("object store unreachable"));
        }
        (true, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_not_healthy() {
        let p = Probe::new();
        assert!(!p.is_healthy());
        assert!(!p.snapshot().0);
    }

    #[test]
    fn ready_helper_is_healthy() {
        let p = Probe::ready();
        assert!(p.is_healthy());
    }

    #[test]
    fn object_store_degraded_overrides_ready() {
        let p = Probe::ready();
        p.set_object_store_degraded(true);
        assert!(!p.is_healthy());
        assert_eq!(p.snapshot().1, Some("object store unreachable"));
    }
}
