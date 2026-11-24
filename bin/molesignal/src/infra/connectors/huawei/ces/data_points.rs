// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `BatchListMetricData`'s answer as metric points.
//!
//! Each entry of `metrics` is one series: its `metric_name`, `dimensions` and `unit`, and its
//! `datapoints`, each with a `timestamp` in milliseconds and the value under the statistic's name
//! (`average`, `max`, …).

use serde_json::Value;

use super::{api::dimensions, settings::Settings};
use crate::infra::connectors::cloud_metrics::{
    MetricPoint, Window, at_seconds,
    naming::{dimension_label, metric_name},
};

/// The points of one statistic in a `BatchListMetricData` answer that fall in `window`.
pub(super) fn points(
    settings: &Settings,
    statistic: &str,
    answer: &Value,
    window: &Window,
) -> Vec<MetricPoint> {
    let period = settings.schedule.period_secs;
    let mut points = Vec::new();

    let Some(metrics) = answer.get("metrics").and_then(Value::as_array) else {
        return points;
    };

    for metric in metrics {
        let Some(metric_id) = metric.get("metric_name").and_then(Value::as_str) else {
            continue;
        };

        let name = metric_name(
            "huaweicloud",
            &settings.namespace,
            metric_id,
            Some(statistic),
        );
        let unit = metric
            .get("unit")
            .and_then(Value::as_str)
            .map(str::to_string);
        let mut labels = vec![("region".to_string(), settings.region.clone())];

        labels.extend(
            dimensions(metric)
                .into_iter()
                .map(|(name, value)| (dimension_label(&name), value)),
        );

        for data_point in metric
            .get("datapoints")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            let seconds = data_point
                .get("timestamp")
                .and_then(Value::as_f64)
                .map(|ms| ms as i64 / 1_000);

            let (Some(seconds), Some(value)) =
                (seconds, data_point.get(statistic).and_then(Value::as_f64))
            else {
                continue;
            };

            // Cloud Eye rounds `from` down to a period, so points before the window may come too.
            if seconds < window.start || seconds >= window.end {
                continue;
            }

            points.push(MetricPoint {
                name: name.clone(),
                timestamp: at_seconds(window.start + (seconds - window.start) / period * period),
                value,
                labels: labels.clone(),
                unit: unit.clone(),
            });
        }
    }

    points
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn settings() -> Settings {
        Settings::parse(&json!({
            "accessKey": "AK",
            "secretKey": "SK",
            "region": "cn-north-4",
            "projectId": "p1",
            "namespace": "SYS.ECS",
            "metrics": ["cpu_util"],
        }))
        .unwrap()
    }

    /// Shaped as the BatchListMetricData reference's example.
    fn answer() -> Value {
        json!({
            "metrics": [{
                "unit": "%",
                "datapoints": [
                    {"average": 0, "timestamp": 1_484_153_000_000_i64},
                    {"average": 1, "timestamp": 1_484_153_300_000_i64},
                    {"average": 2.3, "timestamp": 1_484_153_600_000_i64},
                    {"average": 4, "timestamp": 1_484_153_900_000_i64}
                ],
                "namespace": "SYS.ECS",
                "dimensions": [{"name": "instance_id", "value": "33328f02-3814-422e-b688-bfdba93d4050"}],
                "metric_name": "cpu_util"
            }]
        })
    }

    #[test]
    fn the_points_of_the_window_carry_name_labels_and_unit() {
        let window = Window {
            start: 1_484_153_300,
            end: 1_484_153_900,
            more: false,
            skipped: false,
        };

        let points = points(&settings(), "average", &answer(), &window);

        assert_eq!(points.len(), 2);
        assert_eq!(points[0].name, "huaweicloud_sys_ecs_cpu_util_average");
        assert_eq!(points[0].timestamp, at_seconds(1_484_153_300));
        assert_eq!(points[1].value, 2.3);
        assert_eq!(points[0].unit.as_deref(), Some("%"));
        assert_eq!(
            points[0].labels,
            [
                ("region".to_string(), "cn-north-4".to_string()),
                (
                    "dimension_instance_id".to_string(),
                    "33328f02-3814-422e-b688-bfdba93d4050".to_string()
                )
            ]
        );
    }

    #[test]
    fn another_statistic_reads_its_own_field() {
        let window = Window {
            start: 1_484_153_000,
            end: 1_484_154_200,
            more: false,
            skipped: false,
        };

        assert!(points(&settings(), "max", &answer(), &window).is_empty());
    }
}
