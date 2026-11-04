// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Prometheus remote_write 接收器（spec intake-protocols Prometheus remote_write Receiver）。
//!
//! `POST /api/v1/prometheus/api/v1/write`
//! - `Content-Encoding: snappy` + `Content-Type: application/x-protobuf`
//! - body: `prometheus.WriteRequest`（prompb v1 子集）
//!
//! 一个请求写进一个指标流：请求头 `stream-name` 指定，缺省 `default`。`__name__` 写进
//! `metric_name`，其余 labels 落到 fields；与指标保留字段同名的 label 存为 `exported_<name>`。
//! 完整请求先做结构限制 preflight，再切成有界 `IntakeBatch`，确保每个 WAL seq 不承载无界
//! sample；`Sample` 的 timestamp 从 ms 转 μs，NaN/Inf 落 null 以兼容 Prometheus staleness
//! marker。原生 Exemplar 以同一指标流的旁路行保存，不写 `value`，因此普通 PromQL sample 查询
//! 不会把 Exemplar 当作采样点。

use std::{borrow::Cow, collections::HashSet, sync::OnceLock, time::Instant};

use ::prometheus::IntCounterVec;
use axum::{
    Extension, Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
};
use prost::Message;
use serde_json::{Map, Number, Value};

use crate::{
    api::AppState,
    app::{
        iam::IamContext,
        intake::usage::{IntakeReceipt, ServiceTally},
    },
    config::PrometheusIntakeSettings,
    domain::{
        iam::permission,
        intake::{IntakeBatch, RawEvent, usage::source::IntakeCredential},
        metrics::{
            METRIC_NAME_FIELD, PROMETHEUS_EXEMPLAR_LABELS_FIELD, PROMETHEUS_EXEMPLAR_MARKER_FIELD,
            PROMETHEUS_EXEMPLAR_VALUE_FIELD, is_metric_identity_storage_field,
            is_prometheus_exemplar_storage_field,
        },
        stream::StreamType,
    },
    infra::intake::SeriesIdentity,
    shared::{Error, Result, ids::Id, metrics::register_int_counter_vec, time::TimestampMicros},
};

static STRUCTURAL_REJECTIONS: OnceLock<IntCounterVec> = OnceLock::new();

pub fn routes() -> Router<AppState> {
    Router::new().route("/prometheus/api/v1/write", post(remote_write))
}

// --- prompb v1 最小子集 -----------------------------------------------------
//
// 仅解 remote_write v1 的 sample / exemplar 必需 message；`metadata`（WriteRequest.tag=3）
// 和 native histogram（TimeSeries.tag=4）由 prost 自动跳过。完整定义见
// prometheus/prompb/{types,remote}.proto。

#[derive(Clone, PartialEq, prost::Message)]
struct PromLabel {
    #[prost(string, tag = "1")]
    name: String,
    #[prost(string, tag = "2")]
    value: String,
}

#[derive(Clone, PartialEq, prost::Message)]
struct PromSample {
    #[prost(double, tag = "1")]
    value: f64,
    /// 毫秒 unix 时间戳
    #[prost(int64, tag = "2")]
    timestamp: i64,
}

#[derive(Clone, PartialEq, prost::Message)]
struct PromExemplar {
    #[prost(message, repeated, tag = "1")]
    labels: Vec<PromLabel>,
    #[prost(double, tag = "2")]
    value: f64,
    /// 毫秒 unix 时间戳
    #[prost(int64, tag = "3")]
    timestamp: i64,
}

