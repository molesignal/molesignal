// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Reading one metric type's time series from Cloud Monitoring, aligned to the connector's
//! period, as samples.
//!
//! How a series can be aligned depends on its descriptor's kind and value type ([`plan`]): a
//! gauge is averaged, a delta summed, a cumulative counter turned into a rate per second, a
//! boolean gauge into the fraction of true samples, and a distribution reads as its count and
//! mean over the period. Google stamps an aligned point with the end of its period; a sample
//! carries the start.

use chrono::{DateTime, Utc};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::Value;

use super::settings::Settings;
use crate::infra::connectors::cloud_metrics::{MetricPoint, Window, at_seconds, naming};

const API: &str = "https://monitoring.googleapis.com/v3";

/// How many points one page of `timeSeries.list` holds at most.
pub(super) const PAGE_SIZE: u32 = 10_000;

/// What a query value keeps as it is; the rest is percent-encoded.
const QUERY_VALUE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~')
    .remove(b':');

/// What a metric descriptor says about how to read the metric.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Descriptor {
    pub metric_kind: String,
    pub value_type: String,
    pub unit: String,
}

impl Descriptor {
    pub(super) fn parse(body: &Value) -> Self {
        let field = |key: &str| {
            body.get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        };

        Self {
            metric_kind: field("metricKind"),
            value_type: field("valueType"),
            unit: field("unit"),
        }
    }
}

/// What an aligned value is read as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Reading {
    /// One number, named by its statistic.
    Number(&'static str),
    /// A distribution's `count` and `mean`.
    Distribution,
}

/// How one metric type is aligned and read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Plan {
    pub aligner: &'static str,
    pub reading: Reading,
}

/// How a metric of `descriptor` is aligned, or why it is not read.
pub(super) fn plan(descriptor: &Descriptor) -> Result<Plan, String> {
    let numeric = matches!(descriptor.value_type.as_str(), "INT64" | "DOUBLE");

    let number = |aligner, statistic| {
        Ok(Plan {
            aligner,
            reading: Reading::Number(statistic),
        })
    };

    let distribution = |aligner| {
        Ok(Plan {
            aligner,
            reading: Reading::Distribution,
        })
    };

    match (
        descriptor.metric_kind.as_str(),
        descriptor.value_type.as_str(),
    ) {
        ("GAUGE", _) if numeric => number("ALIGN_MEAN", "mean"),
        ("DELTA", _) if numeric => number("ALIGN_SUM", "sum"),
        ("CUMULATIVE", _) if numeric => number("ALIGN_RATE", "rate"),
        ("GAUGE", "BOOL") => number("ALIGN_FRACTION_TRUE", "fraction_true"),
        // ALIGN_DELTA is not valid for gauges; summing a period's distributions merges them.
        ("GAUGE", "DISTRIBUTION") => distribution("ALIGN_SUM"),
        ("DELTA" | "CUMULATIVE", "DISTRIBUTION") => distribution("ALIGN_DELTA"),
        (_, "STRING") => Err("its values are strings, which are not numbers".to_string()),
        (kind, value_type) => Err(format!(
            "a {} metric of {} values is not read",
            kind.to_ascii_lowercase(),
            value_type.to_ascii_lowercase()
        )),
    }
}

/// The name of a sample of `metric_type`: `compute.googleapis.com/instance/cpu/utilization` and
/// `mean` as `gcp_compute_instance_cpu_utilization_mean`.
pub(super) fn sample_name(metric_type: &str, statistic: &str) -> String {
    let (service, path) = metric_type.split_once('/').unwrap_or((metric_type, ""));
    let service = service.strip_suffix(".googleapis.com").unwrap_or(service);

    naming::metric_name("gcp", service, path, Some(statistic))
}

/// The URL of the descriptor of `metric_type`.
pub(super) fn descriptor_url(project_id: &str, metric_type: &str) -> String {
    format!("{API}/projects/{project_id}/metricDescriptors/{metric_type}")
}

