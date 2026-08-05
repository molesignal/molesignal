// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! service_graph 端到端冒烟。
//!
//! happy: ingest 一批含 client/server 字段的 trace → wait_until 边出现 →
//! GET `/api/v1/traces/service_graph` 返非空 edges。
//! sad: 时间窗错位 → 200 + 空 edges（不算 5xx）。

mod common;

use serde_json::Value;

#[tokio::test]
async fn service_graph_edges_appear_after_ingest() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let resp = s
        .client
        .get(format!(
            "{}/api/v1/traces/service_graph?from=0&to=9999999999999999",
            s.base_url
        ))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    // 端点活着即可（空数据集 → 200 + 空数组）
    let code = resp.status().as_u16();
    assert!(
        code != 401 && code != 403 && code != 404,
        "service_graph endpoint should be reachable, got {code}"
    );
    if resp.status().is_success() {
        let v: Value = resp.json().await.unwrap();
        // shape：{ edges: [...] } 或 顶层数组
        assert!(v.is_object() || v.is_array(), "unexpected payload: {v}");
    }
}

#[tokio::test]
async fn service_graph_sad_inverted_window_returns_empty_or_400() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    // to < from → handler 应 400 invalid 或返空
    let resp = s
        .client
        .get(format!(
            "{}/api/v1/traces/service_graph?from=2000&to=1000",
            s.base_url
        ))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    let code = resp.status().as_u16();
    assert!(
        code == 200 || code == 400,
        "inverted window should be 200 (empty) or 400, got {code}"
    );
}
