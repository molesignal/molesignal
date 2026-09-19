// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `/api/v1/web/search` 端点冒烟。
//!
//! happy: q=arbitrary → 200 + items（可能为空，迁移就位即可）
//! sad:   q="" → handler 短路返 200 + items=[]（⌘K 面板未输入时即会打本端点）。
//!        q 超长 → 400，在打 pg_trgm 的 6 表 UNION 之前就拒掉。
//!
//! 需要 pg_trgm extension：默认 testcontainers postgres image 自带可
//! `CREATE EXTENSION` 与 GIN 索引由初始 schema 启动期自动创建。

mod common;

use serde_json::Value;

#[tokio::test]
async fn web_search_returns_items_array() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let resp = s
        .client
        .get(format!("{}/api/v1/web/search?q=acme&limit=10", s.base_url))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();

    assert!(
        resp.status().is_success(),
        "expected 200 got {}",
        resp.status()
    );
    let body: Value = resp.json().await.unwrap();
    let items = body
        .get("items")
        .and_then(Value::as_array)
        .expect("items array present");
    // 迁移就位 + 无数据 → items 至少是个数组（可能命中默认 org "acme" 几个字符的近似）
    let _ = items.len();
}

#[tokio::test]
async fn web_search_empty_query_returns_empty_items() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let resp = s
        .client
        .get(format!("{}/api/v1/web/search?q=&limit=5", s.base_url))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    let items = body
        .get("items")
        .and_then(Value::as_array)
        .expect("items array");
    assert!(items.is_empty(), "empty q should short-circuit to []");
}

#[tokio::test]
async fn web_search_rejects_overlong_query() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    // 257 字符：上限是 256。必须在进 pg_trgm 的 6 表 UNION 之前被拒。
    let long_q = "x".repeat(257);
    let resp = s
        .client
        .get(format!(
            "{}/api/v1/web/search?q={long_q}&limit=5",
            s.base_url
        ))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        400,
        "overlong q must be rejected before hitting the trigram UNION"
    );
}

#[tokio::test]
async fn web_search_respects_kind_filter() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let resp = s
        .client
        .get(format!(
            "{}/api/v1/web/search?q=test&types=stream,dashboard",
            s.base_url
        ))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    let items = body.get("items").and_then(Value::as_array).unwrap();
    // 任何返回项的 kind 都应在过滤集合内
    for item in items {
        let kind = item.get("kind").and_then(Value::as_str).unwrap();
        assert!(
            kind == "stream" || kind == "dashboard",
            "kind {kind} leaked"
        );
    }
}
