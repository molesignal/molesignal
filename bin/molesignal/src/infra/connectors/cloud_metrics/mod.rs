// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Pulling metrics from a cloud's monitoring API.
//!
//! Every cloud kind works the same way: on each tick the runner asks it for the next [`Window`]
//! of whole periods (see [`window`]); it lists the metrics its settings name, asks the cloud for
//! their aggregates over that range and returns them as [`MetricPoint`]s, one per statistic per
//! period, named by [`naming`]. The runner writes them to the connector's `metricsStream` and keeps
//! the window's end as the read position, so a failed write reads the same range again.
//!
//! Each cloud's kind lives in its vendor's directory: [`super::aws::cloudwatch_metrics`],
//! [`super::azure::monitor`], [`super::gcp::monitoring`], [`super::aliyun::cms`],
//! [`super::tencent::monitor`] and [`super::huawei::ces`].

use std::time::Duration;

use domain::{
    intake::RawEvent,
    metrics::{METRIC_KIND_FIELD, METRIC_NAME_FIELD, METRIC_UNIT_FIELD},
};
use kernel::{Error, Result, time::TimestampMicros};
use serde_json::{Map, Value};

use super::{ConnectorKind, config::text};

pub mod naming;
pub mod window;

pub use window::{Schedule, Window};

/// How long one request to a monitoring API may take.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// One value of one statistic of one series at one time.
#[derive(Debug, Clone, PartialEq)]
pub struct MetricPoint {
    /// The full name, from [`naming::metric_name`].
    pub name: String,
    /// The start of the period the value covers.
    pub timestamp: TimestampMicros,
    pub value: f64,
    /// Label names already safe ([`naming::label_safe`]), values as the cloud gave them.
    pub labels: Vec<(String, String)>,
    pub unit: Option<String>,
}

impl MetricPoint {
    /// The point as a gauge sample; `None` for a value that is not a finite number.
    pub fn into_event(self) -> Option<RawEvent> {
        if !self.value.is_finite() {
            return None;
        }

        let mut fields = Map::new();

        for (label, value) in self.labels {
            if !value.is_empty() {
                fields.insert(label, Value::String(value));
            }
        }

        fields.insert(METRIC_NAME_FIELD.into(), Value::String(self.name));
        fields.insert(METRIC_KIND_FIELD.into(), Value::String("gauge".into()));
        fields.insert("value".into(), Value::from(self.value));

        if let Some(unit) = self.unit.filter(|unit| !unit.is_empty() && unit != "None") {
            fields.insert(METRIC_UNIT_FIELD.into(), Value::String(unit));
        }

        Some(RawEvent {
            timestamp: self.timestamp,
            fields,
        })
    }
}

/// The samples of `points`, those with a finite value.
pub fn events(points: Vec<MetricPoint>) -> Vec<RawEvent> {
    points
        .into_iter()
        .filter_map(MetricPoint::into_event)
        .collect()
}

/// Seconds since the epoch as a sample time.
pub fn at_seconds(seconds: i64) -> TimestampMicros {
    TimestampMicros(seconds.saturating_mul(1_000_000))
}

/// The metrics stream a connector of `kind` writes to when it names none.
pub fn default_metrics_stream(kind: ConnectorKind) -> &'static str {
    match kind {
        ConnectorKind::AwsKinesisFirehose | ConnectorKind::AwsCloudwatchMetrics => "cloudwatch",
        ConnectorKind::AzureMonitorMetrics => "azure",
        ConnectorKind::GcpMonitoringMetrics => "gcp",
        ConnectorKind::AliyunCmsMetrics => "aliyun",
        ConnectorKind::TencentMonitorMetrics => "tencentcloud",
        ConnectorKind::HuaweiCesMetrics => "huaweicloud",
        _ => "default",
    }
}

/// A list setting: a JSON array of strings, or one string of comma- or newline-separated items.
pub fn list(config: &Value, key: &str) -> Vec<String> {
    let items: Vec<String> = match config.get(key) {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        Some(Value::String(_)) => text(config, key)
            .split([',', '\n'])
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    };

    items
        .into_iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

/// A required setting, or why it is missing.
pub fn required<'a>(config: &'a Value, key: &str, kind: ConnectorKind) -> Result<&'a str> {
    match text(config, key) {
        "" => Err(Error::invalid(format!(
            "`{}` is required for {}",
            naming::snake(key),
            kind.as_str()
        ))),
        value => Ok(value),
    }
}

/// An HTTP client for a monitoring API.
pub fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| Error::internal(format!("monitoring api client: {e}")))
}

/// A refused request to a monitoring API as an error: `401` and `403` say the credentials or
/// their permissions are wrong, `429` that the API's rate limit was hit, anything else that the
/// API is unavailable for now.
pub fn refusal(api: &str, status: u16, body: &str) -> Error {
    let body: String = body.chars().take(500).collect();
    let message = format!("{api} answered {status}: {body}");

    match status {
        401 => Error::unauthorized(message),
        403 => Error::forbidden(message),
        404 => Error::not_found(message),
        400 | 422 => Error::invalid(message),
        429 => Error::resource_exhausted(message),
        _ => Error::unavailable(message),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_point_is_a_gauge_sample_with_its_labels() {
        let point = MetricPoint {
            name: "aws_ec2_cpuutilization_average".into(),
            timestamp: at_seconds(1_700_000_000),
            value: 12.5,
            labels: vec![
                ("region".into(), "us-east-1".into()),
                ("dimension_InstanceId".into(), "i-0abc".into()),
                ("empty".into(), String::new()),
            ],
            unit: Some("Percent".into()),
        };

        let event = point.into_event().unwrap();

        assert_eq!(event.timestamp.0, 1_700_000_000_000_000);
        assert_eq!(
            event.fields[METRIC_NAME_FIELD],
            "aws_ec2_cpuutilization_average"
        );
        assert_eq!(event.fields[METRIC_KIND_FIELD], "gauge");
        assert_eq!(event.fields[METRIC_UNIT_FIELD], "Percent");
        assert_eq!(event.fields["value"], 12.5);
        assert_eq!(event.fields["dimension_InstanceId"], "i-0abc");
        assert!(!event.fields.contains_key("empty"));
    }

    #[test]
    fn a_value_that_is_no_number_is_left_out() {
        let point = MetricPoint {
            name: "n".into(),
            timestamp: at_seconds(0),
            value: f64::NAN,
            labels: Vec::new(),
            unit: None,
        };

        assert!(point.into_event().is_none());
    }

    #[test]
    fn a_list_is_an_array_or_a_separated_string() {
        assert_eq!(list(&json!({"m": ["a", " b ", ""]}), "m"), ["a", "b"]);
        assert_eq!(list(&json!({"m": "a, b\nc"}), "m"), ["a", "b", "c"]);
        assert!(list(&json!({}), "m").is_empty());
    }
}
