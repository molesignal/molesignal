// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 查询准入的指标：每个工作组在本节点的执行数、排队数、等待时长与拒绝次数。

use std::sync::OnceLock;

use prometheus::{HistogramVec, IntCounterVec, IntGaugeVec};
use signals::metrics::{register_histogram_vec, register_int_counter_vec, register_int_gauge_vec};

pub(super) struct AdmissionMetrics {
    pub(super) running: IntGaugeVec,
    pub(super) waiting: IntGaugeVec,
    pub(super) wait_seconds: HistogramVec,
    pub(super) refusals: IntCounterVec,
}

pub(super) fn metrics() -> &'static AdmissionMetrics {
    static METRICS: OnceLock<AdmissionMetrics> = OnceLock::new();

    METRICS.get_or_init(|| AdmissionMetrics {
        running: register_int_gauge_vec(
            "query_admission_running",
            "Queries holding an admission slot on this node, by work group.",
            &["group"],
        ),
        waiting: register_int_gauge_vec(
            "query_admission_waiting",
            "Queries waiting for an admission slot on this node, by work group.",
            &["group"],
        ),
        wait_seconds: register_histogram_vec(
            "query_admission_wait_seconds",
            "Time a query waited for an admission slot, by work group and outcome.",
            &["group", "outcome"],
            vec![
                0.001, 0.01, 0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0, 120.0,
            ],
        ),
        refusals: register_int_counter_vec(
            "query_admission_refusals_total",
            "Queries refused an admission slot, by work group and reason.",
            &["group", "reason"],
        ),
    })
}
