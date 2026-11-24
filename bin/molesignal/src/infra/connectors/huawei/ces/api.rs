// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Cloud Eye API v1 calls: `ListMetrics` to find the series of a metric and
//! `BatchListMetricData` to read their aggregates, both signed with the AK/SK signature.

use kernel::{Error, Result, time::TimestampMicros};
use serde_json::{Value, json};
use signals::http_trace::{self, HttpTarget};

use super::{settings::Settings, sign};
use crate::infra::connectors::cloud_metrics::refusal;

const CONTENT_TYPE: &str = "application/json";

/// How many series one `ListMetrics` page lists at most.
pub(super) const LIST_LIMIT: usize = 1_000;

/// One series of a metric: its dimensions, as `ListMetrics` gave them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Series {
    pub metric: String,
    pub dimensions: Vec<(String, String)>,
}

/// One page of `ListMetrics`.
#[derive(Debug, Default)]
pub(super) struct ListPage {
    pub series: Vec<Series>,
    /// The `start` of the next page; `None` on the last one.
    pub marker: Option<String>,
}

/// Lists the series of `metric`, `limit` at a time from `start`.
pub(super) async fn list_metrics(
    client: &reqwest::Client,
    settings: &Settings,
    metric: &str,
    limit: usize,
    start: Option<&str>,
) -> Result<ListPage> {
    let mut params = vec![
        ("namespace", settings.namespace.clone()),
        ("metric_name", metric.to_string()),
        ("limit", limit.to_string()),
    ];

    if let Some(start) = start {
        params.push(("start", start.to_string()));
    }

    let path = format!("/V1.0/{}/metrics", settings.project_id);
    let body = send(client, settings, "GET", &path, &params, None, "ListMetrics").await?;

    Ok(list_page(&body, limit))
}

/// The series of a `ListMetrics` answer and where the next page starts.
pub(super) fn list_page(body: &Value, limit: usize) -> ListPage {
    let series: Vec<Series> = body
        .get("metrics")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|metric| {
            Some(Series {
                metric: metric.get("metric_name")?.as_str()?.to_string(),
                dimensions: dimensions(metric),
            })
        })
        .collect();

    let marker = body
        .get("meta_data")
        .and_then(|meta| meta.get("marker"))
        .and_then(Value::as_str)
        .filter(|marker| !marker.is_empty() && series.len() >= limit)
        .map(str::to_string);

    ListPage { series, marker }
}

/// The `BatchListMetricData` body for `series` over `[start, end)` and one statistic.
pub(super) fn batch_request(
    settings: &Settings,
    series: &[Series],
    statistic: &str,
    start: i64,
    end: i64,
) -> Value {
    let metrics: Vec<Value> = series
        .iter()
        .map(|series| {
            let dimensions: Vec<Value> = series
                .dimensions
                .iter()
                .map(|(name, value)| json!({ "name": name, "value": value }))
                .collect();

            json!({
                "namespace": settings.namespace,
                "metric_name": series.metric,
                "dimensions": dimensions,
            })
        })
        .collect();

    json!({
        "metrics": metrics,
        "from": start * 1_000,
        "to": end * 1_000,
        "period": settings.schedule.period_secs.to_string(),
        "filter": statistic,
    })
}

/// Reads one batch; the answer's `metrics` hold the data points.
pub(super) async fn batch_list_metric_data(
    client: &reqwest::Client,
    settings: &Settings,
    request: &Value,
) -> Result<Value> {
    let path = format!("/V1.0/{}/batch-query-metric-data", settings.project_id);

    send(
        client,
        settings,
        "POST",
        &path,
        &[],
        Some(request),
        "BatchListMetricData",
    )
    .await
}

/// The `name`/`value` pairs of a metric's `dimensions`.
pub(super) fn dimensions(metric: &Value) -> Vec<(String, String)> {
    metric
        .get("dimensions")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|dimension| {
            Some((
                dimension.get("name")?.as_str()?.to_string(),
                dimension.get("value")?.as_str()?.to_string(),
            ))
        })
        .collect()
}

async fn send(
    client: &reqwest::Client,
    settings: &Settings,
    method: &str,
    path: &str,
    params: &[(&str, String)],
    body: Option<&Value>,
    action: &str,
) -> Result<Value> {
    let host = settings.host();
    let query = sign::canonical_query(params);
    let payload = body.map(Value::to_string).unwrap_or_default();
    let date = sign::sdk_date(TimestampMicros::now().0 / 1_000_000);

    let authorization = sign::authorization(
        &settings.access_key,
        &settings.secret_key,
        &sign::Request {
            method,
            host: &host,
            path,
            query: &query,
            content_type: CONTENT_TYPE,
            body: payload.as_bytes(),
            date: &date,
        },
    );

    let url = if query.is_empty() {
        format!("https://{host}{path}")
    } else {
        format!("https://{host}{path}?{query}")
    };

    let request = if method == "POST" {
        client.post(url).body(payload)
    } else {
        client.get(url)
    };

    let request = request
        .header("content-type", CONTENT_TYPE)
        .header("x-sdk-date", date)
        .header("authorization", authorization);

    let what = format!("cloud eye {action}");

    let response = http_trace::send(client, request, HttpTarget::ThirdParty)
        .await
        .map_err(|e| Error::unavailable(format!("{what} request: {e}")))?;

    let status = response.status().as_u16();

    let text = response
        .text()
        .await
        .map_err(|e| Error::unavailable(format!("{what} body: {e}")))?;

    if !(200..300).contains(&status) {
        return Err(refusal(&what, status, &text));
    }

    serde_json::from_str(&text)
        .map_err(|e| Error::unavailable(format!("{what} answered with no JSON: {e}")))
}

#[cfg(test)]
mod tests {
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

    /// Shaped as the ListMetrics reference's example.
    #[test]
    fn a_list_page_has_the_series_and_the_next_marker() {
        let body = json!({
            "metrics": [{
                "namespace": "SYS.ECS",
                "dimensions": [{"name": "instance_id", "value": "d9112af5-6913-4f3b-bd0a-3f96711e004d"}],
                "metric_name": "cpu_util",
                "unit": "%"
            }],
            "meta_data": {
                "count": 1,
                "marker": "SYS.ECS.cpu_util.instance_id:d9112af5-6913-4f3b-bd0a-3f96711e004d",
                "total": 7
            }
        });

        let full = list_page(&body, 1);

        assert_eq!(
            full.series,
            [Series {
                metric: "cpu_util".into(),
                dimensions: vec![(
                    "instance_id".into(),
                    "d9112af5-6913-4f3b-bd0a-3f96711e004d".into()
                )],
            }]
        );
        assert_eq!(
            full.marker.as_deref(),
            Some("SYS.ECS.cpu_util.instance_id:d9112af5-6913-4f3b-bd0a-3f96711e004d")
        );

        // A page shorter than the limit is the last one.
        assert_eq!(list_page(&body, LIST_LIMIT).marker, None);
    }

    #[test]
    fn a_batch_names_each_series_with_one_filter() {
        let series = vec![Series {
            metric: "cpu_util".into(),
            dimensions: vec![("instance_id".into(), "i-1".into())],
        }];

        assert_eq!(
            batch_request(&settings(), &series, "max", 1_484_153_300, 1_484_153_900),
            json!({
                "metrics": [{
                    "namespace": "SYS.ECS",
                    "metric_name": "cpu_util",
                    "dimensions": [{"name": "instance_id", "value": "i-1"}]
                }],
                "from": 1_484_153_300_000_i64,
                "to": 1_484_153_900_000_i64,
                "period": "300",
                "filter": "max"
            })
        );
    }
}
