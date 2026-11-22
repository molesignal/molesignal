// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Process-level health probe: read by the `/api/v1/healthz` endpoint, written by the intake
//! and object_store probes.
//!
//! Currently covers:
//! - `intake_ready`: until the intake worker is up, the whole process returns 503 externally.
//!   It is set while the process starts, before it serves: nothing is replayed on start, since
//!   unflushed data lives in the object-storage WAL and lost writers are taken over.
//! - `object_store_degraded`: set when N consecutive object store round-trips fail
//!   (design / spec 16.9).

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Debug)]
pub struct Probe {
    intake_ready: AtomicBool,
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
            intake_ready: AtomicBool::new(false),
            object_store_degraded: AtomicBool::new(false),
        }
    }

    /// A probe that is ready from the start, with `intake_ready=true`.
    pub fn ready() -> Self {
        let p = Self::new();
        p.set_intake_ready(true);
        p
    }

    pub fn shared() -> Arc<Self> {
        Arc::new(Self::new())
    }

    pub fn set_intake_ready(&self, b: bool) {
        self.intake_ready.store(b, Ordering::Release);
    }

    pub fn is_intake_ready(&self) -> bool {
        self.intake_ready.load(Ordering::Acquire)
    }

    pub fn set_object_store_degraded(&self, b: bool) {
        self.object_store_degraded.store(b, Ordering::Release);
    }

    pub fn is_object_store_degraded(&self) -> bool {
        self.object_store_degraded.load(Ordering::Acquire)
    }

    /// Overall health status: unhealthy until the intake worker is up, or while object_store is
    /// degraded.
    pub fn is_healthy(&self) -> bool {
        self.is_intake_ready() && !self.is_object_store_degraded()
    }

    /// Returns (healthy, reason): when unhealthy, reason gives the main cause.
    pub fn snapshot(&self) -> (bool, Option<&'static str>) {
        if !self.is_intake_ready() {
            return (false, Some("intake is starting"));
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
        assert_eq!(p.snapshot(), (false, Some("intake is starting")));
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
