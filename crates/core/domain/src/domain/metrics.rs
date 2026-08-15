// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Metrics 领域模型与 Prometheus Exemplar 的内部存储契约。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::shared::time::TimestampMicros;

/// Container-style metric streams (OTLP and self telemetry) persist the
/// logical Prometheus metric identity alongside each sample.
pub const METRIC_NAME_FIELD: &str = "metric_name";
/// Persisted metric metadata used by catalogs; it is not a Prometheus label.
pub const METRIC_KIND_FIELD: &str = "metric_kind";
/// Human-readable OTLP metric description.
pub const METRIC_DESCRIPTION_FIELD: &str = "metric_description";
/// OTLP metric unit.
pub const METRIC_UNIT_FIELD: &str = "metric_unit";
/// OTLP aggregation temporality normalized to a stable string.
pub const METRIC_TEMPORALITY_FIELD: &str = "metric_temporality";
/// Whether an OTLP Sum is monotonic.
pub const METRIC_MONOTONIC_FIELD: &str = "metric_monotonic";
/// Aggregation interval start in Unix nanoseconds.
pub const METRIC_START_TIME_UNIX_NANO_FIELD: &str = "metric_start_time_unix_nano";

/// Returns whether a field is persisted metric metadata rather than a
/// user-supplied Prometheus label.
pub fn is_metric_identity_storage_field(name: &str) -> bool {
    matches!(
        name,
        METRIC_NAME_FIELD
            | METRIC_KIND_FIELD
            | METRIC_DESCRIPTION_FIELD
            | METRIC_UNIT_FIELD
            | METRIC_TEMPORALITY_FIELD
            | METRIC_MONOTONIC_FIELD
            | METRIC_START_TIME_UNIX_NANO_FIELD
    )
}

/// Per-sample storage metadata required to evaluate OTLP aggregation semantics.
pub fn is_metric_query_semantics_storage_field(name: &str) -> bool {
    matches!(
        name,
        METRIC_TEMPORALITY_FIELD | METRIC_MONOTONIC_FIELD | METRIC_START_TIME_UNIX_NANO_FIELD
    )
}

/// Exemplar 行与普通 metric sample 共用 stream，但不写 `value`。
///
/// 这些字段必须保留，避免远端 series label 覆盖内部标记后被普通 PromQL 当作样本。
pub const PROMETHEUS_EXEMPLAR_MARKER_FIELD: &str = "__molesignal_exemplar";
pub const PROMETHEUS_EXEMPLAR_VALUE_FIELD: &str = "__molesignal_exemplar_value";
pub const PROMETHEUS_EXEMPLAR_LABELS_FIELD: &str = "__molesignal_exemplar_labels";

pub fn is_prometheus_exemplar_storage_field(name: &str) -> bool {
    matches!(
        name,
        PROMETHEUS_EXEMPLAR_MARKER_FIELD
            | PROMETHEUS_EXEMPLAR_VALUE_FIELD
            | PROMETHEUS_EXEMPLAR_LABELS_FIELD
    )
}

pub type MetricLabelSet = BTreeMap<String, String>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrometheusExemplar {
    pub labels: MetricLabelSet,
    pub value: f64,
    pub timestamp: TimestampMicros,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrometheusExemplarSeries {
    pub series_labels: MetricLabelSet,
    pub exemplars: Vec<PrometheusExemplar>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PrometheusExemplarQueryResult {
    pub series: Vec<PrometheusExemplarSeries>,
    /// 达到查询侧硬上限时为 true；Prometheus HTTP adapter 会返回 warning。
    pub truncated: bool,
}
