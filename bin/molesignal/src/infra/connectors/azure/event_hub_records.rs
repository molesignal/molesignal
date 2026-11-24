// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What Azure Monitor sends to an event hub: each message is `{"records": [...]}`.
//!
//! Diagnostic settings, Log Analytics data export and data collection rules all export to Event
//! Hubs this way, and a Kafka source with `messageFormat: azure_records` reads the hub through its Kafka
//! endpoint. Each record becomes its own event:
//!
//! - a resource log or activity log record (`time`, `resourceId`, `category`, `operationName`,
//!   `properties`, ...) is a log event at its `time`, with its fields as they are;
//! - a platform metric record (`metricName` with `count`, `total`, `minimum`, `maximum`,
//!   `average`) is a metric sample per statistic, named as the Azure Monitor metric pull names them
//!   (`azure_compute_virtualmachines_percentage_cpu_average`). Data collection rules write the
//!   statistics capitalized and as strings, with a `dimension` object; both shapes are read.
//!
//! See <https://learn.microsoft.com/en-us/azure/azure-monitor/essentials/stream-monitoring-data-event-hubs>
//! and <https://learn.microsoft.com/en-us/azure/azure-monitor/data-collection/metrics-export-create>.

use chrono::DateTime;
use domain::intake::RawEvent;
use kernel::time::TimestampMicros;
use serde_json::{Map, Value};

use crate::infra::connectors::cloud_metrics::{self, MetricPoint, naming};

/// The statistics a metric record may carry.
const STATISTICS: [&str; 5] = ["average", "minimum", "maximum", "total", "count"];

/// The events of messages read as Azure exports: log events and metric samples. A message without
/// `records` is kept as it is.
pub fn unpack(messages: Vec<RawEvent>) -> (Vec<RawEvent>, Vec<RawEvent>) {
    let mut logs = Vec::new();
    let mut metrics = Vec::new();

    for mut message in messages {
        let Some(Value::Array(records)) = message.fields.remove("records") else {
            logs.push(message);
            continue;
        };

        // What the Kafka source adds (topic, partition, offset, key) stays with each record.
        let shared: Map<String, Value> = message
            .fields
            .iter()
            .filter(|(key, _)| key.starts_with("kafka_"))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();

        for record in records {
            let Value::Object(record) = record else {
                continue;
            };

            if field(&record, "metricName").is_some() {
                metrics.extend(cloud_metrics::events(metric_points(&record)));
                continue;
            }

            let timestamp = field(&record, "time")
                .and_then(Value::as_str)
                .and_then(rfc3339)
                .unwrap_or(message.timestamp);

            let mut fields = record;

            for (key, value) in &shared {
                fields.entry(key.clone()).or_insert_with(|| value.clone());
            }

            fields
                .entry("source")
                .or_insert_with(|| Value::String("azure".into()));

            logs.push(RawEvent { timestamp, fields });
        }
    }

    (logs, metrics)
}

/// A field by its name in any case: data collection rules capitalize what diagnostic settings do
/// not.
fn field<'a>(record: &'a Map<String, Value>, name: &str) -> Option<&'a Value> {
    record.get(name).or_else(|| {
        record
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value)
    })
}

fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

fn rfc3339(text: &str) -> Option<TimestampMicros> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|time| TimestampMicros(time.timestamp_micros()))
}

/// The samples of a metric record, one per statistic it has.
fn metric_points(record: &Map<String, Value>) -> Vec<MetricPoint> {
    let text = |name: &str| {
        field(record, name)
            .and_then(Value::as_str)
            .unwrap_or_default()
    };
    let Some(timestamp) = rfc3339(text("time")) else {
        return Vec::new();
    };

    let resource = Resource::of(text("resourceId"));
    let mut labels = resource.labels();

    if let Some(Value::Object(dimensions)) = field(record, "dimension") {
        for (name, value) in dimensions {
            if let Some(value) = value.as_str() {
                labels.push((naming::dimension_label(name), value.to_string()));
            }
        }
    }

    STATISTICS
        .iter()
        .filter_map(|statistic| {
            let value = field(record, statistic).and_then(number)?;

            Some(MetricPoint {
                name: naming::metric_name(
                    "azure",
                    &resource.kind,
                    text("metricName"),
                    Some(statistic),
                ),
                timestamp,
                value,
                labels: labels.clone(),
                unit: Some(text("unit").to_string()).filter(|unit| !unit.is_empty()),
            })
        })
        .collect()
}

/// What an Azure resource id says:
/// `/subscriptions/<id>/resourceGroups/<group>/providers/Microsoft.Compute/virtualMachines/<name>`.
#[derive(Debug, Default, PartialEq)]
struct Resource {
    id: String,
    subscription_id: String,
    resource_group: String,
    /// The resource type without its `Microsoft.` prefix: `Compute/virtualMachines`.
    kind: String,
    name: String,
}

