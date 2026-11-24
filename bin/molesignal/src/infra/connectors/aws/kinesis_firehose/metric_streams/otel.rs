// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The OpenTelemetry output formats of CloudWatch Metric Streams, 1.0.0 and 0.7.0.
//!
//! A record holds one or more `ExportMetricsServiceRequest`s, each after an unsigned varint32 of
//! its length. Every metric is a Summary named `amazonaws.com/<namespace>/<metric>`; quantile 0 is
//! the minimum and quantile 1 the maximum. The resource carries `cloud.account.id` and
//! `cloud.region`.
//!
//! The two versions differ, for a reader, only in where a data point keeps its attributes: 1.0 in
//! `attributes` (tag 7) as `Namespace`, `MetricName` and a `Dimensions` key-value list; 0.7 in
//! `labels` (tag 1) as plain strings, the dimensions beside `Namespace` and `MetricName`. The
//! messages below are the part of the schema both share, with both places for attributes, so one
//! reading serves both. See
//! <https://docs.aws.amazon.com/AmazonCloudWatch/latest/monitoring/CloudWatch-metric-streams-formats-opentelemetry-100.html>.

use domain::intake::RawEvent;
use kernel::time::TimestampMicros;
use opentelemetry_proto::tonic::common::v1::{AnyValue, KeyValue, any_value};
use prost::Message;

use super::{super::Delivery, Datum, quantile_suffix};

#[derive(Clone, PartialEq, Message)]
struct Request {
    #[prost(message, repeated, tag = "1")]
    resource_metrics: Vec<ResourceMetrics>,
}

#[derive(Clone, PartialEq, Message)]
struct ResourceMetrics {
    #[prost(message, optional, tag = "1")]
    resource: Option<Resource>,
    /// `scope_metrics` in 1.0, `instrumentation_library_metrics` in 0.7.
    #[prost(message, repeated, tag = "2")]
    scope_metrics: Vec<ScopeMetrics>,
}

#[derive(Clone, PartialEq, Message)]
struct Resource {
    #[prost(message, repeated, tag = "1")]
    attributes: Vec<KeyValue>,
}

#[derive(Clone, PartialEq, Message)]
struct ScopeMetrics {
    #[prost(message, repeated, tag = "2")]
    metrics: Vec<Metric>,
}

#[derive(Clone, PartialEq, Message)]
struct Metric {
    #[prost(string, tag = "1")]
    name: String,
    #[prost(string, tag = "3")]
    unit: String,
    /// `summary` in 1.0, `double_summary` in 0.7.
    #[prost(message, optional, tag = "11")]
    summary: Option<Summary>,
}

#[derive(Clone, PartialEq, Message)]
struct Summary {
    #[prost(message, repeated, tag = "1")]
    data_points: Vec<SummaryDataPoint>,
}

#[derive(Clone, PartialEq, Message)]
struct SummaryDataPoint {
    /// 0.7 only.
    #[prost(message, repeated, tag = "1")]
    labels: Vec<StringKeyValue>,
    #[prost(fixed64, tag = "3")]
    time_unix_nano: u64,
    #[prost(fixed64, tag = "4")]
    count: u64,
    #[prost(double, tag = "5")]
    sum: f64,
    #[prost(message, repeated, tag = "6")]
    quantile_values: Vec<ValueAtQuantile>,
    /// 1.0 only.
    #[prost(message, repeated, tag = "7")]
    attributes: Vec<KeyValue>,
}

#[derive(Clone, PartialEq, Message)]
struct StringKeyValue {
    #[prost(string, tag = "1")]
    key: String,
    #[prost(string, tag = "2")]
    value: String,
}

#[derive(Clone, PartialEq, Message)]
struct ValueAtQuantile {
    #[prost(double, tag = "1")]
    quantile: f64,
    #[prost(double, tag = "2")]
    value: f64,
}

/// The samples of a record, or `None` when it is not a sequence of length-delimited requests
/// holding at least one Metric Streams data point.
pub fn samples(bytes: &[u8], delivery: &Delivery) -> Option<Vec<RawEvent>> {
    let mut rest = bytes;
    let mut datums = Vec::new();

    while !rest.is_empty() {
        let request = Request::decode_length_delimited(&mut rest).ok()?;

        for resource_metrics in &request.resource_metrics {
            read_resource(resource_metrics, &mut datums);
        }
    }

    if datums.is_empty() {
        return None;
    }

    Some(
        datums
            .iter()
            .flat_map(|datum| datum.samples(delivery))
            .collect(),
    )
}

