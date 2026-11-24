// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! CloudWatch Metric Streams records, in either output format, as metric samples.
//!
//! A stream sends one aggregate per metric per minute: its sample count, sum, minimum and maximum,
//! and any percentiles the stream asks for. Both formats are read into a [`Datum`] first:
//!
//! - [`json`]: a JSON object per line;
//! - [`otel`]: OpenTelemetry 1.0.0 or 0.7.0, size-delimited `ExportMetricsServiceRequest`s.
//!
//! A datum becomes one gauge sample per statistic, named the way yet-another-cloudwatch-exporter
//! names them, so PromQL written against it works unchanged:
//! `aws_<namespace>_<metric>_<statistic>`, as in `aws_ec2_cpuutilization_average`. The labels are
//! `account_id`, `region` and one `dimension_<Name>` per dimension. See
//! <https://docs.aws.amazon.com/AmazonCloudWatch/latest/monitoring/CloudWatch-metric-streams-formats.html>.

use domain::{
    intake::RawEvent,
    metrics::{METRIC_KIND_FIELD, METRIC_NAME_FIELD, METRIC_UNIT_FIELD},
};
use kernel::time::TimestampMicros;
use serde_json::{Map, Value};

use super::Delivery;
use crate::infra::connectors::cloud_metrics::naming;

pub mod json;
pub mod otel;

/// One metric's aggregate over one period.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Datum {
    pub account_id: Option<String>,
    pub region: Option<String>,
    /// As CloudWatch names it: `AWS/EC2`, or a custom namespace.
    pub namespace: String,
    pub metric_name: String,
    pub dimensions: Vec<(String, String)>,
    pub timestamp: TimestampMicros,
    /// CloudWatch's unit, or the UCUM unit of the OpenTelemetry formats; `None` is left out.
    pub unit: Option<String>,
    pub count: Option<f64>,
    pub sum: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// Further statistics by their exported suffix: `p99`, `p99_9`, `tm_25_75`.
    pub extra: Vec<(String, f64)>,
}

impl Datum {
    /// The samples of this datum, one per statistic it has.
    pub fn samples(&self, delivery: &Delivery) -> Vec<RawEvent> {
        let average = match (self.sum, self.count) {
            (Some(sum), Some(count)) if count > 0.0 => Some(sum / count),
            _ => None,
        };

        let statistics = [
            ("average", average),
            ("sum", self.sum),
            ("sample_count", self.count),
            ("minimum", self.min),
            ("maximum", self.max),
        ];

        let labels = self.labels(delivery);

        statistics
            .into_iter()
            .filter_map(|(statistic, value)| value.map(|value| (statistic.to_string(), value)))
            .chain(self.extra.iter().cloned())
            .filter(|(_, value)| value.is_finite())
            .map(|(statistic, value)| {
                let mut fields = labels.clone();
                fields.insert(
                    METRIC_NAME_FIELD.into(),
                    Value::String(metric_name(&self.namespace, &self.metric_name, &statistic)),
                );
                fields.insert("value".into(), Value::from(value));

                RawEvent {
                    timestamp: self.timestamp,
                    fields,
                }
            })
            .collect()
    }

    fn labels(&self, delivery: &Delivery) -> Map<String, Value> {
        let mut fields = Map::new();
        fields.insert(METRIC_KIND_FIELD.into(), Value::String("gauge".into()));

        if let Some(unit) = self
            .unit
            .as_deref()
            .filter(|unit| !unit.is_empty() && *unit != "None")
        {
            fields.insert(METRIC_UNIT_FIELD.into(), Value::String(unit.into()));
        }

        let account = self.account_id.as_ref().or(delivery.account_id.as_ref());
        let region = self.region.as_ref().or(delivery.region.as_ref());

        for (label, value) in [("account_id", account), ("region", region)] {
            if let Some(value) = value.filter(|value| !value.is_empty()) {
                fields.insert(label.into(), Value::String(value.clone()));
            }
        }

        for (name, value) in &self.dimensions {
            fields.insert(naming::dimension_label(name), Value::String(value.clone()));
        }

        delivery.stamp(&mut fields);
        fields
    }
}

/// `aws_<namespace>_<metric>_<statistic>`: the namespace without its `AWS/` prefix, by the rule
/// every cloud's metrics are named by ([`naming::metric_name`]).
pub fn metric_name(namespace: &str, metric: &str, statistic: &str) -> String {
    let namespace = namespace.strip_prefix("AWS/").unwrap_or(namespace);

    naming::metric_name("aws", namespace, metric, Some(statistic))
}

