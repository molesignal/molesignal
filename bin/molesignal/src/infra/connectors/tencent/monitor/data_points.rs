// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `GetMonitorData`'s `DataPoints` as metric points.
//!
//! Each data point is one instance: its `Dimensions`, the `Timestamps` (unix seconds) that have
//! data and, aligned with them, `AvgValues`, `MaxValues` and `MinValues` for the statistics
//! `SpecifyStatistics` asked for.

use serde_json::Value;

use super::settings::Settings;
use crate::infra::connectors::cloud_metrics::{
    MetricPoint, Window, at_seconds,
    naming::{dimension_label, metric_name},
};

/// The points of `metric` in a `GetMonitorData` response that fall in `window`.
pub(super) fn points(
    settings: &Settings,
    metric: &str,
    response: &Value,
    window: &Window,
) -> Vec<MetricPoint> {
    let period = settings.schedule.period_secs;
    let mut points = Vec::new();

    let Some(data_points) = response.get("DataPoints").and_then(Value::as_array) else {
        return points;
    };

    for data_point in data_points {
        let labels = labels(settings, data_point);

        let timestamps = data_point
            .get("Timestamps")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();

        for statistic in &settings.statistics {
            let Some(values) = data_point.get(statistic.field()).and_then(Value::as_array) else {
                continue;
            };

            let name = metric_name(
                "tencentcloud",
                settings.short_namespace(),
                metric,
                Some(statistic.as_str()),
            );

            for (timestamp, value) in timestamps.iter().zip(values) {
                let (Some(seconds), Some(value)) = (timestamp.as_f64(), value.as_f64()) else {
                    continue;
                };

                let seconds = seconds as i64;

                if seconds < window.start || seconds >= window.end {
                    continue;
                }

                points.push(MetricPoint {
                    name: name.clone(),
                    timestamp: at_seconds(
                        window.start + (seconds - window.start) / period * period,
                    ),
                    value,
                    labels: labels.clone(),
                    unit: None,
                });
            }
        }
    }

    points
}

/// `region` and each dimension as `dimension_<Name>`.
fn labels(settings: &Settings, data_point: &Value) -> Vec<(String, String)> {
    let mut labels = vec![("region".to_string(), settings.region.clone())];

    for dimension in data_point
        .get("Dimensions")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let name = dimension.get("Name").and_then(Value::as_str);
        let value = dimension.get("Value").and_then(Value::as_str);

        if let (Some(name), Some(value)) = (name, value) {
            labels.push((dimension_label(name), value.to_string()));
        }
    }

    labels
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn settings() -> Settings {
        Settings::parse(&json!({
            "accessKey": "AKIDexample",
            "secretKey": "secret",
            "region": "ap-guangzhou",
            "namespace": "QCE/CVM",
            "metrics": ["CpuUsage"],
            "statistics": "avg,max",
        }))
        .unwrap()
    }

    /// Shaped as the GetMonitorData reference's example, with the statistics it asked for.
    fn response() -> Value {
        json!({
            "StartTime": "2023-11-15T06:10:00+08:00",
            "EndTime": "2023-11-15T06:19:59+08:00",
            "Period": 300,
            "MetricName": "CpuUsage",
            "DataPoints": [{
                "Dimensions": [{"Name": "InstanceId", "Value": "ins-p2lgmb3s"}],
                "Timestamps": [1_699_999_800, 1_700_000_100, 1_700_000_400],
                "Values": [1.0, 2.0, 3.0],
                "AvgValues": [1.0, 2.0, 3.0],
                "MaxValues": [1.5, null, 3.5],
                "MinValues": null
            }],
            "RequestId": "ef9d5dd2-1f68-41c8-9b52-c1b6d1c7a8b9"
        })
    }

    #[test]
    fn each_statistic_of_each_period_is_one_point() {
        let window = Window {
            start: 1_699_999_800,
            end: 1_700_000_400,
            more: false,
            skipped: false,
        };

        let points = points(&settings(), "CpuUsage", &response(), &window);
        let names: Vec<_> = points.iter().map(|point| point.name.as_str()).collect();

        // The third period starts at the window's end; the null maximum is left out.
        assert_eq!(
            names,
            [
                "tencentcloud_cvm_cpu_usage_avg",
                "tencentcloud_cvm_cpu_usage_avg",
                "tencentcloud_cvm_cpu_usage_max"
            ]
        );
        assert_eq!(points[1].timestamp, at_seconds(1_700_000_100));
        assert_eq!(points[2].value, 1.5);
        assert_eq!(
            points[0].labels,
            [
                ("region".to_string(), "ap-guangzhou".to_string()),
                (
                    "dimension_InstanceId".to_string(),
                    "ins-p2lgmb3s".to_string()
                )
            ]
        );
    }

    #[test]
    fn a_response_without_data_points_is_no_points() {
        let window = Window {
            start: 0,
            end: 300,
            more: false,
            skipped: false,
        };

        assert!(points(&settings(), "CpuUsage", &json!({"DataPoints": []}), &window).is_empty());
        assert!(points(&settings(), "CpuUsage", &json!({}), &window).is_empty());
    }
}