impl Resource {
    fn of(id: &str) -> Self {
        let parts: Vec<&str> = id.trim_matches('/').split('/').collect();
        let after = |key: &str| {
            parts
                .iter()
                .position(|part| part.eq_ignore_ascii_case(key))
                .and_then(|at| parts.get(at + 1))
                .map(|part| part.to_string())
                .unwrap_or_default()
        };

        let kind = parts
            .iter()
            .position(|part| part.eq_ignore_ascii_case("providers"))
            .and_then(|at| Some((parts.get(at + 1)?, parts.get(at + 2)?)))
            .map(|(namespace, kind)| {
                let namespace = namespace
                    .split_once('.')
                    .filter(|(vendor, _)| vendor.eq_ignore_ascii_case("microsoft"))
                    .map(|(_, rest)| rest)
                    .unwrap_or(namespace);

                format!("{namespace}/{kind}")
            })
            .unwrap_or_default();

        Self {
            id: id.to_string(),
            subscription_id: after("subscriptions"),
            resource_group: after("resourceGroups"),
            kind,
            name: parts
                .last()
                .map(|part| part.to_string())
                .unwrap_or_default(),
        }
    }

    fn labels(&self) -> Vec<(String, String)> {
        [
            ("subscription_id", &self.subscription_id),
            ("resource_group", &self.resource_group),
            ("resource_name", &self.name),
            ("resource_id", &self.id),
        ]
        .into_iter()
        .filter(|(_, value)| !value.is_empty())
        .map(|(label, value)| (label.to_string(), value.clone()))
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use domain::metrics::METRIC_NAME_FIELD;
    use serde_json::json;

    use super::*;

    fn message(body: Value) -> RawEvent {
        let Value::Object(mut fields) = body else {
            unreachable!()
        };

        fields.insert("kafka_topic".into(), json!("insights-logs"));

        RawEvent {
            timestamp: TimestampMicros(1),
            fields,
        }
    }

    const VM: &str =
        "/SUBSCRIPTIONS/S-1/RESOURCEGROUPS/RG-1/PROVIDERS/MICROSOFT.COMPUTE/VIRTUALMACHINES/VM-1";

    #[test]
    fn each_log_record_is_an_event_at_its_own_time() {
        let (logs, metrics) = unpack(vec![message(json!({"records": [
            {"time": "2026-11-24T10:00:00Z", "resourceId": VM, "category": "Administrative",
             "operationName": "Microsoft.Compute/virtualMachines/write", "level": "Information"},
            {"time": "2026-11-24T10:00:01Z", "resourceId": VM, "category": "Policy"}
        ]}))]);

        assert!(metrics.is_empty());
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0].timestamp, rfc3339("2026-11-24T10:00:00Z").unwrap());
        assert_eq!(logs[0].fields["category"], "Administrative");
        assert_eq!(logs[0].fields["kafka_topic"], "insights-logs");
        assert_eq!(logs[0].fields["source"], "azure");
    }

    #[test]
    fn a_metric_record_is_a_sample_per_statistic() {
        let (logs, metrics) = unpack(vec![message(json!({"records": [
            {"count": 2, "total": 10.0, "minimum": 4.0, "maximum": 6.0, "average": 5.0,
             "resourceId": VM, "time": "2026-11-24T10:00:00Z", "metricName": "Percentage CPU",
             "timeGrain": "PT1M"}
        ]}))]);

        assert!(logs.is_empty());
        assert_eq!(metrics.len(), 5);

        let average = metrics
            .iter()
            .find(|sample| {
                sample.fields[METRIC_NAME_FIELD]
                    == "azure_compute_virtualmachines_percentage_cpu_average"
            })
            .unwrap();

        assert_eq!(average.fields["value"], 5.0);
        assert_eq!(average.fields["resource_group"], "RG-1");
        assert_eq!(average.fields["resource_name"], "VM-1");
        assert_eq!(average.fields["subscription_id"], "S-1");
    }

    #[test]
    fn a_data_collection_rule_record_with_strings_and_dimensions_is_read_too() {
        let (_, metrics) = unpack(vec![message(json!({"records": [
            {"Average": "1", "Count": "1", "Maximum": "1", "Minimum": "1", "Total": "1",
             "resourceId": "/subscriptions/s/resourceGroups/rg/providers/Microsoft.KeyVault/vaults/kv",
             "time": "2024-08-22T13:43:00.0000000Z", "unit": "Count", "metricName": "ServiceApiHit",
             "timeGrain": "PT1M", "dimension": {"ActivityName": "keycreate"}}
        ]}))]);

        assert_eq!(metrics.len(), 5);
        assert_eq!(
            metrics[0].fields[METRIC_NAME_FIELD],
            "azure_keyvault_vaults_service_api_hit_average"
        );
        assert_eq!(metrics[0].fields["dimension_ActivityName"], "keycreate");
        assert_eq!(metrics[0].fields["metric_unit"], "Count");
    }

    #[test]
    fn a_message_without_records_is_kept_as_it_is() {
        let (logs, _) = unpack(vec![message(json!({"msg": "hi"}))]);

        assert_eq!(logs[0].fields["msg"], "hi");
    }
}
