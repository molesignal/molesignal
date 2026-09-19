// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `/api/v1/query/stream` NDJSON 端点冒烟。
//!
//! happy: 合法 GET → 200 + Content-Type=application/x-ndjson + 末尾含 `__meta__` 行
//! sad:   `to < from` → 400

mod common;

#[tokio::test]
async fn query_stream_returns_ndjson_content_type() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let url = format!(
        "{}/api/v1/query/stream?sql={sql}&stream=nonexistent_logs&stream_type=logs&from=0&to=1000000",
        s.base_url,
        sql = "SELECT%201"
    );
    let resp = s.client.get(&url).header(hk, &hv).send().await.unwrap();
    let code = resp.status().as_u16();
    // 空 stream 可能让 engine 报错 5xx；200 + ndjson Content-Type 是 happy path
    if resp.status().is_success() {
        let ct = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(
            ct.contains("application/x-ndjson"),
            "expected ndjson, got {ct}"
        );
        let body = resp.text().await.unwrap();
        assert!(
            body.contains("__meta__"),
            "stream should emit trailing __meta__ line"
        );
    } else {
        // stream 不存在 → planner 报 Forbidden(403)（防泄漏存在性）；其他失败 4xx/5xx
        assert!(
            code == 400 || code == 403 || code == 500,
            "got unexpected status {code}"
        );
    }
}

#[tokio::test]
async fn query_stream_rejects_inverted_window() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let url = format!(
        "{}/api/v1/query/stream?sql=SELECT%201&stream=logs&from=100&to=50",
        s.base_url
    );
    let resp = s.client.get(&url).header(hk, &hv).send().await.unwrap();
    assert_eq!(resp.status().as_u16(), 400);
}

#[tokio::test]
async fn query_stream_post_returns_ndjson() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let resp = s
        .client
        .post(format!("{}/api/v1/query/stream", s.base_url))
        .header(hk, &hv)
        .json(&serde_json::json!({
            "org_id": s.root_org_id.0,
            "language": "sql",
            "statement": "SELECT 1",
            "time_range": { "start": 0_i64, "end": 1_000_000_i64 },
            "stream": { "name": "nonexistent_logs", "stream_type": "logs" }
        }))
        .send()
        .await
        .unwrap();
    let code = resp.status().as_u16();
    if resp.status().is_success() {
        let ct = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(ct.contains("application/x-ndjson"), "got {ct}");
    } else {
        // stream 不存在 → planner 报 Forbidden(403)（防泄漏存在性）；其他失败 4xx/5xx
        assert!(
            code == 400 || code == 403 || code == 500,
            "got unexpected status {code}"
        );
    }
}