#[derive(Clone, PartialEq, prost::Message)]
struct PromTimeSeries {
    #[prost(message, repeated, tag = "1")]
    labels: Vec<PromLabel>,
    #[prost(message, repeated, tag = "2")]
    samples: Vec<PromSample>,
    #[prost(message, repeated, tag = "3")]
    exemplars: Vec<PromExemplar>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct PromWriteRequest {
    #[prost(message, repeated, tag = "1")]
    timeseries: Vec<PromTimeSeries>,
}

// --- handler ---------------------------------------------------------------

#[permission("metrics.write")]
async fn remote_write(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode> {
    let raw = snap::raw::Decoder::new()
        .decompress_vec(&body)
        .map_err(|e| Error::invalid(format!("snappy decode failed: {e}")))?;

    let req = PromWriteRequest::decode(raw.as_slice())
        .map_err(|e| Error::invalid(format!("protobuf decode failed: {e}")))?;

    let settings = &crate::config::get().intake.prometheus;

    let identities = preflight(&req, settings).map_err(|violation| {
        inc_structural_rejection(violation.reason());
        violation.into_error(settings)
    })?;

    state
        .telemetry
        .prometheus_series_admission
        .admit(&ctx.org_id, identities, Instant::now())
        .map_err(|reason| Error::resource_exhausted(reason.client_message()))?;

    let stream = super::stream_name(&headers, super::DEFAULT_STREAM);
    let received_at = TimestampMicros::now();
    let max_samples = settings.max_samples_per_batch.max(1);
    let mut events = Vec::with_capacity(max_samples);
    let mut services = ServiceTally::default();
    let mut accepted = 0_u64;

    for event in MetricEventIter::new(req, received_at) {
        events.push(event);

        if events.len() >= max_samples {
            let chunk = std::mem::replace(&mut events, Vec::with_capacity(max_samples));
            services.add_events(&chunk);
            accepted += intake_chunk(&state, &ctx.org_id, &stream, chunk, received_at).await?;
        }
    }

    services.add_events(&events);
    accepted += intake_chunk(&state, &ctx.org_id, &stream, events, received_at).await?;

    state.platform.intake_meter.record(
        &ctx.org_id,
        IntakeReceipt {
            credential: IntakeCredential::of(&ctx),
            stream_type: StreamType::METRICS,
            stream,
            bytes: raw.len() as u64,
            services,
        },
        accepted,
    );

    Ok(StatusCode::NO_CONTENT)
}

async fn intake_chunk(
    state: &AppState,
    org_id: &Id,
    stream: &str,
    events: Vec<RawEvent>,
    received_at: TimestampMicros,
) -> Result<u64> {
    if events.is_empty() {
        return Ok(0);
    }

    let result = state
        .intake
        .intake(IntakeBatch {
            batch_id: Id::new(),
            org_id: org_id.clone(),
            stream: stream.to_string(),
            stream_type: StreamType::METRICS,
            events,
            received_at,
        })
        .await?;

    Ok(result.accepted as u64)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StructuralViolation {
    MissingMetricName,
    DuplicateLabel,
    EmptyLabelName,
    ReservedLabelName,
    TooManyLabels,
    LabelNameTooLong,
    LabelValueTooLong,
    InvalidExemplarValue,
}

impl StructuralViolation {
    fn reason(self) -> &'static str {
        match self {
            Self::MissingMetricName => "missing_metric_name",
            Self::DuplicateLabel => "duplicate_label",
            Self::EmptyLabelName => "empty_label_name",
            Self::ReservedLabelName => "reserved_label_name",
            Self::TooManyLabels => "too_many_labels",
            Self::LabelNameTooLong => "label_name_too_long",
            Self::LabelValueTooLong => "label_value_too_long",
            Self::InvalidExemplarValue => "invalid_exemplar_value",
        }
    }

    fn into_error(self, settings: &PrometheusIntakeSettings) -> Error {
        let message = match self {
            Self::MissingMetricName => {
                "TimeSeries must contain exactly one non-empty __name__ label".to_string()
            }
            Self::DuplicateLabel => "TimeSeries contains a duplicate label name".to_string(),
            Self::EmptyLabelName => "TimeSeries contains an empty label name".to_string(),
            Self::ReservedLabelName => {
                "TimeSeries label name conflicts with a reserved storage column".to_string()
            }
            Self::TooManyLabels => format!(
                "TimeSeries exceeds max_labels_per_series ({})",
                settings.max_labels_per_series
            ),
            Self::LabelNameTooLong => format!(
                "TimeSeries label name exceeds max_label_name_length ({})",
                settings.max_label_name_length
            ),
            Self::LabelValueTooLong => format!(
                "TimeSeries label value exceeds max_label_value_length ({})",
                settings.max_label_value_length
            ),
            Self::InvalidExemplarValue => "Exemplar value must be a finite number".to_string(),
        };

        Error::invalid(message)
    }
}

fn preflight(
    req: &PromWriteRequest,
    settings: &PrometheusIntakeSettings,
) -> std::result::Result<Vec<SeriesIdentity>, StructuralViolation> {
    let mut identities = Vec::with_capacity(req.timeseries.len());

    for series in &req.timeseries {
        let mut seen: HashSet<Cow<'_, str>> = HashSet::with_capacity(series.labels.len());
        let mut metric_name = None;
        let mut label_count = 0usize;

        for label in &series.labels {
            if label.name.is_empty() {
                return Err(StructuralViolation::EmptyLabelName);
            }

            if label.name.len() > settings.max_label_name_length {
                return Err(StructuralViolation::LabelNameTooLong);
            }

            if label.value.len() > settings.max_label_value_length {
                return Err(StructuralViolation::LabelValueTooLong);
            }

            if !seen.insert(stored_label_name(&label.name)) {
                return Err(StructuralViolation::DuplicateLabel);
            }

            if label.name == "__name__" {
                if !label.value.is_empty() {
                    metric_name = Some(label.value.as_str());
                }

                continue;
            }

            if matches!(label.name.as_str(), "value" | "_timestamp")
                || is_prometheus_exemplar_storage_field(&label.name)
            {
                return Err(StructuralViolation::ReservedLabelName);
            }

            label_count += 1;

            if label_count > settings.max_labels_per_series {
                return Err(StructuralViolation::TooManyLabels);
            }
        }

        let metric_name = metric_name.ok_or(StructuralViolation::MissingMetricName)?;

        for exemplar in &series.exemplars {
            if !exemplar.value.is_finite() {
                return Err(StructuralViolation::InvalidExemplarValue);
            }

            let mut seen: HashSet<&str> = HashSet::with_capacity(exemplar.labels.len());

            if exemplar.labels.len() > settings.max_labels_per_series {
                return Err(StructuralViolation::TooManyLabels);
            }

            for label in &exemplar.labels {
                if label.name.is_empty() {
                    return Err(StructuralViolation::EmptyLabelName);
                }

                if label.name.len() > settings.max_label_name_length {
                    return Err(StructuralViolation::LabelNameTooLong);
                }

                if label.value.len() > settings.max_label_value_length {
                    return Err(StructuralViolation::LabelValueTooLong);
                }

                if !seen.insert(&label.name) {
                    return Err(StructuralViolation::DuplicateLabel);
                }
            }
        }

        if !series.samples.is_empty() || !series.exemplars.is_empty() {
            identities.push(SeriesIdentity::from_labels(
                metric_name,
                series
                    .labels
                    .iter()
                    .filter(|label| label.name != "__name__")
                    .map(|label| (label.name.as_str(), label.value.as_str())),
            ));
        }
    }

    Ok(identities)
}

fn structural_rejections_vec() -> &'static IntCounterVec {
    STRUCTURAL_REJECTIONS.get_or_init(|| {
        register_int_counter_vec(
            "prometheus_remote_write_structural_rejections_total",
            "Prometheus remote-write requests rejected before persistence by bounded reason",
            &["reason"],
        )
    })
}

fn inc_structural_rejection(reason: &str) {
    structural_rejections_vec()
        .with_label_values(&[reason])
        .inc();
}

/// The column a label is stored in. A label named like one of the metric fields, such as
/// `metric_name`, is kept as `exported_<name>`, as Prometheus does for a label that clashes with
/// a target label.
fn stored_label_name(name: &str) -> Cow<'_, str> {
    if is_metric_identity_storage_field(name) {
        Cow::Owned(format!("exported_{name}"))
    } else {
        Cow::Borrowed(name)
    }
}

