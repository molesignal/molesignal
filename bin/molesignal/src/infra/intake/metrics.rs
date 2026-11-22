// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Intake role 自身的 metric（与 caching 共享同一全局 registry）。
//!
//! - `intake_flush_errors_total{step}`: step ∈ {parquet_write, file_catalog_commit, wal_cleanup};
//!   data that can never be published and is dropped from memory counts once per drop under
//!   {stream_gone, dataset_gone, writer_fenced}.
//! - `intake_rotations_total{stream_type,reason}`：reason ∈ {size, age, retry, forced}。
//! - `intake_flush_inflight{stream_type}`：正在执行完整 flush transaction 的并发数。
//!
//! The WAL's own metrics are in `object_wal::metrics`.

use std::sync::OnceLock;

use prometheus::{HistogramVec, IntCounterVec, IntGauge, IntGaugeVec};
use signals::metrics::{register_histogram_vec, register_int_counter_vec, register_int_gauge_vec};

use super::rotation::RotationReason;

static FLUSH_ERRORS: OnceLock<IntCounterVec> = OnceLock::new();
static ROTATIONS: OnceLock<IntCounterVec> = OnceLock::new();
static FLUSH_INFLIGHT: OnceLock<IntGaugeVec> = OnceLock::new();
static SERIES_REJECTIONS: OnceLock<IntCounterVec> = OnceLock::new();
static ACTIVE_SERIES: OnceLock<IntGauge> = OnceLock::new();
static MEMORY_REJECTIONS: OnceLock<IntCounterVec> = OnceLock::new();
static RESERVED_BUFFER_BYTES: OnceLock<IntGauge> = OnceLock::new();
static PARQUET_RATIO: OnceLock<HistogramVec> = OnceLock::new();
static ADAPTIVE_TARGET: OnceLock<HistogramVec> = OnceLock::new();

fn flush_errors_vec() -> &'static IntCounterVec {
    FLUSH_ERRORS.get_or_init(|| {
        register_int_counter_vec(
            "intake_flush_errors_total",
            "intake flush errors by step",
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
            "intake_rotations_total",
            "Parquet rotations by stream type and bounded trigger reason",
            &["stream_type", "reason"],
        )
    })
}

pub fn inc_rotation(stream_type: &str, reason: RotationReason) {
    rotations_vec()
        .with_label_values(&[stream_type, reason.as_str()])
        .inc();
}

fn flush_inflight_vec() -> &'static IntGaugeVec {
    FLUSH_INFLIGHT.get_or_init(|| {
        register_int_gauge_vec(
            "intake_flush_inflight",
            "Concurrent full flush transactions by stream type",
            &["stream_type"],
        )
    })
}

pub struct FlushInflightGuard {
    stream_type: String,
}

impl FlushInflightGuard {
    pub fn enter(stream_type: &str) -> Self {
        flush_inflight_vec().with_label_values(&[stream_type]).inc();

        Self {
            stream_type: stream_type.to_owned(),
        }
    }
}

impl Drop for FlushInflightGuard {
    fn drop(&mut self) {
        flush_inflight_vec()
            .with_label_values(&[&self.stream_type])
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
        signals::metrics::register_int_gauge(
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
            "intake_memory_rejections_total",
            "Intake batches rejected before WAL append by bounded stream type",
            &["stream_type"],
        )
    })
}

pub(super) fn inc_memory_rejection(stream_type: &str) {
    memory_rejections_vec()
        .with_label_values(&[stream_type])
        .inc();
}

fn reserved_buffer_bytes_gauge() -> &'static IntGauge {
    RESERVED_BUFFER_BYTES.get_or_init(|| {
        signals::metrics::register_int_gauge(
            "intake_buffer_reserved_bytes",
            "Raw payload bytes reserved until a full intake flush transaction succeeds",
        )
    })
}

pub(super) fn add_reserved_buffer_bytes(delta: i64) {
    reserved_buffer_bytes_gauge().add(delta);
}

fn parquet_ratio_vec() -> &'static HistogramVec {
    PARQUET_RATIO.get_or_init(|| {
        register_histogram_vec(
            "intake_parquet_encoded_raw_ratio",
            "Observed encoded Parquet bytes divided by estimated raw generation bytes",
            &["dataset_type"],
            vec![0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0],
        )
    })
}

pub(super) fn observe_parquet_ratio(dataset_type: &str, ratio: f64) {
    parquet_ratio_vec()
        .with_label_values(&[dataset_type])
        .observe(ratio);
}

fn adaptive_target_vec() -> &'static HistogramVec {
    ADAPTIVE_TARGET.get_or_init(|| {
        register_histogram_vec(
            "intake_adaptive_rotation_target_bytes",
            "Adaptive raw buffer rotation thresholds selected after successful files",
            &["dataset_type"],
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

pub(super) fn observe_adaptive_target(dataset_type: &str, bytes: f64) {
    adaptive_target_vec()
        .with_label_values(&[dataset_type])
        .observe(bytes);
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
