// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `/api/v1/web/topology` 端点冒烟。
//!
//! happy: 注入若干 `service_graph_edges` → 查询窗口聚合返 nodes / edges
//! sad:   `to < from` → 400 invalid

mod common;

use molesignal::{
    infra::traces::EdgeSnapshot,
    shared::{ids::Id, time::TimestampMicros},
};
use serde_json::Value;

#[tokio::test]
async fn topology_empty_window_returns_empty_graph() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let from = "2020-01-01T00:00:00Z";
    let to = "2020-01-01T00:01:00Z";
    let resp = s
        .client
        .get(format!(
            "{}/api/v1/web/topology?from={from}&to={to}",
            s.base_url
        ))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "got {}", resp.status());
    let body: Value = resp.json().await.unwrap();
    let nodes = body.get("nodes").and_then(Value::as_array).unwrap();
    let edges = body.get("edges").and_then(Value::as_array).unwrap();
    assert!(nodes.is_empty());
    assert!(edges.is_empty());
}

#[tokio::test]
async fn topology_invalid_window_rejected() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let resp = s
        .client
        .get(format!(
            "{}/api/v1/web/topology?from=2024-06-01T00:00:00Z&to=2024-05-01T00:00:00Z",
            s.base_url
        ))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    // to < from → handler 内 Error::invalid → 400
    assert_eq!(resp.status().as_u16(), 400);
}

#[tokio::test]
async fn topology_aggregates_seeded_edges() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    // 直接通过 service_graph_repo 注入一条 web→api 的边
    let bucket_at = chrono::Utc::now()
        .timestamp_micros()
        .saturating_sub(60_000_000);
    s.state
        .telemetry
        .service_graph
        .insert_many(&[EdgeSnapshot {
            id: Id::new(),
            org_id: s.root_org_id.clone(),
            client_service: "web".into(),
            server_service: "api".into(),
            bucket_at_micros: bucket_at,
            request_count: 120,
            error_count: 6,
            p50_us: Some(2_000),
            p95_us: Some(7_000),
            p99_us: Some(11_000),
        }])
        .await
        .unwrap();

    // 注意：to_rfc3339() 在 UTC 下输出 `+00:00`，URL query 里 `+` 会被解码成空格 →
    // 后端 parse_from_rfc3339 失败 400。用 `Z` 后缀的 ISO8601 规避这个 URL 坑。
    let fmt = |dt: chrono::DateTime<chrono::Utc>| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let now = chrono::Utc::now();
    let from = fmt(now - chrono::Duration::minutes(10));
    let to = fmt(now);
    let resp = s
        .client
        .get(format!(
            "{}/api/v1/web/topology?from={from}&to={to}",
            s.base_url
        ))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "got {}", resp.status());
    let body: Value = resp.json().await.unwrap();
    let edges = body.get("edges").and_then(Value::as_array).unwrap();
    assert!(
        edges
            .iter()
            .any(|e| e["source"] == "web" && e["target"] == "api"),
        "expected web→api edge in {body}"
    );
    // 用 TimestampMicros 仅做 unused 警告防御
    let _ = TimestampMicros::now();
}
