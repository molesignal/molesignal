// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! OTLP metric normalization into MoleSignal's metric storage contract.

use opentelemetry_proto::tonic::{
    collector::metrics::v1::ExportMetricsServiceRequest,
    common::v1::KeyValue,
    metrics::v1::{AggregationTemporality, Metric, metric, number_data_point},
};
use serde_json::{Map, Value};

use super::{base_fields, nanos_to_micros, put_attrs};
use crate::domain::{
    intake::RawEvent,
    metrics::{
        METRIC_DESCRIPTION_FIELD, METRIC_KIND_FIELD, METRIC_MONOTONIC_FIELD, METRIC_NAME_FIELD,
        METRIC_START_TIME_UNIX_NANO_FIELD, METRIC_TEMPORALITY_FIELD, METRIC_UNIT_FIELD,
    },
};

#[derive(Clone, Copy)]
struct MetricMetadata {
    kind: &'static str,
    temporality: Option<i32>,
    monotonic: Option<bool>,
}

fn temporality_name(raw: i32) -> String {
    match AggregationTemporality::try_from(raw) {
        Ok(AggregationTemporality::Unspecified) => "unspecified".into(),
        Ok(AggregationTemporality::Delta) => "delta".into(),
        Ok(AggregationTemporality::Cumulative) => "cumulative".into(),
        Err(_) => format!("unknown:{raw}"),
    }
}

fn put_metric_metadata(fields: &mut Map<String, Value>, metric: &Metric, meta: MetricMetadata) {
    fields.insert(METRIC_NAME_FIELD.into(), Value::String(metric.name.clone()));
    fields.insert(METRIC_KIND_FIELD.into(), Value::String(meta.kind.into()));
    if !metric.description.is_empty() {
        fields.insert(
            METRIC_DESCRIPTION_FIELD.into(),
            Value::String(metric.description.clone()),
        );
    }
    if !metric.unit.is_empty() {
        fields.insert(METRIC_UNIT_FIELD.into(), Value::String(metric.unit.clone()));
    }
    if let Some(temporality) = meta.temporality {
        fields.insert(
            METRIC_TEMPORALITY_FIELD.into(),
            Value::String(temporality_name(temporality)),
        );
    }
    if let Some(monotonic) = meta.monotonic {
        fields.insert(METRIC_MONOTONIC_FIELD.into(), Value::Bool(monotonic));
    }
}

fn metric_event(
    base: &Map<String, Value>,
    metric: &Metric,
    attributes: &[KeyValue],
    start_time_unix_nano: Option<u64>,
    time_unix_nano: u64,
    meta: MetricMetadata,
) -> RawEvent {
    let mut fields = base.clone();
    put_attrs(&mut fields, attributes);
    // Internal metadata wins over identically named OTLP attributes.
    put_metric_metadata(&mut fields, metric, meta);
    if let Some(start) = start_time_unix_nano.filter(|start| *start != 0) {
        fields.insert(METRIC_START_TIME_UNIX_NANO_FIELD.into(), Value::from(start));
    }
    RawEvent {
        timestamp: nanos_to_micros(time_unix_nano),
        fields,
    }
}

fn put_number(fields: &mut Map<String, Value>, value: &Option<number_data_point::Value>) {
    match value {
        Some(number_data_point::Value::AsDouble(value)) => {
            fields.insert("value".into(), Value::from(*value));
        }
        Some(number_data_point::Value::AsInt(value)) => {
            fields.insert("value".into(), Value::from(*value));
        }
        None => {}
    }
}