/// A time as the API takes it, RFC 3339 in UTC.
pub(super) fn timestamp(seconds: i64) -> String {
    DateTime::<Utc>::from_timestamp(seconds, 0)
        .unwrap_or_default()
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

/// The filter of `metric_type`, with the connector's clause ANDed.
pub(super) fn filter(metric_type: &str, extra: Option<&str>) -> String {
    match extra {
        Some(extra) => format!("metric.type = \"{metric_type}\" AND ({extra})"),
        None => format!("metric.type = \"{metric_type}\""),
    }
}

/// The `timeSeries.list` URL of one page of `metric_type` over `window`.
///
/// A read covers `(startTime, endTime]` and aligned periods end at `endTime`, so the window's
/// edges give exactly its periods.
pub(super) fn list_url(
    settings: &Settings,
    metric_type: &str,
    plan: Plan,
    window: Window,
    page_token: Option<&str>,
) -> String {
    let filter = filter(metric_type, settings.filter.as_deref());
    let start = timestamp(window.start);
    let end = timestamp(window.end);
    let period = format!("{}s", settings.schedule.period_secs);
    let page_size = PAGE_SIZE.to_string();

    let mut query = vec![
        ("filter", filter.as_str()),
        ("interval.startTime", start.as_str()),
        ("interval.endTime", end.as_str()),
        ("aggregation.alignmentPeriod", period.as_str()),
        ("aggregation.perSeriesAligner", plan.aligner),
        ("view", "FULL"),
        ("pageSize", page_size.as_str()),
    ];

    if let Some(token) = page_token {
        query.push(("pageToken", token));
    }

    let query = query
        .into_iter()
        .map(|(key, value)| format!("{key}={}", utf8_percent_encode(value, QUERY_VALUE)))
        .collect::<Vec<_>>()
        .join("&");

    format!("{API}/projects/{}/timeSeries?{query}", settings.project_id)
}

/// What one page is read with.
pub(super) struct Reader<'a> {
    pub settings: &'a Settings,
    pub metric_type: &'a str,
    pub plan: Plan,
    pub descriptor: &'a Descriptor,
    pub window: Window,
}

impl Reader<'_> {
    /// The samples of one page of `timeSeries.list`, and the token of the next page.
    pub(super) fn page(&self, body: &Value) -> (Vec<MetricPoint>, Option<String>) {
        let mut points = Vec::new();

        for series in body
            .get("timeSeries")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            self.series(series, &mut points);
        }

        let next = body
            .get("nextPageToken")
            .and_then(Value::as_str)
            .filter(|token| !token.is_empty())
            .map(str::to_string);

        (points, next)
    }

    fn series(&self, series: &Value, points: &mut Vec<MetricPoint>) {
        let labels = self.labels(series);
        let period = self.settings.schedule.period_secs;

        for point in series
            .get("points")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(end) = point
                .get("interval")
                .and_then(|interval| interval.get("endTime"))
                .and_then(Value::as_str)
                .and_then(|end| DateTime::parse_from_rfc3339(end).ok())
                .map(|end| end.timestamp())
                .filter(|end| *end > self.window.start && *end <= self.window.end)
            else {
                continue;
            };

            let value = point.get("value").unwrap_or(&Value::Null);

            for (statistic, value, unit) in self.values(value) {
                points.push(MetricPoint {
                    name: sample_name(self.metric_type, statistic),
                    timestamp: at_seconds(end - period),
                    value,
                    labels: labels.clone(),
                    unit,
                });
            }
        }
    }

    /// The statistics an aligned value reads as, with their units.
    fn values(&self, value: &Value) -> Vec<(&'static str, f64, Option<String>)> {
        let unit = Some(self.descriptor.unit.clone()).filter(|unit| !unit.is_empty());

        match self.plan.reading {
            Reading::Number(statistic) => {
                let unit = match statistic {
                    "rate" => Some(per_second(&self.descriptor.unit)),
                    "fraction_true" => Some("1".to_string()),
                    _ => unit,
                };

                number(value)
                    .map(|n| vec![(statistic, n, unit)])
                    .unwrap_or_default()
            }
            Reading::Distribution => {
                let Some(distribution) = value.get("distributionValue") else {
                    return Vec::new();
                };

                let count = distribution.get("count").and_then(as_f64).unwrap_or(0.0);
                let mut values = vec![("count", count, Some("1".to_string()))];

                // The mean of no values is not zero, it is nothing.
                if count > 0.0 {
                    let mean = distribution.get("mean").and_then(as_f64).unwrap_or(0.0);

                    values.push(("mean", mean, unit));
                }

                values
            }
        }
    }

    /// The labels of a series: its project and resource type, every resource label, and every
    /// metric label whose name no resource label has.
    fn labels(&self, series: &Value) -> Vec<(String, String)> {
        let resource = series.get("resource").unwrap_or(&Value::Null);

        let mut labels = vec![
            ("project_id".to_string(), self.settings.project_id.clone()),
            (
                "resource_type".to_string(),
                resource
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            ),
        ];

        let resource_labels = resource.get("labels").and_then(Value::as_object);
        let metric_labels = series
            .get("metric")
            .and_then(|metric| metric.get("labels"))
            .and_then(Value::as_object);

        for (key, value) in resource_labels.into_iter().flatten() {
            set(&mut labels, naming::label_safe(key), text(value), true);
        }

        for (key, value) in metric_labels.into_iter().flatten() {
            set(&mut labels, naming::label_safe(key), text(value), false);
        }

        labels
    }
}

