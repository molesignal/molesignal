// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Prometheus active-series admission for the current consistent-hash ingester owner.
//!
//! Only 128-bit SHA-256 prefixes are retained. Metric names and label material never enter the
//! registry, logs, errors, or metric labels.

use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use dashmap::DashMap;
use parking_lot::Mutex;
use sha2::{Digest, Sha256};

use super::metrics::{add_active_series, inc_series_rejection};
use crate::{config::PrometheusCardinalitySettings, shared::ids::Id};

const RATE_WINDOW: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Fingerprint([u8; 16]);

/// Canonical metric + series hashes. Debug output is safe because it contains no raw labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeriesIdentity {
    metric: Fingerprint,
    series: Fingerprint,
}

impl SeriesIdentity {
    pub fn from_labels<'a>(
        metric_name: &str,
        labels: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Self {
        let mut labels = labels.into_iter().collect::<Vec<_>>();
        labels.sort_unstable();

        let mut metric_hasher = Sha256::new();
        update_segment(&mut metric_hasher, b"metric");
        update_segment(&mut metric_hasher, metric_name.as_bytes());
        let metric = finish_fingerprint(metric_hasher);

        let mut series_hasher = Sha256::new();
        update_segment(&mut series_hasher, b"series");
        update_segment(&mut series_hasher, metric_name.as_bytes());
        for (name, value) in labels {
            update_segment(&mut series_hasher, name.as_bytes());
            update_segment(&mut series_hasher, value.as_bytes());
        }
        Self {
            metric,
            series: finish_fingerprint(series_hasher),
        }
    }
}

