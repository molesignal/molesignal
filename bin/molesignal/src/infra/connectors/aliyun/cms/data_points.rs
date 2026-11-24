// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! CloudMonitor's `Datapoints` as metric points.
//!
//! `Datapoints` is a JSON text holding an array of records such as
//! `{"timestamp":1548777660000,"userId":"1208…","instanceId":"i-abc","Average":9.92,"Maximum":9.92}`:
//! the time in milliseconds, the account, the dimensions as text and each statistic as a number.

use kernel::{Error, Result};
use serde_json::{Map, Value};

use super::settings::Settings;
use crate::infra::connectors::cloud_metrics::{
    MetricPoint, Window, at_seconds,
    naming::{dimension_label, metric_name},
};

/// The points of one metric in `datapoints` that fall in `window`, one per statistic the settings
/// name and the record holds.
pub(super) fn points(
    settings: &Settings,
    metric: &str,
    datapoints: &Value,
    window: &Window,
) -> Result<Vec<MetricPoint>> {
    let records = records(datapoints)?;
    let period = settings.schedule.period_secs;

    let names: Vec<(&str, String)> = settings
        .statistics
        .iter()
        .map(|statistic| {
            let name = metric_name(
                "aliyun",
                settings.short_namespace(),
                metric,
                Some(statistic),
            );

            (statistic.as_str(), name)
        })
        .collect();

    let mut points = Vec::new();

    for record in &records {
        let Some(seconds) = record
            .get("timestamp")
            .and_then(number)
            .map(|ms| ms as i64 / 1000)
        else {
            continue;
        };

        if seconds < window.start || seconds >= window.end {
            continue;
        }

        let start = window.start + (seconds - window.start) / period * period;
        let labels = labels(settings, record);

        for (statistic, name) in &names {
            let Some(value) = record.get(*statistic).and_then(number) else {
                continue;
            };

            points.push(MetricPoint {
                name: name.clone(),
                timestamp: at_seconds(start),
                value,
                labels: labels.clone(),
                unit: None,
            });
        }
    }

    Ok(points)
}

/// The records of `Datapoints`, given as its JSON text or, should the API change, as the array.
fn records(datapoints: &Value) -> Result<Vec<Map<String, Value>>> {
    let array = match datapoints {
        Value::Null => return Ok(Vec::new()),
        Value::String(text) if text.trim().is_empty() => return Ok(Vec::new()),
        Value::String(text) => serde_json::from_str::<Value>(text)
            .map_err(|e| Error::unavailable(format!("cloudmonitor Datapoints is not JSON: {e}")))?,
        other => other.clone(),
    };

    match array {
        Value::Array(items) => Ok(items
            .into_iter()
            .filter_map(|item| match item {
                Value::Object(record) => Some(record),
                _ => None,
            })
            .collect()),
        Value::Null => Ok(Vec::new()),
        _ => Err(Error::unavailable(
            "cloudmonitor Datapoints is not an array",
        )),
    }
}

/// `region`, `user_id` and every other text field of the record as `dimension_<key>`.
fn labels(settings: &Settings, record: &Map<String, Value>) -> Vec<(String, String)> {
    let mut labels = vec![("region".to_string(), settings.region.clone())];

    for (key, value) in record {
        let Some(value) = value.as_str() else {
            continue;
        };

        if key == "userId" {
            labels.push(("user_id".to_string(), value.to_string()));
        } else {
            labels.push((dimension_label(key), value.to_string()));
        }
    }

    labels
}

/// A number, or a number written as text.
fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn settings(statistics: &str) -> Settings {
        Settings::parse(&json!({
            "accessKey": "id",
            "secretKey": "secret",
            "region": "cn-hangzhou",
            "namespace": "acs_ecs_dashboard",
            "metrics": ["CPUUtilization"],
            "statistics": statistics,
        }))
        .unwrap()
    }

    fn window() -> Window {
        Window {
            start: 1_548_777_600,
            end: 1_548_777_600 + 600,
            more: false,
            skipped: false,
        }
    }

    /// Records shaped as in the DescribeMetricList reference.
    fn datapoints() -> Value {
        json!(
            "[{\"timestamp\":1548777600000,\"userId\":\"120886317861****\",\"instanceId\":\"i-abc\",\"Minimum\":9.92,\"Average\":9.92,\"Maximum\":9.92},\
              {\"timestamp\":1548777900000,\"userId\":\"120886317861****\",\"instanceId\":\"i-abc\",\"Minimum\":1.5,\"Average\":2.5,\"Maximum\":3.5},\
              {\"timestamp\":1548778200000,\"userId\":\"120886317861****\",\"instanceId\":\"i-abc\",\"Average\":7}]"
        )
    }

    #[test]
    fn each_statistic_of_each_period_is_one_point() {
        let points = points(
            &settings("Average,Maximum"),
            "CPUUtilization",
            &datapoints(),
            &window(),
        )
        .unwrap();

        // The third record starts at the window's end and belongs to the next pull.
        assert_eq!(points.len(), 4);
        assert_eq!(
            points[0].name,
            "aliyun_ecs_dashboard_cpuutilization_average"
        );
        assert_eq!(
            points[1].name,
            "aliyun_ecs_dashboard_cpuutilization_maximum"
        );
        assert_eq!(points[0].timestamp, at_seconds(1_548_777_600));
        assert_eq!(points[2].timestamp, at_seconds(1_548_777_900));
        assert_eq!(points[3].value, 3.5);

        let labels = &points[0].labels;

        assert!(labels.contains(&("region".into(), "cn-hangzhou".into())));
        assert!(labels.contains(&("user_id".into(), "120886317861****".into())));
        assert!(labels.contains(&("dimension_instanceId".into(), "i-abc".into())));
        assert!(!labels.iter().any(|(name, _)| name.contains("Minimum")));
    }

    #[test]
    fn a_statistic_the_record_lacks_is_left_out() {
        let points = points(&settings("Sum"), "CPUUtilization", &datapoints(), &window()).unwrap();

        assert!(points.is_empty());
    }

    #[test]
    fn no_datapoints_is_no_points_and_garbage_is_an_error() {
        let settings = settings("Average");

        assert!(
            points(&settings, "m", &Value::Null, &window())
                .unwrap()
                .is_empty()
        );
        assert!(
            points(&settings, "m", &json!("[]"), &window())
                .unwrap()
                .is_empty()
        );
        assert!(points(&settings, "m", &json!("{oops"), &window()).is_err());
    }
}
