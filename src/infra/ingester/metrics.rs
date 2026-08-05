// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Ingester role 自身的 metric（与 caching 共享同一全局 registry）。
//!
//! - `ingester_flush_errors_total{step}`：step ∈ {parquet_write, parquet_file_meta_insert, wal_truncate}。
//! - `ingester_rotations_total{stream_type,reason}`：reason ∈ {size, age, retry, forced}。
//! - `ingester_flush_inflight{stream_type}`：正在执行完整 flush transaction 的并发数。
//! - `wal_append_lock_wait_seconds{stream_type}`：WAL per-key 互斥锁等待延迟 Histogram。
//! - `wal_append_inflight{stream_type}`：当前持有 WAL 临界区的并发数 IntGauge（`WalInflightGuard` RAII 维护）。

use std::sync::OnceLock;

use prometheus::{HistogramVec, IntCounterVec, IntGauge, IntGaugeVec};

use super::rotation::RotationReason;
use crate::shared::metrics::{
    register_histogram_vec, register_int_counter_vec, register_int_gauge_vec,
};

static FLUSH_ERRORS: OnceLock<IntCounterVec> = OnceLock::new();
static ROTATIONS: OnceLock<IntCounterVec> = OnceLock::new();
static FLUSH_INFLIGHT: OnceLock<IntGaugeVec> = OnceLock::new();
static SERIES_REJECTIONS: OnceLock<IntCounterVec> = OnceLock::new();
static ACTIVE_SERIES: OnceLock<IntGauge> = OnceLock::new();
static MEMORY_REJECTIONS: OnceLock<IntCounterVec> = OnceLock::new();
static RESERVED_BUFFER_BYTES: OnceLock<IntGauge> = OnceLock::new();
static PARQUET_RATIO: OnceLock<HistogramVec> = OnceLock::new();
static ADAPTIVE_TARGET: OnceLock<HistogramVec> = OnceLock::new();
static WAL_LOCK_WAIT: OnceLock<HistogramVec> = OnceLock::new();
static WAL_INFLIGHT: OnceLock<IntGaugeVec> = OnceLock::new();

fn flush_errors_vec() -> &'static IntCounterVec {
    FLUSH_ERRORS.get_or_init(|| {
        register_int_counter_vec(
            "ingester_flush_errors_total",
            "ingester flush errors by step",
            &["step"],
        )
    })
}

pub fn inc_flush_error(step: &str) {
    flush_errors_vec().with_label_values(&[step]).inc();
}

fn rotations_vec() -> &'static IntCounterVec {
    ROTATIONS.get_or_init(|| {
        register_int_counter_vec(
            "ingester_rotations_total",
            "Parquet rotations by stream type and bounded trigger reason",
            &["stream_type", "reason"],
        )
    })
}

pub fn inc_rotation(stream_type: &'static str, reason: RotationReason) {
    rotations_vec()
        .with_label_values(&[stream_type, reason.as_str()])
        .inc();
}

fn flush_inflight_vec() -> &'static IntGaugeVec {
    FLUSH_INFLIGHT.get_or_init(|| {
        register_int_gauge_vec(
            "ingester_flush_inflight",
            "Concurrent full flush transactions by stream type",
            &["stream_type"],
        )
    })
}

pub struct FlushInflightGuard {
    stream_type: &'static str,
}

impl FlushInflightGuard {
    pub fn enter(stream_type: &'static str) -> Self {
        flush_inflight_vec().with_label_values(&[stream_type]).inc();
        Self { stream_type }
    }
}

impl Drop for FlushInflightGuard {
    fn drop(&mut self) {
        flush_inflight_vec()
            .with_label_values(&[self.stream_type])
            .dec();
    }
}

fn series_rejections_vec() -> &'static IntCounterVec {
    SERIES_REJECTIONS.get_or_init(|| {
        register_int_counter_vec(
            "prometheus_series_admission_rejections_total",
            "Prometheus remote-write requests rejected by bounded series reason",
            &["reason"],
        )
    })
}

pub(super) fn inc_series_rejection(reason: &'static str) {
    series_rejections_vec().with_label_values(&[reason]).inc();
}

fn active_series_gauge() -> &'static IntGauge {
    ACTIVE_SERIES.get_or_init(|| {
        crate::shared::metrics::register_int_gauge(
            "prometheus_active_series",
            "Active hashed Prometheus series tracked by this process",
        )
    })
}

pub(super) fn add_active_series(delta: i64) {
    active_series_gauge().add(delta);
}

fn memory_rejections_vec() -> &'static IntCounterVec {
    MEMORY_REJECTIONS.get_or_init(|| {
        register_int_counter_vec(
            "ingester_memory_rejections_total",
            "Ingest batches rejected before WAL append by bounded stream type",
            &["stream_type"],
        )
    })
}

pub(super) fn inc_memory_rejection(stream_type: &'static str) {
    memory_rejections_vec()
        .with_label_values(&[stream_type])
        .inc();
}