fn update_segment(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

fn finish_fingerprint(hasher: Sha256) -> Fingerprint {
    let digest = hasher.finalize();
    let mut value = [0u8; 16];
    value.copy_from_slice(&digest[..16]);
    Fingerprint(value)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeriesLimitReason {
    ProcessActive,
    OrganizationActive,
    MetricActive,
    NewSeriesRate,
}

impl SeriesLimitReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProcessActive => "process_active",
            Self::OrganizationActive => "org_active",
            Self::MetricActive => "metric_active",
            Self::NewSeriesRate => "new_series_rate",
        }
    }

    pub const fn client_message(self) -> &'static str {
        match self {
            Self::ProcessActive => "Prometheus process active-series limit exceeded",
            Self::OrganizationActive => "Prometheus organization active-series limit exceeded",
            Self::MetricActive => "Prometheus metric active-series limit exceeded",
            Self::NewSeriesRate => "Prometheus new-series rate limit exceeded; retry later",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SeriesAdmissionOutcome {
    pub existing_series: usize,
    pub new_series: usize,
}

#[derive(Debug, Clone, Copy)]
struct SeriesEntry {
    metric: Fingerprint,
    last_seen: Instant,
}

struct OrgSeriesState {
    active: HashMap<Fingerprint, SeriesEntry>,
    metric_counts: HashMap<Fingerprint, usize>,
    expiry: BinaryHeap<Reverse<(Instant, Fingerprint)>>,
    rate_window_started: Instant,
    new_in_window: usize,
}

impl OrgSeriesState {
    fn new(now: Instant) -> Self {
        Self {
            active: HashMap::new(),
            metric_counts: HashMap::new(),
            expiry: BinaryHeap::new(),
            rate_window_started: now,
            new_in_window: 0,
        }
    }

    fn expire_idle(&mut self, now: Instant, idle_ttl: Duration) -> usize {
        let mut removed = 0usize;
        while let Some(Reverse((expires_at, fingerprint))) = self.expiry.peek().copied() {
            if expires_at > now {
                break;
            }
            self.expiry.pop();
            let Some(entry) = self.active.get(&fingerprint).copied() else {
                continue;
            };
            let current_expiry = entry
                .last_seen
                .checked_add(idle_ttl)
                .expect("validated series idle TTL must fit Instant");
            if current_expiry > now {
                self.expiry.push(Reverse((current_expiry, fingerprint)));
                continue;
            }
            self.active.remove(&fingerprint);
            if let Some(count) = self.metric_counts.get_mut(&entry.metric) {
                *count -= 1;
                if *count == 0 {
                    self.metric_counts.remove(&entry.metric);
                }
            }
            removed += 1;
        }
        removed
    }

    fn reset_rate_window_if_due(&mut self, now: Instant) {
        if now.saturating_duration_since(self.rate_window_started) >= RATE_WINDOW {
            self.rate_window_started = now;
            self.new_in_window = 0;
        }
    }
}

/// Process-local registry shared by every remote-write handler on an ingester.
pub struct PrometheusSeriesAdmission {
    settings: PrometheusCardinalitySettings,
    organizations: DashMap<Id, Arc<Mutex<OrgSeriesState>>>,
    active_process: AtomicUsize,
}

impl PrometheusSeriesAdmission {
    pub fn new(settings: PrometheusCardinalitySettings) -> Self {
        Self {
            settings,
            organizations: DashMap::new(),
            active_process: AtomicUsize::new(0),
        }
    }

    pub fn admit(
        &self,
        org_id: &Id,
        identities: impl IntoIterator<Item = SeriesIdentity>,
        now: Instant,
    ) -> Result<SeriesAdmissionOutcome, SeriesLimitReason> {
        if !self.settings.enabled {
            return Ok(SeriesAdmissionOutcome::default());
        }
        let unique = identities
            .into_iter()
            .map(|identity| (identity.series, identity.metric))
            .collect::<HashMap<_, _>>();
        if unique.is_empty() {
            return Ok(SeriesAdmissionOutcome::default());
        }

        let org = self
            .organizations
            .entry(org_id.clone())
            .or_insert_with(|| Arc::new(Mutex::new(OrgSeriesState::new(now))))
            .clone();
        let mut state = org.lock();
        let removed = state.expire_idle(now, Duration::from_secs(self.settings.idle_ttl_secs));
        if removed > 0 {
            self.active_process.fetch_sub(removed, Ordering::AcqRel);
            add_active_series(-count_as_i64(removed));
        }
        state.reset_rate_window_if_due(now);

        let mut new_by_metric: HashMap<Fingerprint, usize> = HashMap::new();
        let mut new_identities = Vec::new();
        let mut existing_series = 0usize;
        for (series, metric) in unique {
            if let Some(entry) = state.active.get_mut(&series) {
                entry.last_seen = now;
                existing_series += 1;
            } else {
                *new_by_metric.entry(metric).or_default() += 1;
                new_identities.push((series, metric));
            }
        }
        let new_series = new_identities.len();
        if new_series == 0 {
            return Ok(SeriesAdmissionOutcome {
                existing_series,
                new_series: 0,
            });
        }

        if state.active.len().saturating_add(new_series) > self.settings.max_active_series_per_org {
            return Err(self.reject(SeriesLimitReason::OrganizationActive));
        }
        for (metric, count) in &new_by_metric {
            let active = state.metric_counts.get(metric).copied().unwrap_or(0);
            if active.saturating_add(*count) > self.settings.max_active_series_per_metric {
                return Err(self.reject(SeriesLimitReason::MetricActive));
            }
        }
        if state.new_in_window.saturating_add(new_series) > self.settings.max_new_series_per_minute
        {
            return Err(self.reject(SeriesLimitReason::NewSeriesRate));
        }
        if !self.try_reserve_process(new_series) {
            return Err(self.reject(SeriesLimitReason::ProcessActive));
        }

        let idle_ttl = Duration::from_secs(self.settings.idle_ttl_secs);
        let expires_at = now
            .checked_add(idle_ttl)
            .expect("validated series idle TTL must fit Instant");
        for (series, metric) in new_identities {
            state.active.insert(
                series,
                SeriesEntry {
                    metric,
                    last_seen: now,
                },
            );
            *state.metric_counts.entry(metric).or_default() += 1;
            state.expiry.push(Reverse((expires_at, series)));
        }
        state.new_in_window += new_series;
        add_active_series(count_as_i64(new_series));
        Ok(SeriesAdmissionOutcome {
            existing_series,
            new_series,
        })
    }

    pub fn active_series(&self) -> usize {
        self.active_process.load(Ordering::Acquire)
    }

    fn try_reserve_process(&self, count: usize) -> bool {
        self.active_process
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current
                    .checked_add(count)
                    .filter(|next| *next <= self.settings.max_active_series_per_process)
            })
            .is_ok()
    }

    fn reject(&self, reason: SeriesLimitReason) -> SeriesLimitReason {
        inc_series_rejection(reason.as_str());
        reason
    }
}