fn read_resource(resource_metrics: &ResourceMetrics, out: &mut Vec<Datum>) {
    let attributes = resource_metrics
        .resource
        .as_ref()
        .map(|resource| resource.attributes.as_slice())
        .unwrap_or_default();

    let account_id = string_of(attributes, "cloud.account.id");
    let region = string_of(attributes, "cloud.region");

    for metric in resource_metrics
        .scope_metrics
        .iter()
        .flat_map(|scope| &scope.metrics)
    {
        let Some(summary) = &metric.summary else {
            continue;
        };

        let (named_namespace, named_metric) = split_name(&metric.name);

        for point in &summary.data_points {
            let mut namespace = string_of(&point.attributes, "Namespace");
            let mut metric_name = string_of(&point.attributes, "MetricName");
            let mut dimensions = dimensions_of(&point.attributes);

            for label in &point.labels {
                match label.key.as_str() {
                    "Namespace" => namespace = Some(label.value.clone()),
                    "MetricName" => metric_name = Some(label.value.clone()),
                    _ => dimensions.push((label.key.clone(), label.value.clone())),
                }
            }

            let (Some(namespace), Some(metric_name)) = (
                namespace.or_else(|| named_namespace.clone()),
                metric_name.or_else(|| named_metric.clone()),
            ) else {
                continue;
            };

            let mut datum = Datum {
                account_id: account_id.clone(),
                region: region.clone(),
                namespace,
                metric_name,
                dimensions,
                timestamp: TimestampMicros((point.time_unix_nano / 1_000) as i64),
                unit: Some(metric.unit.clone()),
                count: Some(point.count as f64),
                sum: Some(point.sum),
                ..Datum::default()
            };

            for quantile in &point.quantile_values {
                let q = quantile.quantile;

                if q == 0.0 {
                    datum.min = Some(quantile.value);
                } else if q == 1.0 {
                    datum.max = Some(quantile.value);
                } else {
                    datum.extra.push((quantile_suffix(q), quantile.value));
                }
            }

            out.push(datum);
        }
    }
}

/// `amazonaws.com/AWS/EC2/CPUUtilization` → (`AWS/EC2`, `CPUUtilization`).
fn split_name(name: &str) -> (Option<String>, Option<String>) {
    let Some(path) = name.strip_prefix("amazonaws.com/") else {
        return (None, None);
    };

    match path.rsplit_once('/') {
        Some((namespace, metric)) => (Some(namespace.into()), Some(metric.into())),
        None => (None, None),
    }
}

fn string_of(attributes: &[KeyValue], key: &str) -> Option<String> {
    attributes
        .iter()
        .find(|attribute| attribute.key == key)
        .and_then(|attribute| text(attribute.value.as_ref()?))
}

/// The `Dimensions` key-value list of a 1.0 data point.
fn dimensions_of(attributes: &[KeyValue]) -> Vec<(String, String)> {
    let list = attributes
        .iter()
        .find(|attribute| attribute.key == "Dimensions")
        .and_then(|attribute| attribute.value.as_ref()?.value.as_ref());

    match list {
        Some(any_value::Value::KvlistValue(list)) => list
            .values
            .iter()
            .filter_map(|pair| Some((pair.key.clone(), text(pair.value.as_ref()?)?)))
            .collect(),
        _ => Vec::new(),
    }
}

