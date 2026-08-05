// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Bounded-cardinality metrics for the distributed Trace pipeline.
//!
//! Every label is normalized to a closed catalog in this module. Callers must pass only static
//! operation/result strings; tenant IDs, Trace IDs, routes, hosts, and object keys are never labels.

use std::{sync::OnceLock, time::Duration};

use prometheus::{HistogramVec, IntCounterVec, IntGaugeVec};

use crate::shared::metrics::{
    register_histogram_vec, register_int_counter_vec, register_int_gauge_vec,
};

struct TraceMetrics {
    spans: IntCounterVec,
    decisions: IntCounterVec,
    retries: IntCounterVec,
    batches: IntCounterVec,
    queue_depth: IntGaugeVec,
    queue_capacity: IntGaugeVec,
    tail_cache: IntGaugeVec,
    system_load: IntGaugeVec,
    latency: HistogramVec,
}

fn metrics() -> &'static TraceMetrics {
    static METRICS: OnceLock<TraceMetrics> = OnceLock::new();
    METRICS.get_or_init(|| TraceMetrics {
        spans: register_int_counter_vec(
            "molesignal_trace_spans_total",
            "Trace spans observed at bounded pipeline stages.",
            &["stage", "result"],
        ),
        decisions: register_int_counter_vec(
            "molesignal_trace_decisions_total",
            "Tail-sampling decisions by bounded decision and reason.",
            &["decision", "reason"],
        ),
        retries: register_int_counter_vec(
            "molesignal_trace_retries_total",
            "Trace sink retries by bounded sink and reason.",
            &["sink", "reason"],
        ),
        batches: register_int_counter_vec(
            "molesignal_trace_export_batches_total",
            "Trace export batches by bounded sink and result.",
            &["sink", "result"],
        ),
        queue_depth: register_int_gauge_vec(
            "molesignal_trace_queue_depth",
            "Current Trace queue depth.",
            &["queue"],
        ),
        queue_capacity: register_int_gauge_vec(
            "molesignal_trace_queue_capacity",
            "Configured Trace queue capacity.",
            &["queue"],
        ),
        tail_cache: register_int_gauge_vec(
            "molesignal_trace_tail_cache",
            "Current Trace tail-cache usage and capacity.",
            &["resource"],
        ),
        system_load: register_int_gauge_vec(
            "molesignal_trace_system_load_status",
            "Whether required system Trace state loaded successfully (1 healthy, 0 degraded).",
            &["component"],
        ),
        latency: register_histogram_vec(
            "molesignal_trace_latency_seconds",
            "Trace decision/export latency.",
            &["stage", "sink"],
            vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0, 30.0],
        ),
    })
}

fn bounded_stage(value: &'static str) -> &'static str {
    match value {
        "generated" | "candidate" | "routing" | "sampler" | "self_ingest" | "external" => value,
        _ => "unknown",
    }
}

fn bounded_result(value: &'static str) -> &'static str {
    match value {
        "accepted" | "kept" | "dropped" | "duplicate" | "conflict" | "late" | "partial"
        | "exported" | "failed" | "queue_full" | "stopped" | "invalid" => value,
        _ => "unknown",
    }
}

fn bounded_sink(value: &str) -> &'static str {
    match value {
        "self_ingest" | "self-ingest" => "self_ingest",
        "external" | "external_otlp" | "otlp" => "external",
        "routing" => "routing",
        _ => "unknown",
    }
}

fn bounded_queue(value: &'static str) -> &'static str {
    match value {
        "routing" | "candidate" | "self_ingest" | "external" => value,
        _ => "unknown",
    }
}

fn bounded_retry_reason(value: &'static str) -> &'static str {
    match value {
        "export_failed" | "timeout" | "delivery_failed" => value,
        _ => "unknown",
    }
}

pub fn record_spans(stage: &'static str, result: &'static str, count: u64) {
    metrics()
        .spans
        .with_label_values(&[bounded_stage(stage), bounded_result(result)])
        .inc_by(count);
}

pub fn record_decision(kept: bool, reason: &'static str, count: u64, latency: Duration) {
    metrics()
        .decisions
        .with_label_values(&[if kept { "keep" } else { "drop" }, reason])
        .inc();
    metrics()
        .latency
        .with_label_values(&["decision", "sampler"])
        .observe(latency.as_secs_f64());
    record_spans("sampler", if kept { "kept" } else { "dropped" }, count);
}

pub fn record_retry(sink: &str, reason: &'static str) {
    metrics()
        .retries
        .with_label_values(&[bounded_sink(sink), bounded_retry_reason(reason)])
        .inc();
}

pub fn record_export(sink: &str, result: &'static str, span_count: u64, latency: Duration) {
    let sink = bounded_sink(sink);
    let result = bounded_result(result);
    metrics().batches.with_label_values(&[sink, result]).inc();
    metrics()
        .latency
        .with_label_values(&["export", sink])
        .observe(latency.as_secs_f64());
    record_spans(
        if sink == "self_ingest" {
            "self_ingest"
        } else if sink == "routing" {
            "routing"
        } else {
            "external"
        },
        result,
        span_count,
    );
}

pub fn set_queue(queue: &'static str, depth: usize, capacity: usize) {
    let queue = bounded_queue(queue);
    metrics()
        .queue_depth
        .with_label_values(&[queue])
        .set(saturating_i64(depth));
    metrics()
        .queue_capacity
        .with_label_values(&[queue])
        .set(saturating_i64(capacity));
}

pub fn set_tail_cache(traces: usize, bytes: usize, capacity_traces: usize, capacity_bytes: usize) {
    for (resource, value) in [
        ("traces", traces),
        ("bytes", bytes),
        ("capacity_traces", capacity_traces),
        ("capacity_bytes", capacity_bytes),
    ] {
        metrics()
            .tail_cache
            .with_label_values(&[resource])
            .set(saturating_i64(value));
    }
}

pub fn set_system_load(component: &'static str, healthy: bool) {
    let component = match component {
        "system_org" | "license" | "trace_policy" => component,
        _ => "unknown",
    };
    metrics()
        .system_load
        .with_label_values(&[component])
        .set(i64::from(healthy));
}

fn saturating_i64(value: usize) -> i64 {
    value.min(i64::MAX as usize) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_closed_and_never_retain_raw_identifiers() {
        assert_eq!(bounded_sink("https://collector/acme"), "unknown");
        assert_eq!(bounded_queue("org-123"), "unknown");
        assert_eq!(bounded_stage("/users/alice"), "unknown");
        assert_eq!(bounded_result("trace-id"), "unknown");
        assert_eq!(bounded_retry_reason("alice@example.com"), "unknown");
    }
}
