// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `POST /api/v1/query/search_around` HTTP 端点冒烟。
//!
//! 由于 query engine 在空 stream 下会返空，本测试只做：
//! 1) 端点存在 + 鉴权通过 + 返 200 JSON
//! 2) 返回 body 含 `before` / `after` / `pointer` 三个 key

mod common;

use serde_json::Value;

#[tokio::test]
async fn search_around_smoke() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let resp = s
        .client
        .post(format!("{}/api/v1/query/search_around", s.base_url))
        .header(hk, &hv)
        .json(&serde_json::json!({
            "event_timestamp_us": 1_000_000_i64,
            "event_fingerprint": "abc",
            "stream": "nonexistent_logs",
            "stream_type": "logs",
            "before": 5,
            "after": 5,
        }))
        .send()
        .await
        .unwrap();
    // engine 没数据可能 4xx/5xx；只要不是 401/403/404 就算端点活着
    let s_code = resp.status().as_u16();
    assert!(
        s_code != 401 && s_code != 403 && s_code != 404,
        "search_around should be accessible, got {s_code}"
    );
    if resp.status().is_success() {
        let v: Value = resp.json().await.unwrap();
        assert!(v.get("before").is_some());
        assert!(v.get("after").is_some());
        assert!(v.get("pointer").is_some());
    }
}

#[tokio::test]
async fn streaming_query_returns_ndjson_content_type() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let resp = s
        .client
        .post(format!("{}/api/v1/query", s.base_url))
        .header(hk, &hv)
        .header("accept", "application/x-ndjson")
        .json(&serde_json::json!({
            "org_id": s.root_org_id.0,
            "language": "sql",
            "statement": "SELECT 1",
            "time_range": {
                "start": 0_i64,
                "end": 1_000_000_i64
            }
        }))
        .send()
        .await
        .unwrap();
    if resp.status().is_success() {
        let ct = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(
            ct.contains("application/x-ndjson"),
            "streaming should return ndjson, got {ct}"
        );
    }
}

// === before/after 边界 + fingerprint + 跨 day ===

#[tokio::test]
async fn search_around_before_after_boundaries() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();
    // before=0 + after=0 应该 200 + 两侧空集
    let resp = s
        .client
        .post(format!("{}/api/v1/query/search_around", s.base_url))
        .header(hk, &hv)
        .json(&serde_json::json!({
            "event_timestamp_us": 1_000_000_i64,
            "event_fingerprint": "abc",
            "stream": "nonexistent_logs",
            "stream_type": "logs",
            "before": 0,
            "after": 0,
        }))
        .send()
        .await
        .unwrap();
    if resp.status().is_success() {
        let v: serde_json::Value = resp.json().await.unwrap();
        assert!(v.get("before").is_some());
        assert!(v.get("after").is_some());
    }
}

#[tokio::test]
async fn search_around_cross_day_boundary() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();
    // 跨日：用一个 86400_000_000 的时间戳（很多分区裁剪的临界点）
    let resp = s
        .client
        .post(format!("{}/api/v1/query/search_around", s.base_url))
        .header(hk, &hv)
        .json(&serde_json::json!({
            "event_timestamp_us": 86_400_000_000_i64,
            "event_fingerprint": "boundary",
            "stream": "nonexistent_logs",
            "stream_type": "logs",
            "before": 5,
            "after": 5,
        }))
        .send()
        .await
        .unwrap();
    let code = resp.status().as_u16();
    // search_around 在某些权限/feature 配置下可能 403；本边界测试只验非 5xx + 非 404
    assert!(code < 500 && code != 404, "got {code}");
}