fn text(value: &AnyValue) -> Option<String> {
    match value.value.as_ref()? {
        any_value::Value::StringValue(text) => Some(text.clone()),
        any_value::Value::IntValue(number) => Some(number.to_string()),
        any_value::Value::DoubleValue(number) => Some(number.to_string()),
        any_value::Value::BoolValue(flag) => Some(flag.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use domain::metrics::METRIC_NAME_FIELD;
    use opentelemetry_proto::tonic::{
        collector::metrics::v1::ExportMetricsServiceRequest,
        common::v1::KeyValueList,
        metrics::v1::{
            self as metrics, ResourceMetrics as OtelResourceMetrics, ScopeMetrics as OtelScope,
            summary_data_point::ValueAtQuantile as OtelQuantile,
        },
        resource::v1::Resource as OtelResource,
    };

    use super::{
        super::super::{FirehoseRequest, decode, tests::record},
        *,
    };

    fn string(key: &str, value: &str) -> KeyValue {
        KeyValue {
            key: key.into(),
            value: Some(AnyValue {
                value: Some(any_value::Value::StringValue(value.into())),
            }),
            ..Default::default()
        }
    }

    fn quantile(quantile: f64, value: f64) -> OtelQuantile {
        OtelQuantile { quantile, value }
    }

    /// A 1.0 request as CloudWatch writes it.
    fn version_1() -> ExportMetricsServiceRequest {
        let dimensions = KeyValue {
            key: "Dimensions".into(),
            value: Some(AnyValue {
                value: Some(any_value::Value::KvlistValue(KeyValueList {
                    values: vec![string("InstanceId", "i-0abc")],
                })),
            }),
            ..Default::default()
        };

        ExportMetricsServiceRequest {
            resource_metrics: vec![OtelResourceMetrics {
                resource: Some(OtelResource {
                    attributes: vec![
                        string("cloud.provider", "aws"),
                        string("cloud.account.id", "123456789012"),
                        string("cloud.region", "us-east-1"),
                    ],
                    ..Default::default()
                }),
                scope_metrics: vec![OtelScope {
                    metrics: vec![metrics::Metric {
                        name: "amazonaws.com/AWS/EC2/CPUUtilization".into(),
                        unit: "%".into(),
                        data: Some(metrics::metric::Data::Summary(metrics::Summary {
                            data_points: vec![metrics::SummaryDataPoint {
                                attributes: vec![
                                    string("Namespace", "AWS/EC2"),
                                    string("MetricName", "CPUUtilization"),
                                    dimensions,
                                ],
                                start_time_unix_nano: 1_700_000_000_000_000_000 - 60_000_000_000,
                                time_unix_nano: 1_700_000_000_000_000_000,
                                count: 2,
                                sum: 30.0,
                                quantile_values: vec![
                                    quantile(0.0, 10.0),
                                    quantile(0.99, 19.5),
                                    quantile(1.0, 20.0),
                                ],
                                flags: 0,
                            }],
                        })),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        }
    }

    fn value(samples: &[RawEvent], name: &str) -> Option<f64> {
        samples
            .iter()
            .find(|sample| sample.fields[METRIC_NAME_FIELD] == name)
            .and_then(|sample| sample.fields["value"].as_f64())
    }

    #[test]
    fn version_1_records_become_metric_samples() {
        let mut bytes = version_1().encode_length_delimited_to_vec();
        bytes.extend(version_1().encode_length_delimited_to_vec());

        let request = FirehoseRequest {
            request_id: "r".into(),
            timestamp: 0,
            records: vec![record(&bytes)],
        };

        let decoded = decode(request, &Delivery::default());
        let samples = &decoded.metrics;

        assert_eq!(decoded.rejected, 0);
        assert_eq!(samples.len(), 2 * 6);
        assert_eq!(value(samples, "aws_ec2_cpuutilization_average"), Some(15.0));
        assert_eq!(value(samples, "aws_ec2_cpuutilization_minimum"), Some(10.0));
        assert_eq!(value(samples, "aws_ec2_cpuutilization_maximum"), Some(20.0));
        assert_eq!(value(samples, "aws_ec2_cpuutilization_p99"), Some(19.5));

        let first = &samples[0];

        assert_eq!(first.timestamp.0, 1_700_000_000_000_000);
        assert_eq!(first.fields["dimension_InstanceId"], "i-0abc");
        assert_eq!(first.fields["account_id"], "123456789012");
        assert_eq!(first.fields["region"], "us-east-1");
    }

    #[test]
    fn version_0_7_labels_carry_the_dimensions() {
        let request = Request {
            resource_metrics: vec![ResourceMetrics {
                resource: Some(Resource {
                    attributes: vec![string("cloud.region", "eu-west-1")],
                }),
                scope_metrics: vec![ScopeMetrics {
                    metrics: vec![Metric {
                        name: "amazonaws.com/AWS/SQS/NumberOfMessagesSent".into(),
                        unit: "1".into(),
                        summary: Some(Summary {
                            data_points: vec![SummaryDataPoint {
                                labels: vec![
                                    StringKeyValue {
                                        key: "Namespace".into(),
                                        value: "AWS/SQS".into(),
                                    },
                                    StringKeyValue {
                                        key: "MetricName".into(),
                                        value: "NumberOfMessagesSent".into(),
                                    },
                                    StringKeyValue {
                                        key: "QueueName".into(),
                                        value: "orders".into(),
                                    },
                                ],
                                time_unix_nano: 1_700_000_000_000_000_000,
                                count: 1,
                                sum: 42.0,
                                quantile_values: Vec::new(),
                                attributes: Vec::new(),
                            }],
                        }),
                    }],
                }],
            }],
        };

        let samples = samples(
            &request.encode_length_delimited_to_vec(),
            &Delivery::default(),
        )
        .unwrap();

        assert_eq!(
            value(&samples, "aws_sqs_number_of_messages_sent_sum"),
            Some(42.0)
        );
        assert_eq!(samples[0].fields["dimension_QueueName"], "orders");
        assert_eq!(samples[0].fields["region"], "eu-west-1");
    }

    #[test]
    fn bytes_that_are_no_metric_stream_are_refused() {
        assert!(samples(&[0x0a, 0xff, 0x01], &Delivery::default()).is_none());
        assert!(
            samples(
                &ExportMetricsServiceRequest::default().encode_length_delimited_to_vec(),
                &Delivery::default()
            )
            .is_none()
        );
    }
}
