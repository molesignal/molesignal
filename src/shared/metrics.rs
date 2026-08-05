// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 全局 prometheus::Registry 单例 + 注册 helper。
//!
//! 所有模块通过 [`global_registry`] / `register_*` 把 metric family 注册到这里；
//! HTTP `/metrics` handler 从 [`gather_text`] 取文本格式输出。重复注册同名 metric
//! 容忍 `AlreadyReg`（多次调用 init 路径不会 panic）。

use std::{collections::BTreeMap, sync::OnceLock};

use prometheus::{
    Counter, CounterVec, Histogram, HistogramOpts, HistogramVec, IntCounter, IntCounterVec,
    IntGauge, IntGaugeVec, Opts, Registry, core::Collector,
};
use tracing::{Event, Subscriber, field::Visit};
use tracing_subscriber::{Layer, layer::Context};

static REGISTRY: OnceLock<Registry> = OnceLock::new();
static DB_POOL_ACQUIRE_DURATION: OnceLock<HistogramVec> = OnceLock::new();

const META_POOL_LABEL: &str = "meta";

#[derive(Debug, Clone, PartialEq)]
pub struct MetricSample {
    pub metric_name: String,
    pub metric_kind: &'static str,
    pub value: f64,
    pub labels: BTreeMap<String, String>,
}

/// 全局 metrics registry。第一次访问时惰性初始化。
pub fn global_registry() -> &'static Registry {
    REGISTRY.get_or_init(Registry::new)
}

/// `prometheus::TextEncoder` 编码当前所有 metric 为 Prometheus 文本格式。
pub fn gather_text() -> Result<String, prometheus::Error> {
    let encoder = prometheus::TextEncoder::new();
    let mf = global_registry().gather();
    encoder.encode_to_string(&mf)
}

/// Prometheus registry 的结构化快照。self-ingest 直接消费此表示，避免 scrape/解析
/// 自己的 `/metrics` 文本端点。
#[allow(deprecated)]
pub fn gather_structured() -> Vec<MetricSample> {
    use prometheus::proto::MetricType;

    let mut out = Vec::new();
    for family in global_registry().gather() {
        let family_name = family.name();
        for metric in family.get_metric() {
            let labels = metric
                .get_label()
                .iter()
                .map(|pair| (pair.name().to_string(), pair.value().to_string()))
                .collect::<BTreeMap<_, _>>();
            let mut push = |metric_name: String,
                            metric_kind: &'static str,
                            value: f64,
                            labels: BTreeMap<String, String>| {
                out.push(MetricSample {
                    metric_name,
                    metric_kind,
                    value,
                    labels,
                });
            };

            match family.get_field_type() {
                MetricType::COUNTER => push(
                    family_name.to_string(),
                    "counter",
                    metric
                        .get_counter()
                        .as_ref()
                        .map(|counter| counter.value())
                        .unwrap_or_default(),
                    labels,
                ),
                MetricType::GAUGE => push(
                    family_name.to_string(),
                    "gauge",
                    metric
                        .get_gauge()
                        .as_ref()
                        .map(|gauge| gauge.value())
                        .unwrap_or_default(),
                    labels,
                ),
                MetricType::UNTYPED => push(
                    family_name.to_string(),
                    "untyped",
                    metric
                        .untyped
                        .as_ref()
                        .map(|untyped| untyped.value())
                        .unwrap_or_default(),
                    labels,
                ),
                MetricType::HISTOGRAM => {
                    let histogram = metric.get_histogram();
                    for bucket in histogram.get_bucket() {
                        let mut bucket_labels = labels.clone();
                        bucket_labels.insert("le".into(), format_bound(bucket.upper_bound()));
                        push(
                            format!("{family_name}_bucket"),
                            "histogram_bucket",
                            bucket.cumulative_count() as f64,
                            bucket_labels,
                        );
                    }
                    let mut inf_labels = labels.clone();
                    inf_labels.insert("le".into(), "+Inf".into());
                    push(
                        format!("{family_name}_bucket"),
                        "histogram_bucket",
                        histogram.get_sample_count() as f64,
                        inf_labels,
                    );
                    push(
                        format!("{family_name}_count"),
                        "histogram_count",
                        histogram.get_sample_count() as f64,
                        labels.clone(),
                    );
                    push(
                        format!("{family_name}_sum"),
                        "histogram_sum",
                        histogram.get_sample_sum(),
                        labels,
                    );
                }
                MetricType::SUMMARY => {
                    let summary = metric.get_summary();
                    for quantile in summary.get_quantile() {
                        let mut quantile_labels = labels.clone();
                        quantile_labels
                            .insert("quantile".into(), format_bound(quantile.quantile()));
                        push(
                            family_name.to_string(),
                            "summary_quantile",
                            quantile.value(),
                            quantile_labels,
                        );
                    }
                    push(
                        format!("{family_name}_count"),
                        "summary_count",
                        summary.sample_count() as f64,
                        labels.clone(),
                    );
                    push(
                        format!("{family_name}_sum"),
                        "summary_sum",
                        summary.sample_sum(),
                        labels,
                    );
                }
            }
        }
    }
    out
}