struct CurrentSeries {
    labels: Map<String, Value>,
    samples: std::vec::IntoIter<PromSample>,
    exemplars: std::vec::IntoIter<PromExemplar>,
}

struct MetricEventIter {
    series: std::vec::IntoIter<PromTimeSeries>,
    current: Option<CurrentSeries>,
    received_at: TimestampMicros,
}

impl MetricEventIter {
    fn new(req: PromWriteRequest, received_at: TimestampMicros) -> Self {
        Self {
            series: req.timeseries.into_iter(),
            current: None,
            received_at,
        }
    }
}

impl Iterator for MetricEventIter {
    type Item = RawEvent;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(current) = &mut self.current
                && let Some(sample) = current.samples.next()
            {
                let timestamp = prometheus_timestamp(sample.timestamp, self.received_at);
                let mut fields = current.labels.clone();
                fields.insert("value".into(), float_to_json(sample.value));
                return Some(RawEvent { timestamp, fields });
            }

            if let Some(current) = &mut self.current
                && let Some(exemplar) = current.exemplars.next()
            {
                let timestamp = prometheus_timestamp(exemplar.timestamp, self.received_at);
                let mut fields = current.labels.clone();

                let exemplar_labels = exemplar
                    .labels
                    .into_iter()
                    .map(|label| (label.name, Value::String(label.value)))
                    .collect();

                fields.insert(PROMETHEUS_EXEMPLAR_MARKER_FIELD.into(), Value::Bool(true));

                fields.insert(
                    PROMETHEUS_EXEMPLAR_VALUE_FIELD.into(),
                    float_to_json(exemplar.value),
                );

                fields.insert(
                    PROMETHEUS_EXEMPLAR_LABELS_FIELD.into(),
                    Value::Object(exemplar_labels),
                );

                return Some(RawEvent { timestamp, fields });
            }

