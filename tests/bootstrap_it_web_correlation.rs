// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `/api/v1/web/correlation/:from/:to?ctx=` 冒烟。
//!
//! happy: trace→log + ctx 含 trace_id → 返 200 + filters 含 trace_id
//! sad:   ctx 非合法 base64 → 400

mod common;

use base64::engine::{Engine, general_purpose::URL_SAFE_NO_PAD};
use serde_json::Value;

#[tokio::test]
async fn correlation_trace_to_log_emits_trace_id_filter() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let payload = serde_json::json!({ "trace_id": "abc123", "services": ["api"] });
    let ctx = URL_SAFE_NO_PAD.encode(serde_json::to_string(&payload).unwrap());
    let resp = s
        .client
        .get(format!(
            "{}/api/v1/web/correlation/trace/log?ctx={ctx}",
            s.base_url
        ))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "got {}", resp.status());
    let body: Value = resp.json().await.unwrap();
    let filters = body.get("filters").and_then(Value::as_array).unwrap();
    let has_trace_id = filters
        .iter()
        .any(|f| f["field"] == "trace_id" && f["value"] == "abc123");
    assert!(has_trace_id, "expected trace_id filter in {body}");
}

#[tokio::test]
async fn correlation_invalid_ctx_returns_400() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let resp = s
        .client
        .get(format!(
            "{}/api/v1/web/correlation/trace/log?ctx=$$not-base64$$",
            s.base_url
        ))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 400);
}

#[tokio::test]
async fn correlation_unknown_pair_falls_back_to_empty_filters() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();
    let ctx = URL_SAFE_NO_PAD.encode(b"{}");
    let resp = s
        .client
        .get(format!(
            "{}/api/v1/web/correlation/zzz/yyy?ctx={ctx}",
            s.base_url
        ))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    let filters = body.get("filters").and_then(Value::as_array).unwrap();
    assert!(filters.is_empty(), "unknown pair must fall back to []");
}