fn format_bound(value: f64) -> String {
    if value == f64::INFINITY {
        "+Inf".into()
    } else if value == f64::NEG_INFINITY {
        "-Inf".into()
    } else {
        value.to_string()
    }
}

fn try_register<C: Collector + Clone + 'static>(c: C) -> C {
    match global_registry().register(Box::new(c.clone())) {
        Ok(()) | Err(prometheus::Error::AlreadyReg) => c,
        Err(e) => panic!("register metric: {e}"),
    }
}

pub fn register_counter(name: &str, help: &str) -> Counter {
    let c = Counter::new(name, help).expect("create counter");
    try_register(c)
}

pub fn register_int_counter(name: &str, help: &str) -> IntCounter {
    let c = IntCounter::new(name, help).expect("create int counter");
    try_register(c)
}

pub fn register_counter_vec(name: &str, help: &str, labels: &[&str]) -> CounterVec {
    let c = CounterVec::new(Opts::new(name, help), labels).expect("create counter vec");
    try_register(c)
}

pub fn register_int_counter_vec(name: &str, help: &str, labels: &[&str]) -> IntCounterVec {
    let c = IntCounterVec::new(Opts::new(name, help), labels).expect("create int counter vec");
    try_register(c)
}

pub fn register_int_gauge(name: &str, help: &str) -> IntGauge {
    let g = IntGauge::new(name, help).expect("create int gauge");
    try_register(g)
}

pub fn register_int_gauge_vec(name: &str, help: &str, labels: &[&str]) -> IntGaugeVec {
    let g = IntGaugeVec::new(Opts::new(name, help), labels).expect("create int gauge vec");
    try_register(g)
}

pub fn register_histogram(name: &str, help: &str, buckets: Vec<f64>) -> Histogram {
    let h = Histogram::with_opts(HistogramOpts::new(name, help).buckets(buckets))
        .expect("create histogram");
    try_register(h)
}

pub fn register_histogram_vec(
    name: &str,
    help: &str,
    labels: &[&str],
    buckets: Vec<f64>,
) -> HistogramVec {
    let h = HistogramVec::new(HistogramOpts::new(name, help).buckets(buckets), labels)
        .expect("create histogram vec");
    try_register(h)
}

fn db_pool_acquire_duration() -> &'static HistogramVec {
    DB_POOL_ACQUIRE_DURATION.get_or_init(|| {
        register_histogram_vec(
            "db_pool_acquire_duration_seconds",
            "Time spent waiting to acquire a database connection",
            &["pool"],
            vec![
                0.0005, 0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0,
                10.0,
            ],
        )
    })
}

/// Captures SQLx's per-acquire timing events without exposing query text or emitting a log for
/// every successful connection checkout.
#[derive(Debug, Default, Clone, Copy)]
pub struct SqlxPoolAcquireMetricsLayer;

impl<S> Layer<S> for SqlxPoolAcquireMetricsLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if event.metadata().target() != "sqlx::pool::acquire" {
            return;
        }
        let mut visitor = AcquireDurationVisitor::default();
        event.record(&mut visitor);
        if let Some(seconds) = visitor.seconds {
            db_pool_acquire_duration()
                .with_label_values(&[META_POOL_LABEL])
                .observe(seconds);
        }
    }
}

#[derive(Default)]
struct AcquireDurationVisitor {
    seconds: Option<f64>,
}

