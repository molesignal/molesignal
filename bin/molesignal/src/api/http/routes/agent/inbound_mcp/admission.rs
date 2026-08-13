// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};

const RATE_WINDOW: Duration = Duration::from_secs(60);
pub(super) const IDLE_RETENTION: Duration = Duration::from_secs(10 * 60);

#[derive(Debug)]
pub(super) enum AdmissionRejection {
    Concurrent,
    RateLimited { retry_after_seconds: u64 },
}

#[derive(Debug)]
pub(super) struct CredentialAdmission {
    in_flight: AtomicU32,
    calls: Mutex<VecDeque<Instant>>,
    last_seen: Mutex<Instant>,
}

impl Default for CredentialAdmission {
    fn default() -> Self {
        Self {
            in_flight: AtomicU32::new(0),
            calls: Mutex::new(VecDeque::new()),
            last_seen: Mutex::new(Instant::now()),
        }
    }
}

impl CredentialAdmission {
    pub fn enter(
        self: &Arc<Self>,
        max_concurrent: u32,
        calls_per_minute: usize,
    ) -> Result<AdmissionGuard, AdmissionRejection> {
        let previous = self.in_flight.fetch_add(1, Ordering::AcqRel);
        if previous >= max_concurrent {
            self.in_flight.fetch_sub(1, Ordering::AcqRel);
            return Err(AdmissionRejection::Concurrent);
        }

        let now = Instant::now();
        *self
            .last_seen
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = now;
        let mut calls = self.calls.lock().unwrap_or_else(|error| error.into_inner());
        while calls
            .front()
            .is_some_and(|started| now.saturating_duration_since(*started) >= RATE_WINDOW)
        {
            calls.pop_front();
        }
        if calls.len() >= calls_per_minute {
            let retry_after_seconds = calls
                .front()
                .map(|started| {
                    RATE_WINDOW
                        .saturating_sub(now.saturating_duration_since(*started))
                        .as_secs()
                        .max(1)
                })
                .unwrap_or(1);
            drop(calls);
            self.in_flight.fetch_sub(1, Ordering::AcqRel);
            return Err(AdmissionRejection::RateLimited {
                retry_after_seconds,
            });
        }
        calls.push_back(now);
        drop(calls);

        Ok(AdmissionGuard {
            admission: Arc::clone(self),
        })
    }

    pub fn is_idle_at(&self, now: Instant) -> bool {
        self.in_flight.load(Ordering::Acquire) == 0
            && self
                .last_seen
                .lock()
                .map(|last_seen| now.saturating_duration_since(*last_seen) >= IDLE_RETENTION)
                .unwrap_or(false)
    }
}

pub(super) struct AdmissionGuard {
    admission: Arc<CredentialAdmission>,
}

impl Drop for AdmissionGuard {
    fn drop(&mut self) {
        self.admission.in_flight.fetch_sub(1, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforces_concurrency_and_releases_on_drop() {
        let admission = Arc::new(CredentialAdmission::default());
        let first = admission.enter(1, 60).expect("first call admitted");
        assert!(matches!(
            admission.enter(1, 60),
            Err(AdmissionRejection::Concurrent)
        ));
        drop(first);
        assert!(admission.enter(1, 60).is_ok());
    }

    #[test]
    fn enforces_per_minute_limit() {
        let admission = Arc::new(CredentialAdmission::default());
        drop(admission.enter(8, 1).expect("first call admitted"));
        assert!(matches!(
            admission.enter(8, 1),
            Err(AdmissionRejection::RateLimited { .. })
        ));
    }
}
