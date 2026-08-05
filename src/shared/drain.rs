// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 节点优雅退役（drain）状态机。
//!
//! 进程级单例：被 admin 触发后，角色循环观察它来「停接新活 + 把 pending 数据 flush 干净」。
//!
//! 三相单调推进，不可回退：
//! - `Running`  —— 正常服务，接受写入。
//! - `Draining` —— 停接新写入；ingester flush loop 把全部 buffer 落盘后推进到 Drained。
//! - `Drained`  —— pending 数据已全部 flush，可安全下线该节点。
//!
//! 无 ingester 角色的节点（如纯 querier）没有待 flush 的 buffer，停在 `Draining` 即「可下线」
//! 语义（无人推进到 Drained）。

use std::sync::atomic::{AtomicU8, Ordering};

const RUNNING: u8 = 0;
const DRAINING: u8 = 1;
const DRAINED: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrainPhase {
    Running,
    Draining,
    Drained,
}

impl DrainPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            DrainPhase::Running => "running",
            DrainPhase::Draining => "draining",
            DrainPhase::Drained => "drained",
        }
    }
}

/// 进程级 drain 状态。`Arc` 共享给 ingest 用例、ingester flush loop、compactor loop、admin 路由。
#[derive(Debug)]
pub struct DrainController {
    phase: AtomicU8,
}

impl DrainController {
    pub fn new() -> Self {
        Self {
            phase: AtomicU8::new(RUNNING),
        }
    }

    pub fn phase(&self) -> DrainPhase {
        match self.phase.load(Ordering::Acquire) {
            RUNNING => DrainPhase::Running,
            DRAINING => DrainPhase::Draining,
            _ => DrainPhase::Drained,
        }
    }

    /// 触发退役：`Running → Draining`。幂等；已在 draining/drained 时返回 `false`（无状态变更）。
    pub fn begin_drain(&self) -> bool {
        self.phase
            .compare_exchange(RUNNING, DRAINING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// ingester 把 buffer 全部 flush 完后调用：`Draining → Drained`。非 draining 时 no-op。
    pub fn mark_drained(&self) {
        let _ = self
            .phase
            .compare_exchange(DRAINING, DRAINED, Ordering::AcqRel, Ordering::Acquire);
    }

    /// 是否已进入退役（draining 或 drained）——角色循环据此停接新活。
    pub fn is_draining(&self) -> bool {
        self.phase.load(Ordering::Acquire) != RUNNING
    }

    /// 是否仍接受写入（仅 `Running`）。
    pub fn accepts_writes(&self) -> bool {
        self.phase.load(Ordering::Acquire) == RUNNING
    }
}

impl Default for DrainController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_running_and_accepts_writes() {
        let d = DrainController::new();
        assert_eq!(d.phase(), DrainPhase::Running);
        assert!(d.accepts_writes());
        assert!(!d.is_draining());
    }

    #[test]
    fn begin_drain_stops_writes_and_is_idempotent() {
        let d = DrainController::new();
        assert!(d.begin_drain(), "first transition reported");
        assert_eq!(d.phase(), DrainPhase::Draining);
        assert!(!d.accepts_writes(), "draining stops new writes");
        assert!(d.is_draining());
        assert!(!d.begin_drain(), "second begin_drain is a no-op");
    }

    #[test]
    fn mark_drained_only_from_draining() {
        let d = DrainController::new();
        // Running → mark_drained is a no-op (must drain first).
        d.mark_drained();
        assert_eq!(d.phase(), DrainPhase::Running);
        // Draining → Drained.
        d.begin_drain();
        d.mark_drained();
        assert_eq!(d.phase(), DrainPhase::Drained);
        assert!(
            d.is_draining(),
            "drained still counts as draining (no new work)"
        );
        assert!(!d.accepts_writes());
    }
}