impl Visit for AcquireDurationVisitor {
    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        // Keep the upstream SQLx field spelling for compatibility with both normal and slow
        // acquire events.
        if field.name() == "aquired_after_secs" && value.is_finite() && value >= 0.0 {
            self.seconds = Some(value);
        }
    }

    fn record_debug(&mut self, _field: &tracing::field::Field, _value: &dyn std::fmt::Debug) {}
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tracing_subscriber::prelude::*;

    use super::*;

    #[test]
    fn counter_registers_and_emits() {
        let c = register_int_counter("test_metric_a_total", "doc");
        c.inc();
        let text = gather_text().unwrap();
        assert!(text.contains("test_metric_a_total"));
    }

    #[test]
    fn double_register_is_tolerated() {
        let _ = register_int_counter("test_metric_b_total", "doc");
        let _ = register_int_counter("test_metric_b_total", "doc");
    }

    #[test]
    fn histogram_vec_with_labels() {
        let h = register_histogram_vec("test_hist_seconds", "doc", &["op"], vec![0.1, 1.0, 10.0]);
        h.with_label_values(&["x"]).observe(0.5);
        let text = gather_text().unwrap();
        assert!(text.contains("test_hist_seconds"));
    }

    #[test]
    fn sqlx_pool_layer_records_real_acquire_wait() {
        let histogram = db_pool_acquire_duration().with_label_values(&[META_POOL_LABEL]);
        let before = histogram.get_sample_count();
        let subscriber = tracing_subscriber::registry().with(SqlxPoolAcquireMetricsLayer);

        tracing::subscriber::with_default(subscriber, || {
            tracing::trace!(
                target: "sqlx::pool::acquire",
                aquired_after_secs = 0.125,
                "acquired connection"
            );
        });

        assert_eq!(histogram.get_sample_count(), before + 1);
    }

    #[test]
    fn structured_snapshot_preserves_histogram_shape_and_labels() {
        let h = register_histogram_vec(
            "test_structured_hist_seconds",
            "doc",
            &["op"],
            vec![0.1, 1.0],
        );
        h.with_label_values(&["read"]).observe(0.5);
        let samples = gather_structured();
        assert!(samples.iter().any(|sample| {
            sample.metric_name == "test_structured_hist_seconds_bucket"
                && sample.labels.get("op").map(String::as_str) == Some("read")
                && sample.labels.get("le").map(String::as_str) == Some("1")
        }));
        assert!(samples.iter().any(|sample| {
            sample.metric_name == "test_structured_hist_seconds_count" && sample.value == 1.0
        }));
        assert!(
            samples
                .iter()
                .any(|sample| sample.metric_name == "test_structured_hist_seconds_sum")
        );
    }

    struct SummaryCollector {
        desc: prometheus::core::Desc,
    }

    impl prometheus::core::Collector for SummaryCollector {
        fn desc(&self) -> Vec<&prometheus::core::Desc> {
            vec![&self.desc]
        }

        fn collect(&self) -> Vec<prometheus::proto::MetricFamily> {
            use prometheus::proto::{
                LabelPair, Metric, MetricFamily, MetricType, Quantile, Summary,
            };
            let mut quantile = Quantile::default();
            quantile.set_quantile(0.9);
            quantile.set_value(42.0);
            let mut summary = Summary::default();
            summary.set_sample_count(3);
            summary.set_sample_sum(84.0);
            summary.set_quantile(vec![quantile]);
            let mut label = LabelPair::default();
            label.set_name("route".into());
            label.set_value("/query".into());
            let mut metric = Metric::default();
            metric.set_label(vec![label]);
            metric.set_summary(summary);
            let mut family = MetricFamily::default();
            family.set_name("test_structured_summary_seconds".into());
            family.set_help("doc".into());
            family.set_field_type(MetricType::SUMMARY);
            family.set_metric(vec![metric]);
            vec![family]
        }
    }

    #[test]
    fn structured_snapshot_preserves_summary_shape_and_labels() {
        global_registry()
            .register(Box::new(SummaryCollector {
                desc: prometheus::core::Desc::new(
                    "test_structured_summary_seconds".into(),
                    "doc".into(),
                    vec!["route".into()],
                    HashMap::new(),
                )
                .unwrap(),
            }))
            .unwrap();
        let samples = gather_structured();
        assert!(samples.iter().any(|sample| {
            sample.metric_name == "test_structured_summary_seconds"
                && sample.metric_kind == "summary_quantile"
                && sample.labels.get("quantile").map(String::as_str) == Some("0.9")
                && sample.labels.get("route").map(String::as_str) == Some("/query")
                && sample.value == 42.0
        }));
        assert!(samples.iter().any(|sample| {
            sample.metric_name == "test_structured_summary_seconds_count"
                && sample.metric_kind == "summary_count"
                && sample.value == 3.0
        }));
        assert!(samples.iter().any(|sample| {
            sample.metric_name == "test_structured_summary_seconds_sum"
                && sample.metric_kind == "summary_sum"
                && sample.value == 84.0
        }));
    }
}