/// Sets `label`; an existing one is replaced only when `replace` says so.
fn set(labels: &mut Vec<(String, String)>, label: String, value: String, replace: bool) {
    match labels.iter_mut().find(|(name, _)| *name == label) {
        Some((_, existing)) if replace => *existing = value,
        Some(_) => {}
        None => labels.push((label, value)),
    }
}

fn text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// A TypedValue as a number: `doubleValue`, `int64Value` (a string in JSON) or `boolValue`.
fn number(value: &Value) -> Option<f64> {
    if let Some(double) = value.get("doubleValue") {
        return as_f64(double);
    }

    if let Some(int) = value.get("int64Value") {
        return as_f64(int);
    }

    value
        .get("boolValue")
        .and_then(Value::as_bool)
        .map(|bool| if bool { 1.0 } else { 0.0 })
}

/// A JSON number, or a number in a string as JSON carries 64-bit integers and `NaN`.
fn as_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.parse().ok(),
        _ => None,
    }
}

/// The unit of a rate per second of values in `unit`.
fn per_second(unit: &str) -> String {
    match unit {
        "" | "1" => "1/s".to_string(),
        unit => format!("{unit}/s"),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::infra::connectors::cloud_metrics::Schedule;

    fn descriptor(kind: &str, value_type: &str, unit: &str) -> Descriptor {
        Descriptor {
            metric_kind: kind.into(),
            value_type: value_type.into(),
            unit: unit.into(),
        }
    }

    fn settings(filter: Option<&str>) -> Settings {
        Settings {
            account: crate::infra::connectors::gcp::service_account::ServiceAccount {
                client_email: "reader@p.iam.gserviceaccount.com".into(),
                private_key: String::new(),
                token_uri: String::new(),
                project_id: "my-project".into(),
            },
            project_id: "my-project".into(),
            metrics: vec!["compute.googleapis.com/instance/cpu/utilization".into()],
            filter: filter.map(str::to_string),
            schedule: Schedule {
                period_secs: 300,
                delay_secs: 300,
            },
        }
    }

    /// 2024-01-01T00:00:00Z to 00:15:00Z, three five-minute periods.
    const WINDOW: Window = Window {
        start: 1_704_067_200,
        end: 1_704_067_200 + 900,
        more: false,
        skipped: false,
    };

    #[test]
    fn the_aligner_follows_the_kind_and_value_type() {
        let aligner = |kind, value_type| plan(&descriptor(kind, value_type, "")).map(|p| p.aligner);

        assert_eq!(aligner("GAUGE", "DOUBLE"), Ok("ALIGN_MEAN"));
        assert_eq!(aligner("GAUGE", "INT64"), Ok("ALIGN_MEAN"));
        assert_eq!(aligner("DELTA", "INT64"), Ok("ALIGN_SUM"));
        assert_eq!(aligner("CUMULATIVE", "INT64"), Ok("ALIGN_RATE"));
        assert_eq!(aligner("GAUGE", "BOOL"), Ok("ALIGN_FRACTION_TRUE"));
        assert_eq!(aligner("GAUGE", "DISTRIBUTION"), Ok("ALIGN_SUM"));
        assert_eq!(aligner("DELTA", "DISTRIBUTION"), Ok("ALIGN_DELTA"));
        assert_eq!(aligner("CUMULATIVE", "DISTRIBUTION"), Ok("ALIGN_DELTA"));
        assert!(aligner("GAUGE", "STRING").is_err());
        assert!(aligner("CUMULATIVE", "BOOL").is_err());
    }

    #[test]
    fn samples_are_named_by_service_and_path() {
        assert_eq!(
            sample_name("compute.googleapis.com/instance/cpu/utilization", "mean"),
            "gcp_compute_instance_cpu_utilization_mean"
        );
        assert_eq!(
            sample_name("loadbalancing.googleapis.com/https/request_count", "rate"),
            "gcp_loadbalancing_https_request_count_rate"
        );
        assert_eq!(
            sample_name("custom.googleapis.com/checkout/latency", "count"),
            "gcp_custom_checkout_latency_count"
        );
    }

    #[test]
    fn the_list_url_asks_for_the_window_aligned_to_the_period() {
        let plan = plan(&descriptor("GAUGE", "DOUBLE", "10^2.%")).unwrap();
        let url = list_url(
            &settings(Some("resource.labels.zone = \"us-central1-a\"")),
            "compute.googleapis.com/instance/cpu/utilization",
            plan,
            WINDOW,
            Some("next"),
        );

        assert_eq!(
            url,
            "https://monitoring.googleapis.com/v3/projects/my-project/timeSeries?\
             filter=metric.type%20%3D%20%22compute.googleapis.com%2Finstance%2Fcpu%2Futilization%22%20AND%20\
             %28resource.labels.zone%20%3D%20%22us-central1-a%22%29&\
             interval.startTime=2024-01-01T00:00:00Z&interval.endTime=2024-01-01T00:15:00Z&\
             aggregation.alignmentPeriod=300s&aggregation.perSeriesAligner=ALIGN_MEAN&view=FULL&\
             pageSize=10000&pageToken=next"
        );
        assert_eq!(
            descriptor_url(
                "my-project",
                "compute.googleapis.com/instance/cpu/utilization"
            ),
            "https://monitoring.googleapis.com/v3/projects/my-project/metricDescriptors/\
             compute.googleapis.com/instance/cpu/utilization"
        );
    }

    #[test]
    fn a_page_reads_as_samples_stamped_at_the_start_of_their_period() {
        let settings = settings(None);
        let descriptor = descriptor("GAUGE", "DOUBLE", "10^2.%");
        let reader = Reader {
            settings: &settings,
            metric_type: "compute.googleapis.com/instance/cpu/utilization",
            plan: plan(&descriptor).unwrap(),
            descriptor: &descriptor,
            window: WINDOW,
        };

        // A TimeSeries as `timeSeries.list` returns it, newest point first.
        let body = json!({
            "timeSeries": [{
                "metric": {
                    "labels": { "instance_name": "web-1", "zone": "shadowed" },
                    "type": "compute.googleapis.com/instance/cpu/utilization"
                },
                "resource": {
                    "type": "gce_instance",
                    "labels": { "instance_id": "1234567890", "zone": "us-central1-a", "project_id": "my-project" }
                },
                "metricKind": "GAUGE",
                "valueType": "DOUBLE",
                "points": [
                    { "interval": { "startTime": "2024-01-01T00:15:00Z", "endTime": "2024-01-01T00:15:00Z" }, "value": { "doubleValue": 0.3 } },
                    { "interval": { "startTime": "2024-01-01T00:10:00Z", "endTime": "2024-01-01T00:10:00Z" }, "value": { "doubleValue": 0.2 } },
                    { "interval": { "startTime": "2024-01-01T00:00:00Z", "endTime": "2024-01-01T00:00:00Z" }, "value": { "doubleValue": 0.9 } }
                ]
            }],
            "unit": "10^2.%",
            "nextPageToken": "page-2"
        });

        let (points, next) = reader.page(&body);

        assert_eq!(next.as_deref(), Some("page-2"));
        // The point ending at the window's start is the last window's.
        assert_eq!(points.len(), 2);
        assert_eq!(points[0].name, "gcp_compute_instance_cpu_utilization_mean");
        assert_eq!(points[0].timestamp, at_seconds(WINDOW.start + 600));
        assert_eq!(points[0].value, 0.3);
        assert_eq!(points[0].unit.as_deref(), Some("10^2.%"));
        assert_eq!(points[1].timestamp, at_seconds(WINDOW.start + 300));

        let label = |name: &str| {
            points[0]
                .labels
                .iter()
                .find(|(label, _)| label == name)
                .map(|(_, value)| value.as_str())
        };

        assert_eq!(label("project_id"), Some("my-project"));
        assert_eq!(label("resource_type"), Some("gce_instance"));
        assert_eq!(label("instance_id"), Some("1234567890"));
        assert_eq!(label("instance_name"), Some("web-1"));
        assert_eq!(label("zone"), Some("us-central1-a"));
    }

    #[test]
    fn rates_integers_and_distributions_read_as_numbers() {
        let settings = settings(None);
        let counter = descriptor("CUMULATIVE", "INT64", "By");
        let rate = Reader {
            settings: &settings,
            metric_type: "compute.googleapis.com/instance/network/received_bytes_count",
            plan: plan(&counter).unwrap(),
            descriptor: &counter,
            window: WINDOW,
        };

        assert_eq!(
            rate.values(&json!({ "doubleValue": 1250.5 })),
            [("rate", 1250.5, Some("By/s".to_string()))]
        );

        let delta = descriptor("DELTA", "INT64", "1");
        let sum = Reader {
            plan: plan(&delta).unwrap(),
            descriptor: &delta,
            ..rate
        };

        assert_eq!(
            sum.values(&json!({ "int64Value": "42" })),
            [("sum", 42.0, Some("1".to_string()))]
        );

        let latencies = descriptor("DELTA", "DISTRIBUTION", "ms");
        let distribution = Reader {
            plan: plan(&latencies).unwrap(),
            descriptor: &latencies,
            ..sum
        };

        assert_eq!(
            distribution.values(&json!({
                "distributionValue": {
                    "count": "12",
                    "mean": 85.5,
                    "sumOfSquaredDeviation": 100.0,
                    "bucketOptions": { "exponentialBuckets": { "numFiniteBuckets": 10, "growthFactor": 2, "scale": 1 } },
                    "bucketCounts": ["0", "12"]
                }
            })),
            [
                ("count", 12.0, Some("1".to_string())),
                ("mean", 85.5, Some("ms".to_string()))
            ]
        );
        assert_eq!(
            distribution.values(&json!({ "distributionValue": { "bucketCounts": [] } })),
            [("count", 0.0, Some("1".to_string()))]
        );
    }
}