            let series = self.series.next()?;
            let mut labels = Map::new();

            for label in series.labels {
                let name = if label.name == "__name__" {
                    METRIC_NAME_FIELD.to_string()
                } else {
                    stored_label_name(&label.name).into_owned()
                };

                labels.insert(name, Value::String(label.value));
            }

            self.current = Some(CurrentSeries {
                labels,
                samples: series.samples.into_iter(),
                exemplars: series.exemplars.into_iter(),
            });
        }
    }
}

fn prometheus_timestamp(timestamp_millis: i64, received_at: TimestampMicros) -> TimestampMicros {
    if timestamp_millis == 0 {
        received_at
    } else {
        TimestampMicros::from_millis(timestamp_millis)
    }
}

/// Prometheus 用 NaN 表 staleness、可能出现 Inf。serde_json 不允许 NaN/Inf，落为 null。
fn float_to_json(v: f64) -> Value {
    Number::from_f64(v)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::PrometheusCardinalitySettings,
        infra::intake::{PrometheusSeriesAdmission, SeriesLimitReason},
    };

    fn settings() -> PrometheusIntakeSettings {
        PrometheusIntakeSettings::default()
    }

    fn ts(name: &str, label_pairs: &[(&str, &str)], samples: &[(f64, i64)]) -> PromTimeSeries {
        let mut labels = vec![PromLabel {
            name: "__name__".into(),
            value: name.into(),
        }];
        for (k, v) in label_pairs {
            labels.push(PromLabel {
                name: (*k).into(),
                value: (*v).into(),
            });
        }
        PromTimeSeries {
            labels,
            samples: samples
                .iter()
                .map(|(v, t)| PromSample {
                    value: *v,
                    timestamp: *t,
                })
                .collect(),
            exemplars: Vec::new(),
        }
    }

    fn exemplar(label_pairs: &[(&str, &str)], value: f64, timestamp: i64) -> PromExemplar {
        PromExemplar {
            labels: label_pairs
                .iter()
                .map(|(name, value)| PromLabel {
                    name: (*name).into(),
                    value: (*value).into(),
                })
                .collect(),
            value,
            timestamp,
        }
    }

    #[test]
    fn event_iterator_maps_metric_name_and_converts_ms_to_us() {
        let req = PromWriteRequest {
            timeseries: vec![
                ts(
                    "http_requests_total",
                    &[("path", "/a")],
                    &[(1.0, 1000), (2.0, 2000)],
                ),
                ts("http_requests_total", &[("path", "/b")], &[(3.0, 3000)]),
                ts("cpu_seconds", &[("cpu", "0")], &[(0.5, 0)]),
            ],
        };
        let now = TimestampMicros(9_999_999);
        preflight(&req, &settings()).unwrap();
        let events: Vec<_> = MetricEventIter::new(req, now).collect();
        let metric = |name: &str| {
            events
                .iter()
                .filter(|event| event.fields[METRIC_NAME_FIELD] == name)
                .collect::<Vec<_>>()
        };

        let http = metric("http_requests_total");
        assert_eq!(http.len(), 3);
        assert_eq!(http[0].timestamp, TimestampMicros(1_000_000));
        assert_eq!(
            http[0].fields["value"],
            Value::Number(Number::from_f64(1.0).unwrap())
        );
        assert_eq!(http[0].fields["path"], Value::String("/a".into()));
        assert!(!http[0].fields.contains_key("__name__"));

        let cpu = metric("cpu_seconds");
        assert_eq!(cpu.len(), 1);
        // timestamp == 0 → 落到 received_at
        assert_eq!(cpu[0].timestamp, now);
    }

    #[test]
    fn event_iterator_keeps_exemplars_out_of_the_sample_value_column() {
        let mut series = ts(
            "http_request_duration_seconds_bucket",
            &[("service", "checkout"), ("le", "1")],
            &[(42.0, 1_000)],
        );
        series.exemplars.push(exemplar(
            &[
                ("trace_id", "0af7651916cd43dd8448eb211c80319c"),
                ("span_id", "b7ad6b7169203331"),
            ],
            0.875,
            1_000,
        ));
        let req = PromWriteRequest {
            timeseries: vec![series],
        };
        preflight(&req, &settings()).unwrap();

        let events = MetricEventIter::new(req, TimestampMicros(9_999_999)).collect::<Vec<_>>();
        assert_eq!(events.len(), 2);
        let sample = &events[0];
        assert_eq!(sample.fields["value"], serde_json::json!(42.0));
        assert!(!sample.fields.contains_key(PROMETHEUS_EXEMPLAR_MARKER_FIELD));

        let exemplar = &events[1];
        assert_eq!(exemplar.timestamp, TimestampMicros(1_000_000));
        assert_eq!(
            exemplar.fields[METRIC_NAME_FIELD],
            "http_request_duration_seconds_bucket"
        );
        assert!(
            !exemplar.fields.contains_key("value"),
            "exemplar rows must not become ordinary PromQL samples"
        );
        assert_eq!(
            exemplar.fields[PROMETHEUS_EXEMPLAR_MARKER_FIELD],
            Value::Bool(true)
        );
        assert_eq!(
            exemplar.fields[PROMETHEUS_EXEMPLAR_VALUE_FIELD],
            serde_json::json!(0.875)
        );
        assert_eq!(
            exemplar.fields[PROMETHEUS_EXEMPLAR_LABELS_FIELD],
            serde_json::json!({
                "trace_id": "0af7651916cd43dd8448eb211c80319c",
                "span_id": "b7ad6b7169203331"
            })
        );
    }

    #[test]
    fn nan_and_inf_become_null() {
        assert_eq!(float_to_json(f64::NAN), Value::Null);
        assert_eq!(float_to_json(f64::INFINITY), Value::Null);
        assert_eq!(float_to_json(f64::NEG_INFINITY), Value::Null);
        assert!(matches!(float_to_json(1.5), Value::Number(_)));
    }

    #[test]
    fn missing_metric_name_rejected() {
        let req = PromWriteRequest {
            timeseries: vec![PromTimeSeries {
                labels: vec![PromLabel {
                    name: "instance".into(),
                    value: "x".into(),
                }],
                samples: vec![PromSample {
                    value: 1.0,
                    timestamp: 1,
                }],
                exemplars: Vec::new(),
            }],
        };
        assert_eq!(
            preflight(&req, &settings()),
            Err(StructuralViolation::MissingMetricName)
        );
    }

    #[test]
    fn structural_limits_reject_duplicates_reserved_and_oversized_labels() {
        let empty_name = PromWriteRequest {
            timeseries: vec![ts("up", &[("", "hidden")], &[(1.0, 1)])],
        };
        assert_eq!(
            preflight(&empty_name, &settings()),
            Err(StructuralViolation::EmptyLabelName)
        );

        let duplicate = PromWriteRequest {
            timeseries: vec![ts("up", &[("job", "a"), ("job", "b")], &[(1.0, 1)])],
        };
        assert_eq!(
            preflight(&duplicate, &settings()),
            Err(StructuralViolation::DuplicateLabel)
        );

        let reserved = PromWriteRequest {
            timeseries: vec![ts("up", &[("value", "shadow")], &[(1.0, 1)])],
        };
        assert_eq!(
            preflight(&reserved, &settings()),
            Err(StructuralViolation::ReservedLabelName)
        );
        let reserved_exemplar_field = PromWriteRequest {
            timeseries: vec![ts(
                "up",
                &[(PROMETHEUS_EXEMPLAR_MARKER_FIELD, "shadow")],
                &[(1.0, 1)],
            )],
        };
        assert_eq!(
            preflight(&reserved_exemplar_field, &settings()),
            Err(StructuralViolation::ReservedLabelName)
        );

        let mut tight = settings();
        tight.max_labels_per_series = 1;
        let too_many = PromWriteRequest {
            timeseries: vec![ts("up", &[("job", "a"), ("instance", "b")], &[(1.0, 1)])],
        };
        assert_eq!(
            preflight(&too_many, &tight),
            Err(StructuralViolation::TooManyLabels)
        );

        tight.max_label_name_length = 3;
        let long_name = PromWriteRequest {
            timeseries: vec![ts("up", &[("long", "a")], &[(1.0, 1)])],
        };
        assert_eq!(
            preflight(&long_name, &tight),
            Err(StructuralViolation::LabelNameTooLong)
        );

        tight.max_label_name_length = 128;
        tight.max_label_value_length = 3;
        let long_value = PromWriteRequest {
            timeseries: vec![ts("up", &[("job", "long")], &[(1.0, 1)])],
        };
        assert_eq!(
            preflight(&long_value, &tight),
            Err(StructuralViolation::LabelValueTooLong)
        );

        let message = StructuralViolation::LabelValueTooLong
            .into_error(&tight)
            .to_string();
        assert!(!message.contains("long"), "错误信息不得回显 label value");
    }

    #[test]
    fn preflight_validates_exemplar_labels_and_value() {
        let mut duplicate = ts("up", &[("job", "api")], &[]);
        duplicate
            .exemplars
            .push(exemplar(&[("trace_id", "a"), ("trace_id", "b")], 1.0, 1));
        assert_eq!(
            preflight(
                &PromWriteRequest {
                    timeseries: vec![duplicate]
                },
                &settings()
            ),
            Err(StructuralViolation::DuplicateLabel)
        );

        let mut non_finite = ts("up", &[("job", "api")], &[]);
        non_finite
            .exemplars
            .push(exemplar(&[("trace_id", "a")], f64::NAN, 1));
        assert_eq!(
            preflight(
                &PromWriteRequest {
                    timeseries: vec![non_finite]
                },
                &settings()
            ),
            Err(StructuralViolation::InvalidExemplarValue)
        );
    }

    #[test]
    fn a_label_named_like_a_metric_field_is_exported() {
        let req = PromWriteRequest {
            timeseries: vec![ts(
                "up",
                &[("metric_name", "shadow"), ("job", "api")],
                &[(1.0, 1)],
            )],
        };

        preflight(&req, &settings()).unwrap();
        let events = MetricEventIter::new(req, TimestampMicros(0)).collect::<Vec<_>>();
        assert_eq!(events[0].fields[METRIC_NAME_FIELD], "up");
        assert_eq!(events[0].fields["exported_metric_name"], "shadow");

        let clash = PromWriteRequest {
            timeseries: vec![ts(
                "up",
                &[("metric_kind", "a"), ("exported_metric_kind", "b")],
                &[(1.0, 1)],
            )],
        };

        assert_eq!(
            preflight(&clash, &settings()),
            Err(StructuralViolation::DuplicateLabel)
        );
    }

    #[test]
    fn structural_rejection_metric_has_bounded_reason_only() {
        let before = structural_rejections_vec()
            .with_label_values(&["duplicate_label"])
            .get();
        inc_structural_rejection("duplicate_label");
        let after = structural_rejections_vec()
            .with_label_values(&["duplicate_label"])
            .get();
        assert_eq!(after, before + 1);
    }

    #[test]
    fn preflight_identities_feed_atomic_request_admission() {
        let mut exemplar_only = ts("latency_seconds", &[("instance", "exemplar")], &[]);
        exemplar_only
            .exemplars
            .push(exemplar(&[("trace_id", "trace-a")], 0.5, 1));
        let req = PromWriteRequest {
            timeseries: vec![
                ts("up", &[("instance", "secret-a")], &[(1.0, 1)]),
                ts("up", &[("instance", "secret-b")], &[(1.0, 1)]),
                ts("empty", &[("instance", "ignored")], &[]),
                exemplar_only,
            ],
        };
        let identities = preflight(&req, &settings()).unwrap();
        assert_eq!(identities.len(), 3);
        let registry = PrometheusSeriesAdmission::new(
            PrometheusCardinalitySettings {
                enabled: true,
                max_active_series_per_process: 10,
                max_new_series_per_minute: 10,
                idle_ttl_secs: 60,
            },
            1,
            1,
        );
        let error = registry
            .admit(&Id::from_string("org-a"), identities, Instant::now())
            .unwrap_err();
        assert_eq!(error, SeriesLimitReason::OrganizationActive);
        assert!(!error.client_message().contains("secret"));
        assert_eq!(registry.active_series(), 0, "整批拒绝不得部分 admission");
    }

    #[test]
    fn snappy_roundtrip_protobuf_decode() {
        let mut series = ts("up", &[("job", "node")], &[(1.0, 1700000000000)]);
        series.exemplars.push(exemplar(
            &[("trace_id", "0af7651916cd43dd8448eb211c80319c")],
            1.0,
            1_700_000_000_000,
        ));
        let req = PromWriteRequest {
            timeseries: vec![series],
        };
        let proto_bytes = req.encode_to_vec();
        let snappy_bytes = snap::raw::Encoder::new()
            .compress_vec(&proto_bytes)
            .unwrap();
        let decoded_raw = snap::raw::Decoder::new()
            .decompress_vec(&snappy_bytes)
            .unwrap();
        let decoded = PromWriteRequest::decode(decoded_raw.as_slice()).unwrap();
        assert_eq!(decoded.timeseries.len(), 1);
        assert_eq!(decoded.timeseries[0].samples[0].value, 1.0);
        assert_eq!(
            decoded.timeseries[0].exemplars[0].labels[0].name,
            "trace_id"
        );
    }
}