pub(crate) fn metrics_to_events(req: ExportMetricsServiceRequest) -> Vec<RawEvent> {
    let mut out = Vec::new();
    for resource_metrics in &req.resource_metrics {
        for scope_metrics in &resource_metrics.scope_metrics {
            let base = base_fields(
                resource_metrics.resource.as_ref(),
                scope_metrics.scope.as_ref(),
            );
            for metric in &scope_metrics.metrics {
                match &metric.data {
                    Some(metric::Data::Gauge(gauge)) => {
                        let meta = MetricMetadata {
                            kind: "gauge",
                            temporality: None,
                            monotonic: None,
                        };
                        for point in &gauge.data_points {
                            // OTLP explicitly ignores StartTimeUnixNano for Gauge.
                            let mut event = metric_event(
                                &base,
                                metric,
                                &point.attributes,
                                None,
                                point.time_unix_nano,
                                meta,
                            );
                            put_number(&mut event.fields, &point.value);
                            out.push(event);
                        }
                    }
                    Some(metric::Data::Sum(sum)) => {
                        let meta = MetricMetadata {
                            kind: if sum.is_monotonic {
                                "counter"
                            } else {
                                "up_down_counter"
                            },
                            temporality: Some(sum.aggregation_temporality),
                            monotonic: Some(sum.is_monotonic),
                        };
                        for point in &sum.data_points {
                            let mut event = metric_event(
                                &base,
                                metric,
                                &point.attributes,
                                Some(point.start_time_unix_nano),
                                point.time_unix_nano,
                                meta,
                            );
                            put_number(&mut event.fields, &point.value);
                            out.push(event);
                        }
                    }
                    Some(metric::Data::Histogram(histogram)) => {
                        let meta = MetricMetadata {
                            kind: "histogram",
                            temporality: Some(histogram.aggregation_temporality),
                            monotonic: None,
                        };
                        for point in &histogram.data_points {
                            let mut event = metric_event(
                                &base,
                                metric,
                                &point.attributes,
                                Some(point.start_time_unix_nano),
                                point.time_unix_nano,
                                meta,
                            );
                            event
                                .fields
                                .insert("count".into(), Value::from(point.count));
                            if let Some(sum) = point.sum {
                                event.fields.insert("sum".into(), Value::from(sum));
                            }
                            if let Some(min) = point.min {
                                event.fields.insert("min".into(), Value::from(min));
                            }
                            if let Some(max) = point.max {
                                event.fields.insert("max".into(), Value::from(max));
                            }
                            out.push(event);
                        }
                    }
                    Some(metric::Data::ExponentialHistogram(histogram)) => {
                        let meta = MetricMetadata {
                            kind: "exponential_histogram",
                            temporality: Some(histogram.aggregation_temporality),
                            monotonic: None,
                        };
                        for point in &histogram.data_points {
                            let mut event = metric_event(
                                &base,
                                metric,
                                &point.attributes,
                                Some(point.start_time_unix_nano),
                                point.time_unix_nano,
                                meta,
                            );
                            event
                                .fields
                                .insert("count".into(), Value::from(point.count));
                            if let Some(sum) = point.sum {
                                event.fields.insert("sum".into(), Value::from(sum));
                            }
                            out.push(event);
                        }
                    }
                    Some(metric::Data::Summary(summary)) => {
                        let meta = MetricMetadata {
                            kind: "summary",
                            // OTLP Summary count and sum are defined as cumulative.
                            temporality: Some(AggregationTemporality::Cumulative as i32),
                            monotonic: None,
                        };
                        for point in &summary.data_points {
                            let mut event = metric_event(
                                &base,
                                metric,
                                &point.attributes,
                                Some(point.start_time_unix_nano),
                                point.time_unix_nano,
                                meta,
                            );
                            event
                                .fields
                                .insert("count".into(), Value::from(point.count));
                            event.fields.insert("sum".into(), Value::from(point.sum));
                            out.push(event);
                        }
                    }
                    None => {}
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use opentelemetry_proto::tonic::{
        common::v1::{AnyValue, any_value},
        metrics::v1::{
            Gauge, Histogram, HistogramDataPoint, NumberDataPoint, ResourceMetrics, ScopeMetrics,
            Sum, metric, number_data_point,
        },
    };

    use super::*;

    fn str_kv(key: &str, value: &str) -> KeyValue {
        KeyValue {
            key: key.into(),
            value: Some(AnyValue {
                value: Some(any_value::Value::StringValue(value.into())),
            }),
            ..Default::default()
        }
    }

    fn request(metrics: Vec<Metric>) -> ExportMetricsServiceRequest {
        ExportMetricsServiceRequest {
            resource_metrics: vec![ResourceMetrics {
                scope_metrics: vec![ScopeMetrics {
                    metrics,
                    ..Default::default()
                }],
                ..Default::default()
            }],
        }
    }

    #[test]
    fn gauge_preserves_kind_and_metric_metadata() {
        let req = request(vec![Metric {
            name: "cpu.usage".into(),
            description: "CPU utilization".into(),
            unit: "1".into(),
            data: Some(metric::Data::Gauge(Gauge {
                data_points: vec![NumberDataPoint {
                    start_time_unix_nano: 1_000,
                    time_unix_nano: 5_000,
                    attributes: vec![str_kv("host", "a"), str_kv(METRIC_KIND_FIELD, "spoofed")],
                    value: Some(number_data_point::Value::AsDouble(0.5)),
                    ..Default::default()
                }],
            })),
            ..Default::default()
        }]);

        let events = metrics_to_events(req);

        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.timestamp.0, 5);
        assert_eq!(event.fields[METRIC_NAME_FIELD], "cpu.usage");
        assert_eq!(event.fields[METRIC_KIND_FIELD], "gauge");
        assert_eq!(event.fields[METRIC_DESCRIPTION_FIELD], "CPU utilization");
        assert_eq!(event.fields[METRIC_UNIT_FIELD], "1");
        assert_eq!(event.fields["host"], "a");
        assert_eq!(event.fields["value"], 0.5);
        assert!(!event.fields.contains_key(METRIC_TEMPORALITY_FIELD));
        assert!(!event.fields.contains_key(METRIC_MONOTONIC_FIELD));
        assert!(!event.fields.contains_key(METRIC_START_TIME_UNIX_NANO_FIELD));
    }

    #[test]
    fn sum_preserves_counter_semantics() {
        let req = request(vec![
            Metric {
                name: "requests".into(),
                data: Some(metric::Data::Sum(Sum {
                    data_points: vec![NumberDataPoint {
                        start_time_unix_nano: 1_000,
                        time_unix_nano: 5_000,
                        value: Some(number_data_point::Value::AsInt(3)),
                        ..Default::default()
                    }],
                    aggregation_temporality: AggregationTemporality::Delta as i32,
                    is_monotonic: true,
                })),
                ..Default::default()
            },
            Metric {
                name: "queue.depth.change".into(),
                data: Some(metric::Data::Sum(Sum {
                    data_points: vec![NumberDataPoint {
                        start_time_unix_nano: 2_000,
                        time_unix_nano: 6_000,
                        value: Some(number_data_point::Value::AsInt(-2)),
                        ..Default::default()
                    }],
                    aggregation_temporality: AggregationTemporality::Cumulative as i32,
                    is_monotonic: false,
                })),
                ..Default::default()
            },
        ]);

        let events = metrics_to_events(req);

        assert_eq!(events.len(), 2);
        let counter = &events[0].fields;
        assert_eq!(counter[METRIC_KIND_FIELD], "counter");
        assert_eq!(counter[METRIC_TEMPORALITY_FIELD], "delta");
        assert_eq!(counter[METRIC_MONOTONIC_FIELD], true);
        assert_eq!(counter[METRIC_START_TIME_UNIX_NANO_FIELD], 1_000);
        assert_eq!(counter["value"], 3);

        let up_down_counter = &events[1].fields;
        assert_eq!(up_down_counter[METRIC_KIND_FIELD], "up_down_counter");
        assert_eq!(up_down_counter[METRIC_TEMPORALITY_FIELD], "cumulative");
        assert_eq!(up_down_counter[METRIC_MONOTONIC_FIELD], false);
        assert_eq!(up_down_counter[METRIC_START_TIME_UNIX_NANO_FIELD], 2_000);
        assert_eq!(up_down_counter["value"], -2);
    }

    #[test]
    fn histogram_preserves_aggregation_semantics() {
        let req = request(vec![Metric {
            name: "request.duration".into(),
            data: Some(metric::Data::Histogram(Histogram {
                data_points: vec![HistogramDataPoint {
                    start_time_unix_nano: 1_000,
                    time_unix_nano: 5_000,
                    count: 2,
                    sum: Some(0.75),
                    ..Default::default()
                }],
                aggregation_temporality: AggregationTemporality::Cumulative as i32,
            })),
            ..Default::default()
        }]);

        let events = metrics_to_events(req);

        assert_eq!(events.len(), 1);
        let fields = &events[0].fields;
        assert_eq!(fields[METRIC_KIND_FIELD], "histogram");
        assert_eq!(fields[METRIC_TEMPORALITY_FIELD], "cumulative");
        assert_eq!(fields[METRIC_START_TIME_UNIX_NANO_FIELD], 1_000);
        assert_eq!(fields["count"], 2);
        assert_eq!(fields["sum"], 0.75);
    }
}
