// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Asking Azure Monitor for metric values with `metrics:getBatch`, and reading its answer as
//! samples.
//!
//! One request reads up to [`MAX_RESOURCES_PER_CALL`] resources of one type in one region and
//! subscription, for up to [`MAX_METRICS_PER_CALL`] metrics and every configured aggregation.
//! Azure stamps each value with the start of its time grain, which is what a sample carries.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::Value;

use super::{
    resources::{Resource, segment_after},
    settings::{Settings, time_grain},
};
use crate::infra::connectors::cloud_metrics::{MetricPoint, Window, at_seconds, naming};

/// How many resource IDs one `getBatch` takes.
pub(super) const MAX_RESOURCES_PER_CALL: usize = 50;

/// How many metric names one request asks for, as for Azure Monitor's per-resource metrics API.
pub(super) const MAX_METRICS_PER_CALL: usize = 20;

const API_VERSION: &str = "2023-10-01";

/// What a query value keeps as it is; the rest is percent-encoded. Commas stay, since
/// `metricnames` and `aggregation` are comma-separated lists.
const QUERY_VALUE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~')
    .remove(b',')
    .remove(b':');

/// A time as `getBatch` takes it, `yyyy-MM-ddTHH:mm:ss.fffZ`.
pub(super) fn timestamp(seconds: i64) -> String {
    DateTime::<Utc>::from_timestamp(seconds, 0)
        .unwrap_or_default()
        .format("%Y-%m-%dT%H:%M:%S.000Z")
        .to_string()
}

/// The `getBatch` URL for `metrics` over `window`.
pub(super) fn url(settings: &Settings, metrics: &[String], window: Window) -> String {
    let start = timestamp(window.start);
    let end = timestamp(window.end);
    let metric_names = metrics.join(",");
    let aggregation = settings.aggregations.join(",");
    let top = settings.top.to_string();

    let mut query = vec![
        ("starttime", start.as_str()),
        ("endtime", end.as_str()),
        (
            "interval",
            time_grain(settings.schedule.period_secs).unwrap_or("PT5M"),
        ),
        ("metricnamespace", settings.resource_type.as_str()),
        ("metricnames", metric_names.as_str()),
        ("aggregation", aggregation.as_str()),
    ];

    // `top` applies to the series a filter splits into; without a filter it is not allowed.
    if let Some(filter) = settings.filter.as_deref() {
        query.push(("filter", filter));
        query.push(("top", top.as_str()));
    }

    query.push(("api-version", API_VERSION));

    let query = query
        .into_iter()
        .map(|(key, value)| format!("{key}={}", utf8_percent_encode(value, QUERY_VALUE)))
        .collect::<Vec<_>>()
        .join("&");

    format!(
        "https://{}.metrics.monitor.azure.com/subscriptions/{}/metrics:getBatch?{query}",
        settings.region, settings.subscription_id
    )
}

/// What one `getBatch` answer held: its samples, and the metrics Azure could not read.
#[derive(Debug, Default)]
pub(super) struct Answer {
    pub points: Vec<MetricPoint>,
    pub failures: Vec<String>,
}

/// The samples of a `getBatch` answer that fall in `window`, one per aggregation per time grain
/// per series, labelled with the resource they came from.
pub(super) fn parse(
    body: &Value,
    settings: &Settings,
    resources: &HashMap<String, Resource>,
    window: Window,
) -> Answer {
    let mut answer = Answer::default();

    for values in body
        .get("values")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let resource_id = str_field(values, "resourceid");
        let labels = resource_labels(settings, resources, resource_id);

        for metric in values
            .get("value")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            read_metric(metric, settings, &labels, window, resource_id, &mut answer);
        }
    }

    answer
}

