// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! State machine for graceful node retirement (drain).
//!
//! Process-level singleton: once triggered by an admin, the role loops observe it to
//! "stop accepting new work and flush all pending data".
//!
//! It advances monotonically through three phases and never goes back:
//! - `Running`: normal service, accepts writes.
//! - `Draining`: stops accepting new writes; the intake flush loop writes all buffers to disk,
//!   then advances to Drained.
//! - `Drained`: all pending data has been flushed, so the node can be taken offline safely.
//!
//! A node without the intake role (such as a pure querier) has no buffers to flush, so it stays
//! in `Draining`, which means "can be taken offline" (nothing advances it to Drained).

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

/// Process-level drain state, shared via `Arc` with the intake use case, the intake flush loop,
/// the compactor loop, and the admin routes.
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

    /// Triggers the drain: `Running → Draining`. Idempotent; returns `false` (no state change)
    /// if already draining or drained.
    pub fn begin_drain(&self) -> bool {
        self.phase
            .compare_exchange(RUNNING, DRAINING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// Called by intake once it has flushed all buffers: `Draining → Drained`.
    /// No-op when not draining.
    pub fn mark_drained(&self) {
        let _ = self
            .phase
            .compare_exchange(DRAINING, DRAINED, Ordering::AcqRel, Ordering::Acquire);
    }

    /// Whether retirement has begun (draining or drained); the role loops use it to stop taking
    /// new work.
    pub fn is_draining(&self) -> bool {
        self.phase.load(Ordering::Acquire) != RUNNING
    }

    /// Whether the node still accepts writes (only in `Running`).
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