/// The suffix of a further statistic: `p99` → `p99`, `p99.9` → `p99_9`, `TM(25%:75%)` →
/// `tm_25_75`.
pub fn statistic_suffix(name: &str) -> String {
    naming::snake(&name.to_ascii_lowercase())
}

/// The suffix of a quantile of the OpenTelemetry formats: 0.99 → `p99`, 0.999 → `p99_9`.
pub fn quantile_suffix(quantile: f64) -> String {
    let percent = format!("{:.4}", quantile * 100.0);
    let percent = percent.trim_end_matches('0').trim_end_matches('.');

    format!("p{}", percent.replace('.', "_"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_yet_another_cloudwatch_exporter() {
        assert_eq!(
            metric_name("AWS/EC2", "CPUUtilization", "average"),
            "aws_ec2_cpuutilization_average"
        );
        assert_eq!(
            metric_name("AWS/EC2", "NetworkIn", "sum"),
            "aws_ec2_network_in_sum"
        );
        assert_eq!(
            metric_name("AWS/ApplicationELB", "HTTPCode_Target_5XX_Count", "sum"),
            "aws_applicationelb_httpcode_target_5xx_count_sum"
        );
        assert_eq!(
            metric_name("AWS/RDS", "FreeStorageSpace", "minimum"),
            "aws_rds_free_storage_space_minimum"
        );
        assert_eq!(
            metric_name("Acme/Orders", "OrdersPlaced", "sample_count"),
            "aws_acme_orders_orders_placed_sample_count"
        );
    }

    #[test]
    fn further_statistics_and_quantiles_get_safe_suffixes() {
        assert_eq!(statistic_suffix("p99"), "p99");
        assert_eq!(statistic_suffix("p99.9"), "p99_9");
        assert_eq!(statistic_suffix("TM(25%:75%)"), "tm_25_75");
        assert_eq!(quantile_suffix(0.99), "p99");
        assert_eq!(quantile_suffix(0.999), "p99_9");
        assert_eq!(quantile_suffix(0.5), "p50");
    }

    #[test]
    fn a_datum_is_one_gauge_sample_per_statistic() {
        let datum = Datum {
            account_id: Some("123456789012".into()),
            region: Some("us-east-1".into()),
            namespace: "AWS/EC2".into(),
            metric_name: "CPUUtilization".into(),
            dimensions: vec![("InstanceId".into(), "i-0abc".into())],
            timestamp: TimestampMicros(1_700_000_000_000_000),
            unit: Some("Percent".into()),
            count: Some(4.0),
            sum: Some(10.0),
            min: Some(1.0),
            max: Some(5.0),
            extra: vec![("p99".into(), 4.9)],
        };

        let samples = datum.samples(&Delivery::default());
        let value = |name: &str| {
            samples
                .iter()
                .find(|sample| sample.fields[METRIC_NAME_FIELD] == name)
                .map(|sample| sample.fields["value"].as_f64().unwrap())
        };

        assert_eq!(samples.len(), 6);
        assert_eq!(value("aws_ec2_cpuutilization_average"), Some(2.5));
        assert_eq!(value("aws_ec2_cpuutilization_sum"), Some(10.0));
        assert_eq!(value("aws_ec2_cpuutilization_sample_count"), Some(4.0));
        assert_eq!(value("aws_ec2_cpuutilization_minimum"), Some(1.0));
        assert_eq!(value("aws_ec2_cpuutilization_maximum"), Some(5.0));
        assert_eq!(value("aws_ec2_cpuutilization_p99"), Some(4.9));

        let first = &samples[0].fields;

        assert_eq!(first["dimension_InstanceId"], "i-0abc");
        assert_eq!(first["account_id"], "123456789012");
        assert_eq!(first["region"], "us-east-1");
        assert_eq!(first[METRIC_KIND_FIELD], "gauge");
        assert_eq!(first[METRIC_UNIT_FIELD], "Percent");
        assert_eq!(samples[0].timestamp.0, 1_700_000_000_000_000);
    }

    #[test]
    fn no_average_without_samples_and_no_unit_named_none() {
        let datum = Datum {
            namespace: "AWS/SQS".into(),
            metric_name: "NumberOfMessagesSent".into(),
            unit: Some("None".into()),
            count: Some(0.0),
            sum: Some(0.0),
            ..Datum::default()
        };

        let samples = datum.samples(&Delivery::default());

        assert_eq!(samples.len(), 2);
        assert!(!samples[0].fields.contains_key(METRIC_UNIT_FIELD));
    }
}