impl Drop for PrometheusSeriesAdmission {
    fn drop(&mut self) {
        let active = self.active_process.load(Ordering::Acquire);
        if active > 0 {
            add_active_series(-count_as_i64(active));
        }
    }
}

fn count_as_i64(count: usize) -> i64 {
    i64::try_from(count).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(
        process: usize,
        org: usize,
        metric: usize,
        new_per_minute: usize,
        ttl_secs: u64,
    ) -> PrometheusCardinalitySettings {
        PrometheusCardinalitySettings {
            enabled: true,
            max_active_series_per_process: process,
            max_active_series_per_org: org,
            max_active_series_per_metric: metric,
            max_new_series_per_minute: new_per_minute,
            idle_ttl_secs: ttl_secs,
        }
    }

    fn identity(metric: &str, value: &str) -> SeriesIdentity {
        SeriesIdentity::from_labels(metric, [("instance", value)])
    }

    #[test]
    fn fingerprint_is_order_independent_and_value_sensitive() {
        let a = SeriesIdentity::from_labels("up", [("job", "api"), ("instance", "a")]);
        let b = SeriesIdentity::from_labels("up", [("instance", "a"), ("job", "api")]);
        let c = SeriesIdentity::from_labels("up", [("instance", "b"), ("job", "api")]);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(!format!("{a:?}").contains("instance"));
    }

    #[test]
    fn existing_series_bypasses_exhausted_new_series_rate() {
        let registry = PrometheusSeriesAdmission::new(settings(10, 10, 10, 1, 60));
        let org = Id::from_string("org-a");
        let now = Instant::now();
        registry.admit(&org, [identity("up", "a")], now).unwrap();
        let existing = registry
            .admit(&org, [identity("up", "a")], now + Duration::from_secs(1))
            .unwrap();
        assert_eq!(existing.existing_series, 1);
        assert_eq!(
            registry.admit(&org, [identity("up", "b")], now),
            Err(SeriesLimitReason::NewSeriesRate)
        );
    }

    #[test]
    fn metric_org_and_process_caps_are_distinct() {
        let now = Instant::now();
        let org = Id::from_string("org-a");
        let metric_registry = PrometheusSeriesAdmission::new(settings(10, 10, 1, 10, 60));
        metric_registry
            .admit(&org, [identity("up", "a")], now)
            .unwrap();
        assert_eq!(
            metric_registry.admit(&org, [identity("up", "b")], now),
            Err(SeriesLimitReason::MetricActive)
        );

        let org_registry = PrometheusSeriesAdmission::new(settings(10, 1, 1, 10, 60));
        org_registry
            .admit(&org, [identity("up", "a")], now)
            .unwrap();
        assert_eq!(
            org_registry.admit(&org, [identity("other", "a")], now),
            Err(SeriesLimitReason::OrganizationActive)
        );

        let process_registry = PrometheusSeriesAdmission::new(settings(1, 1, 1, 10, 60));
        process_registry
            .admit(&org, [identity("up", "a")], now)
            .unwrap();
        assert_eq!(
            process_registry.admit(&Id::from_string("org-b"), [identity("up", "b")], now),
            Err(SeriesLimitReason::ProcessActive)
        );
    }

    #[test]
    fn idle_expiry_releases_all_capacity_levels() {
        let registry = PrometheusSeriesAdmission::new(settings(1, 1, 1, 10, 10));
        let org = Id::from_string("org-a");
        let now = Instant::now();
        registry.admit(&org, [identity("up", "a")], now).unwrap();
        assert_eq!(registry.active_series(), 1);
        registry
            .admit(
                &org,
                [identity("other", "b")],
                now + Duration::from_secs(11),
            )
            .unwrap();
        assert_eq!(registry.active_series(), 1);
    }
}
