// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Process-level health probe: read by the `/api/v1/healthz` endpoint, written by the intake
//! and object_store probes.
//!
//! Currently covers:
//! - `replay_done`: until the intake WAL replay completes, the whole process returns 503
//!   externally.
//! - `object_store_degraded`: set when N consecutive object store round-trips fail
//!   (design / spec 16.9).

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

    /// A process without the intake role (such as a standalone querier-only process) is ready
    /// at startup, so this helper directly builds a probe with `replay_done=true`.
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

    /// Overall health status: unhealthy if replay has not finished or object_store is degraded.
    pub fn is_healthy(&self) -> bool {
        self.is_replay_done() && !self.is_object_store_degraded()
    }

    /// Returns (healthy, reason): when unhealthy, reason gives the main cause.
    pub fn snapshot(&self) -> (bool, Option<&'static str>) {
        if !self.is_replay_done() {
            return (false, Some("intake wal replay in progress"));
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