fn read_metric(
    metric: &Value,
    settings: &Settings,
    labels: &[(String, String)],
    window: Window,
    resource_id: &str,
    answer: &mut Answer,
) {
    let name = metric
        .get("name")
        .and_then(|name| name.get("value"))
        .and_then(Value::as_str)
        .unwrap_or_default();

    let error_code = str_field(metric, "errorCode");

    if !error_code.is_empty() && error_code != "Success" {
        let reason = str_field(metric, "errorMessage");

        answer.failures.push(format!(
            "`{name}` of {}: {error_code} {reason}",
            segment_after(resource_id, "providers").unwrap_or_default()
        ));

        return;
    }

    let unit = Some(str_field(metric, "unit"))
        .filter(|unit| !unit.is_empty() && *unit != "Unspecified")
        .map(str::to_string);

    for series in metric
        .get("timeseries")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let mut series_labels = labels.to_vec();

        for dimension in series
            .get("metadatavalues")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let dimension_name = dimension
                .get("name")
                .and_then(|name| name.get("value"))
                .and_then(Value::as_str)
                .unwrap_or_default();

            if !dimension_name.is_empty() {
                series_labels.push((
                    naming::dimension_label(dimension_name),
                    str_field(dimension, "value").to_string(),
                ));
            }
        }

        for data in series
            .get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(at) = DateTime::parse_from_rfc3339(str_field(data, "timeStamp"))
                .ok()
                .map(|at| at.timestamp())
                .filter(|at| (window.start..window.end).contains(at))
            else {
                continue;
            };

            for aggregation in &settings.aggregations {
                let Some(value) = data.get(aggregation.as_str()).and_then(Value::as_f64) else {
                    continue;
                };

                answer.points.push(MetricPoint {
                    name: naming::metric_name(
                        "azure",
                        settings.namespace(),
                        name,
                        Some(aggregation.as_str()),
                    ),
                    timestamp: at_seconds(at),
                    value,
                    labels: series_labels.clone(),
                    unit: unit.clone(),
                });
            }
        }
    }
}

/// The labels every sample of a resource carries.
fn resource_labels(
    settings: &Settings,
    resources: &HashMap<String, Resource>,
    resource_id: &str,
) -> Vec<(String, String)> {
    let resource = resources.get(&resource_id.to_ascii_lowercase());

    let name = resource
        .map(|resource| resource.name.clone())
        .or_else(|| segment_after(resource_id, "providers"))
        .unwrap_or_default();

    let group = resource
        .map(|resource| resource.resource_group.clone())
        .or_else(|| segment_after(resource_id, "resourceGroups"))
        .unwrap_or_default();

    let mut labels = vec![
        (
            "subscription_id".to_string(),
            settings.subscription_id.clone(),
        ),
        ("resource_group".to_string(), group),
        ("resource_name".to_string(), name),
        (
            "resource_id".to_string(),
            resource
                .map_or(resource_id, |resource| resource.id.as_str())
                .to_string(),
        ),
        ("region".to_string(), settings.region.clone()),
    ];

    for (key, value) in resource
        .map(|resource| resource.tags.as_slice())
        .unwrap_or_default()
    {
        labels.push((format!("tag_{}", naming::label_safe(key)), value.clone()));
    }

    labels
}