fn reserved_buffer_bytes_gauge() -> &'static IntGauge {
    RESERVED_BUFFER_BYTES.get_or_init(|| {
        crate::shared::metrics::register_int_gauge(
            "ingester_buffer_reserved_bytes",
            "Raw payload bytes reserved until a full ingester flush transaction succeeds",
        )
    })
}

pub(super) fn add_reserved_buffer_bytes(delta: i64) {
    reserved_buffer_bytes_gauge().add(delta);
}

fn parquet_ratio_vec() -> &'static HistogramVec {
    PARQUET_RATIO.get_or_init(|| {
        register_histogram_vec(
            "ingester_parquet_encoded_raw_ratio",
            "Observed encoded Parquet bytes divided by estimated raw generation bytes",
            &["stream_type"],
            vec![0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0],
        )
    })
}

pub(super) fn observe_parquet_ratio(stream_type: &'static str, ratio: f64) {
    parquet_ratio_vec()
        .with_label_values(&[stream_type])
        .observe(ratio);
}

fn adaptive_target_vec() -> &'static HistogramVec {
    ADAPTIVE_TARGET.get_or_init(|| {
        register_histogram_vec(
            "ingester_adaptive_rotation_target_bytes",
            "Adaptive raw buffer rotation thresholds selected after successful files",
            &["stream_type"],
            vec![
                1024.0 * 1024.0,
                4.0 * 1024.0 * 1024.0,
                16.0 * 1024.0 * 1024.0,
                64.0 * 1024.0 * 1024.0,
                128.0 * 1024.0 * 1024.0,
                256.0 * 1024.0 * 1024.0,
                512.0 * 1024.0 * 1024.0,
                1024.0 * 1024.0 * 1024.0,
            ],
        )
    })
}

pub(super) fn observe_adaptive_target(stream_type: &'static str, bytes: f64) {
    adaptive_target_vec()
        .with_label_values(&[stream_type])
        .observe(bytes);
}

fn wal_lock_wait_vec() -> &'static HistogramVec {
    WAL_LOCK_WAIT.get_or_init(|| {
        register_histogram_vec(
            "wal_append_lock_wait_seconds",
            "WAL per-key mutex wait time observed at WalPool::append",
            &["stream_type"],
            vec![0.0001, 0.001, 0.01, 0.1, 1.0],
        )
    })
}

fn wal_inflight_vec() -> &'static IntGaugeVec {
    WAL_INFLIGHT.get_or_init(|| {
        register_int_gauge_vec(
            "wal_append_inflight",
            "Concurrent WalPool::append entries holding the per-key mutex",
            &["stream_type"],
        )
    })
}

/// 记录 `WalPool::append` 在 per-key mutex 上的等待时长。
pub fn observe_wal_lock_wait(stream_type: &str, secs: f64) {
    wal_lock_wait_vec()
        .with_label_values(&[stream_type])
        .observe(secs);
}

/// RAII：进入 `WalPool::append` 临界区时 +1，drop 时 -1。
///
/// label cardinality 锁死在 `stream_type` 固定枚举，不带 `org_id` / `stream_name`，
/// 避免 high cardinality。
pub struct WalInflightGuard {
    stream_type: &'static str,
}

impl WalInflightGuard {
    pub fn enter(stream_type: &'static str) -> Self {
        wal_inflight_vec().with_label_values(&[stream_type]).inc();
        Self { stream_type }
    }
}

impl Drop for WalInflightGuard {
    fn drop(&mut self) {
        wal_inflight_vec()
            .with_label_values(&[self.stream_type])
            .dec();
    }
}

#[cfg(test)]
pub(crate) fn wal_inflight_count(stream_type: &str) -> i64 {
    wal_inflight_vec().with_label_values(&[stream_type]).get()
}

#[cfg(test)]
pub(crate) fn wal_lock_wait_sample_count(stream_type: &str) -> u64 {
    wal_lock_wait_vec()
        .with_label_values(&[stream_type])
        .get_sample_count()
}

#[cfg(test)]
pub(crate) fn flush_inflight_count(stream_type: &str) -> i64 {
    flush_inflight_vec().with_label_values(&[stream_type]).get()
}

#[cfg(test)]
pub(crate) fn rotation_count(stream_type: &str, reason: RotationReason) -> u64 {
    rotations_vec()
        .with_label_values(&[stream_type, reason.as_str()])
        .get()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flush_metrics_use_bounded_labels_and_raii() {
        let before = rotation_count("extend", RotationReason::Size);
        inc_rotation("extend", RotationReason::Size);
        assert_eq!(rotation_count("extend", RotationReason::Size), before + 1);

        let before = flush_inflight_count("extend");
        {
            let _guard = FlushInflightGuard::enter("extend");
            assert_eq!(flush_inflight_count("extend"), before + 1);
        }
        assert_eq!(flush_inflight_count("extend"), before);
    }
}
