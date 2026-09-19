// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::OnceLock;

use prometheus::{Histogram, IntCounter, IntGauge};

use crate::shared::metrics::{register_histogram, register_int_counter, register_int_gauge};

struct ObjectReadMetrics {
    requests: IntCounter,
    bytes: IntCounter,
    duration: Histogram,
    cache_hit_bytes: IntCounter,
    cache_miss_bytes: IntCounter,
    corrupt_blocks: IntCounter,
    evictions: IntCounter,
    singleflight_waiters: IntGauge,
    disk_bytes: IntGauge,
}

fn metrics() -> &'static ObjectReadMetrics {
    static METRICS: OnceLock<ObjectReadMetrics> = OnceLock::new();
    METRICS.get_or_init(|| ObjectReadMetrics {
        requests: register_int_counter(
            "object_read_requests_total",
            "Catalog-backed object read requests",
        ),
        bytes: register_int_counter(
            "object_read_bytes_total",
            "Bytes returned by Catalog-backed object reads",
        ),
        duration: register_histogram(
            "object_read_duration_seconds",
            "Catalog-backed object read latency",
            vec![0.0005, 0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 15.0],
        ),
        cache_hit_bytes: register_int_counter(
            "object_cache_hit_bytes_total",
            "Bytes served from local object blocks",
        ),
        cache_miss_bytes: register_int_counter(
            "object_cache_miss_bytes_total",
            "Bytes fetched from the origin for local object blocks",
        ),
        corrupt_blocks: register_int_counter(
            "object_cache_corrupt_blocks_total",
            "Corrupt local object blocks removed before refetch",
        ),
        evictions: register_int_counter(
            "object_cache_evictions_total",
            "Local object blocks evicted for capacity or free-space pressure",
        ),
        singleflight_waiters: register_int_gauge(
            "object_cache_singleflight_waiters",
            "Readers currently waiting for an in-flight block fetch",
        ),
        disk_bytes: register_int_gauge(
            "object_cache_disk_bytes",
            "Bytes occupied by published local object block files",
        ),
    })
}

pub(super) struct ReadTimer(std::time::Instant);

impl ReadTimer {
    pub(super) fn start() -> Self {
        metrics().requests.inc();
        Self(std::time::Instant::now())
    }

    pub(super) fn finish(self, bytes: usize) {
        metrics().bytes.inc_by(bytes as u64);
        metrics().duration.observe(self.0.elapsed().as_secs_f64());
    }
}

pub(super) fn record_hit(bytes: usize) {
    metrics().cache_hit_bytes.inc_by(bytes as u64);
}

pub(super) fn record_miss(bytes: usize) {
    metrics().cache_miss_bytes.inc_by(bytes as u64);
}

pub(super) fn record_corrupt() {
    metrics().corrupt_blocks.inc();
}

pub(super) fn record_eviction() {
    metrics().evictions.inc();
}

pub(super) fn set_disk_bytes(bytes: u64) {
    metrics()
        .disk_bytes
        .set(bytes.try_into().unwrap_or(i64::MAX));
}

pub(super) struct SingleflightWaiter;

impl SingleflightWaiter {
    pub(super) fn enter() -> Self {
        metrics().singleflight_waiters.inc();
        Self
    }
}

impl Drop for SingleflightWaiter {
    fn drop(&mut self) {
        metrics().singleflight_waiters.dec();
    }
}