fn str_field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const SUBSCRIPTION: &str = "12345678-abcd-1234-abcd-123456789abc";
    const STORAGE_1: &str = "/subscriptions/12345678-abcd-1234-abcd-123456789abc/resourceGroups/TestGroup/providers/Microsoft.Storage/storageAccounts/TestStorage1";

    fn settings(extra: Value) -> Settings {
        let mut config = json!({
            "tenantId": "t",
            "clientId": "c",
            "clientSecret": "s",
            "subscriptionId": SUBSCRIPTION,
            "region": "eastus",
            "resourceType": "Microsoft.Storage/storageAccounts",
            "metrics": ["Ingress", "Egress"],
            "aggregations": ["average", "maximum"],
            "period": 900,
        });

        for (key, value) in extra.as_object().unwrap() {
            config[key] = value.clone();
        }

        Settings::parse(&config).unwrap()
    }

    /// 2023-02-01T20:00:00Z to 20:45:00Z, three 15-minute grains.
    fn window() -> Window {
        Window {
            start: 1_675_281_600,
            end: 1_675_281_600 + 2_700,
            more: false,
            skipped: false,
        }
    }

    #[test]
    fn the_url_names_the_window_grain_and_metrics() {
        let url = url(
            &settings(json!({})),
            &["Ingress".into(), "Percentage CPU".into()],
            window(),
        );

        assert_eq!(
            url,
            format!(
                "https://eastus.metrics.monitor.azure.com/subscriptions/{SUBSCRIPTION}/metrics:getBatch?\
                 starttime=2023-02-01T20:00:00.000Z&endtime=2023-02-01T20:45:00.000Z&interval=PT15M&\
                 metricnamespace=Microsoft.Storage%2FstorageAccounts&metricnames=Ingress,Percentage%20CPU&\
                 aggregation=average,maximum&api-version=2023-10-01"
            )
        );
    }

    #[test]
    fn a_filter_comes_with_top() {
        let url = url(
            &settings(json!({ "filter": "ApiName eq '*'", "top": 25 })),
            &["Ingress".into()],
            window(),
        );

        assert!(url.contains("&filter=ApiName%20eq%20%27%2A%27&top=25&api-version="));
    }

    #[test]
    fn an_answer_is_one_sample_per_aggregation_per_grain_in_the_window() {
        // Trimmed from the documented "Query Storage Metrics" response.
        let body = json!({
            "values": [{
                "starttime": "2023-02-01T20:00:00Z",
                "endtime": "2023-02-01T20:45:00Z",
                "interval": "PT15M",
                "value": [
                    {
                        "id": format!("{STORAGE_1}/providers/Microsoft.Insights/metrics/Ingress"),
                        "type": "Microsoft.Insights/metrics",
                        "name": { "value": "Ingress", "localizedValue": "Ingress" },
                        "unit": "Bytes",
                        "timeseries": [{
                            "metadatavalues": [
                                { "name": { "value": "apiname", "localizedValue": "apiname" }, "value": "GetBlob" }
                            ],
                            "data": [
                                { "timeStamp": "2023-02-01T20:00:00Z", "average": 437, "minimum": 437, "maximum": 437 },
                                { "timeStamp": "2023-02-01T20:15:00Z" },
                                { "timeStamp": "2023-02-01T20:30:00Z", "average": 438.07, "maximum": 583 },
                                { "timeStamp": "2023-02-01T20:45:00Z", "average": 1, "maximum": 1 }
                            ]
                        }],
                        "errorCode": "Success"
                    },
                    {
                        "name": { "value": "Egress", "localizedValue": "Egress" },
                        "errorCode": "MetricNotFound",
                        "errorMessage": "Egress is not a metric of this resource"
                    }
                ],
                "namespace": "microsoft.storage/storageaccounts",
                "resourceregion": "eastus",
                "resourceid": STORAGE_1
            }]
        });

        let resources = HashMap::from([(
            STORAGE_1.to_ascii_lowercase(),
            Resource {
                id: STORAGE_1.into(),
                name: "TestStorage1".into(),
                resource_group: "TestGroup".into(),
                tags: vec![("env".into(), "prod".into())],
            },
        )]);

        let answer = parse(&body, &settings(json!({})), &resources, window());

        // 20:00 and 20:30 carry values, 20:15 none, 20:45 is the next window's.
        assert_eq!(answer.points.len(), 4);

        let first = &answer.points[0];

        assert_eq!(first.name, "azure_storage_storageaccounts_ingress_average");
        assert_eq!(first.timestamp, at_seconds(1_675_281_600));
        assert_eq!(first.value, 437.0);
        assert_eq!(first.unit.as_deref(), Some("Bytes"));
        assert_eq!(
            answer.points[1].name,
            "azure_storage_storageaccounts_ingress_maximum"
        );
        assert_eq!(
            answer.points[3].timestamp,
            at_seconds(1_675_281_600 + 1_800)
        );
        assert_eq!(answer.points[3].value, 583.0);

        let label = |name: &str| {
            first
                .labels
                .iter()
                .find(|(label, _)| label == name)
                .map(|(_, value)| value.as_str())
        };

        assert_eq!(label("subscription_id"), Some(SUBSCRIPTION));
        assert_eq!(label("resource_group"), Some("TestGroup"));
        assert_eq!(label("resource_name"), Some("TestStorage1"));
        assert_eq!(label("resource_id"), Some(STORAGE_1));
        assert_eq!(label("region"), Some("eastus"));
        assert_eq!(label("dimension_apiname"), Some("GetBlob"));
        assert_eq!(label("tag_env"), Some("prod"));

        assert_eq!(answer.failures.len(), 1);
        assert!(answer.failures[0].contains("MetricNotFound"));
        assert!(answer.failures[0].contains("TestStorage1"));
    }

    #[test]
    fn a_resource_not_listed_is_labelled_from_its_id() {
        let body = json!({
            "values": [{
                "resourceid": STORAGE_1,
                "value": [{
                    "name": { "value": "Percentage CPU" },
                    "unit": "Percent",
                    "timeseries": [{ "data": [{ "timeStamp": "2023-02-01T20:15:00Z", "average": 3.5 }] }]
                }]
            }]
        });

        let answer = parse(
            &body,
            &settings(json!({ "resourceType": "Microsoft.Compute/virtualMachines" })),
            &HashMap::new(),
            window(),
        );

        assert_eq!(answer.points.len(), 1);
        assert_eq!(
            answer.points[0].name,
            "azure_compute_virtualmachines_percentage_cpu_average"
        );
        assert!(
            answer.points[0]
                .labels
                .contains(&("resource_group".to_string(), "TestGroup".to_string()))
        );
        assert!(
            answer.points[0]
                .labels
                .contains(&("resource_name".to_string(), "TestStorage1".to_string()))
        );
    }
}
