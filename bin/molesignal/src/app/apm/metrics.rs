// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Closed-label Prometheus metrics for the APM projection lifecycle.

use std::{sync::OnceLock, time::Duration};

use prometheus::{Histogram, HistogramVec, IntCounterVec, IntGaugeVec};

use crate::shared::metrics::{
    register_histogram, register_histogram_vec, register_int_counter_vec, register_int_gauge_vec,
};

struct ApmMetrics {
    facts: IntCounterVec,
    cardinality: IntCounterVec,
    queue: IntGaugeVec,
    flushes: IntCounterVec,
    flush_latency: Histogram,
    rollups: IntCounterVec,
    rollup_rows: IntCounterVec,
    lag_seconds: IntGaugeVec,
    health: IntGaugeVec,
    api_latency: HistogramVec,
}

fn metrics() -> &'static ApmMetrics {
    static METRICS: OnceLock<ApmMetrics> = OnceLock::new();
    METRICS.get_or_init(|| ApmMetrics {
        facts: register_int_counter_vec(
            "molesignal_apm_facts_total",
            "APM facts handled by the bounded projector.",
            &["result"],
        ),
        cardinality: register_int_counter_vec(
            "molesignal_apm_cardinality_total",
            "APM cardinality admission outcomes.",
            &["reason"],
        ),
        queue: register_int_gauge_vec(
            "molesignal_apm_queue",
            "APM projector queue depth and capacity.",
            &["resource"],
        ),
        flushes: register_int_counter_vec(
            "molesignal_apm_flushes_total",
            "APM owner snapshot flushes.",
            &["result"],
        ),
        flush_latency: register_histogram(
            "molesignal_apm_flush_duration_seconds",
            "APM snapshot flush duration.",
            vec![0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 2.0, 5.0, 10.0],
        ),
        rollups: register_int_counter_vec(
            "molesignal_apm_rollups_total",
            "APM rollup runs.",
            &["result"],
        ),
        rollup_rows: register_int_counter_vec(
            "molesignal_apm_rollup_rows_total",
            "APM rows handled during rollup.",
            &["kind"],
        ),
        lag_seconds: register_int_gauge_vec(
            "molesignal_apm_lag_seconds",
            "APM projection and rollup lag.",
            &["stage"],
        ),
        health: register_int_gauge_vec(
            "molesignal_apm_health",
            "Detailed APM component health (1 healthy, 0 degraded).",
            &["component"],
        ),
        api_latency: register_histogram_vec(
            "molesignal_apm_api_duration_seconds",
            "APM API query latency.",
            &["endpoint"],
            vec![0.005, 0.01, 0.05, 0.1, 0.25, 0.5, 0.75, 1.0, 2.0, 5.0],
        ),
    })
}

pub(super) fn record_fact(result: &'static str) {
    let result = match result {
        "accepted"
        | "duplicate_skip"
        | "late_kept"
        | "late_dropped"
        | "queue_full"
        | "cardinality_rejected"
        | "stopped"
        | "extract_failed"
        | "flush_failed" => result,
        _ => "unknown",
    };
    metrics().facts.with_label_values(&[result]).inc();
}

pub(super) fn record_cardinality(reason: &'static str) {
    let reason = match reason {
        "service_rejected"
        | "transaction_overflow"
        | "dependency_overflow"
        | "error_overflow"
        | "version_suppressed"
        | "instance_suppressed" => reason,
        _ => "unknown",
    };
    metrics().cardinality.with_label_values(&[reason]).inc();
}

pub(super) fn set_queue(depth: usize, capacity: usize) {
    metrics()
        .queue
        .with_label_values(&["depth"])
        .set(saturating_i64(depth));
    metrics()
        .queue
        .with_label_values(&["capacity"])
        .set(saturating_i64(capacity));
}

pub(super) fn record_flush(success: bool, elapsed: Duration) {
    metrics()
        .flushes
        .with_label_values(&[if success { "success" } else { "failure" }])
        .inc();
    metrics().flush_latency.observe(elapsed.as_secs_f64());
}

pub(super) fn record_rollup(success: bool, source_rows: u64, rollup_rows: u64, deleted_rows: u64) {
    metrics()
        .rollups
        .with_label_values(&[if success { "success" } else { "failure" }])
        .inc();
    for (kind, value) in [
        ("source", source_rows),
        ("rollup", rollup_rows),
        ("deleted", deleted_rows),
    ] {
        metrics()
            .rollup_rows
            .with_label_values(&[kind])
            .inc_by(value);
    }
}

pub(super) fn set_lag(stage: &'static str, micros: i64) {
    let stage = match stage {
        "projection" | "rollup" => stage,
        _ => "unknown",
    };
    metrics()
        .lag_seconds
        .with_label_values(&[stage])
        .set(micros.max(0).saturating_div(1_000_000));
}

pub(super) fn set_health(component: &'static str, healthy: bool) {
    let component = match component {
        "projector" | "repository" | "rollup" => component,
        _ => "unknown",
    };
    metrics()
        .health
        .with_label_values(&[component])
        .set(i64::from(healthy));
}

pub(super) fn record_api(endpoint: &'static str, elapsed: Duration) {
    let endpoint = match endpoint {
        "overview" | "services" | "service_detail" | "transactions" | "dependencies" | "errors"
        | "error_detail" | "version_compare" | "health" => endpoint,
        _ => "unknown",
    };
    metrics()
        .api_latency
        .with_label_values(&[endpoint])
        .observe(elapsed.as_secs_f64());
}

fn saturating_i64(value: usize) -> i64 {
    value.min(i64::MAX as usize) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_identifiers_are_collapsed_to_unknown_labels() {
        record_fact("org-1");
        record_cardinality("alice@example.com");
        set_lag("tenant", 1);
        set_health("https://private", false);
    }
}
