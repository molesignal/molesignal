// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Dashboard Engine 创建与读取端到端。

mod common;

use common::{TestServer, skip_unless_enabled};
use serde_json::{Value, json};

#[tokio::test]
async fn create_then_get_preserves_dashboard_model() {
    if skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }
    let s = TestServer::start().await;

    let resp = s
        .client
        .post(format!("{}/api/v1/dashboards", s.base_url))
        .header(s.auth_header().0, s.auth_header().1)
        .json(&json!({
            "model": {
                "engine": "molesignal-dashboard",
                "schemaVersion": 2,
                "uid": "abcd1234",
                "title": "Latency overview",
                "tags": ["prod", "api"],
                "editable": true,
                "defaultDashboard": false,
                "timeSettings": {
                    "defaultFrom": "now-6h",
                    "defaultTo": "now",
                    "timezone": "browser"
                },
                "refreshSettings": {
                    "enabled": true,
                    "mode": "interval",
                    "defaultInterval": "30s",
                    "allowedIntervals": ["off", "5s", "30s", "1m"]
                },
                "variables": [],
                "annotations": [],
                "links": [],
                "layout": {
                    "type": "grid",
                    "columns": 24,
                    "rowHeight": 8,
                    "gap": 8
                },
                "elements": [{
                    "kind": "panel",
                    "id": "latency",
                    "title": "p99 latency",
                    "gridPos": { "x": 0, "y": 0, "w": 24, "h": 8 },
                    "queryOptions": {},
                    "queries": [{
                        "refId": "A",
                        "enabled": true,
                        "dataSourceType": "metrics",
                        "query": {
                            "language": "promql",
                            "expression": "histogram_quantile(0.99, sum by (le) (rate(http_request_duration_seconds_bucket[5m])))"
                        },
                        "format": "time_series"
                    }],
                    "transformations": [],
                    "visualization": {
                        "type": "time_series",
                        "schemaVersion": 1,
                        "options": {}
                    },
                    "fieldConfig": { "unit": "seconds" },
                    "overrides": [],
                    "links": []
                }]
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "create status");
    let created: serde_json::Value = resp.json().await.unwrap();
    let id = created["id"].as_str().expect("created id").to_string();
    assert_eq!(created["uid"], "abcd1234");
    assert_eq!(created["title"], "Latency overview");
    assert_eq!(created["org_id"], s.root_org_id.0);

    let resp = s
        .client
        .get(format!("{}/api/v1/dashboards/{id}", s.base_url))
        .header(s.auth_header().0, s.auth_header().1)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let fetched: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(fetched["uid"], "abcd1234");
    assert_eq!(fetched["title"], "Latency overview");
    assert_eq!(fetched["model"]["engine"], "molesignal-dashboard");
    assert_eq!(fetched["model"]["schemaVersion"], 2);
    assert_eq!(fetched["model"]["elements"][0]["id"], "latency");
}

#[tokio::test]
async fn invalid_nested_dashboard_returns_structured_issues() {
    if skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }
    let s = TestServer::start().await;
    let response = s
        .client
        .post(format!("{}/api/v1/dashboards", s.base_url))
        .header(s.auth_header().0, s.auth_header().1)
        .json(&json!({
            "model": {
                "engine": "molesignal-dashboard",
                "schemaVersion": 2,
                "title": "Invalid duplicate IDs",
                "tags": [],
                "editable": true,
                "defaultDashboard": false,
                "timeSettings": {
                    "defaultFrom": "now-1h",
                    "defaultTo": "now",
                    "timezone": "browser"
                },
                "refreshSettings": {
                    "enabled": false,
                    "mode": "off",
                    "allowedIntervals": ["off"]
                },
                "variables": [],
                "annotations": [],
                "links": [],
                "layout": {
                    "type": "grid",
                    "columns": 24,
                    "rowHeight": 8,
                    "gap": 8
                },
                "elements": [
                    {
                        "kind": "text",
                        "id": "duplicate",
                        "title": "One",
                        "gridPos": { "x": 0, "y": 0, "w": 12, "h": 8 },
                        "content": "one",
                        "mode": "plain"
                    },
                    {
                        "kind": "text",
                        "id": "duplicate",
                        "title": "Two",
                        "gridPos": { "x": 12, "y": 0, "w": 12, "h": 8 },
                        "content": "two",
                        "mode": "plain"
                    }
                ]
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["error"], "validation_failed");
    assert!(body["issues"].as_array().is_some_and(|issues| {
        issues
            .iter()
            .any(|issue| issue["code"] == "DUPLICATE_ELEMENT_ID")
    }));
}
