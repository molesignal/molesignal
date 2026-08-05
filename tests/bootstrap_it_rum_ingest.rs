// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! RUM ingest 各 4 端点冒烟。
//!
//! POST sessions / actions / errors / replay 各一条 → 端点响应 2xx；
//! sad path: 体不合 schema → 4xx。

mod common;

#[tokio::test]
async fn rum_sessions_endpoint_accepts_valid_post() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let body = serde_json::json!({
        "application": "test-mobile",
        "session_id": "s-1",
        "user_id": "u-1",
        "started_at": 1_700_000_000_000_i64
    });
    let resp = s
        .client
        .post(format!("{}/api/v1/rum/sessions", s.base_url))
        .header(hk, &hv)
        .json(&body)
        .send()
        .await
        .unwrap();
    let code = resp.status().as_u16();
    assert!(
        (200..300).contains(&code),
        "RUM session ingest failed: {code}"
    );
}

#[tokio::test]
async fn rum_errors_endpoint_handles_payload() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let body = serde_json::json!({
        "application": "test-mobile",
        "session_id": "s-1",
        "ts": 1_700_000_000_000_i64,
        "message": "TypeError: undefined is not a function"
    });
    let resp = s
        .client
        .post(format!("{}/api/v1/rum/errors", s.base_url))
        .header(hk, &hv)
        .json(&body)
        .send()
        .await
        .unwrap();
    let code = resp.status().as_u16();
    assert!(
        (200..300).contains(&code),
        "RUM error ingest failed: {code}"
    );
}

#[tokio::test]
async fn rum_replay_endpoint_accepts_json_segment() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let body = serde_json::json!({
        "application": "test-mobile",
        "session_id": "s-1",
        "seq": 1,
        "events": [{
            "type": 2,
            "timestamp": 1_700_000_000_000_u64,
            "data": {"node": {"type": 0}}
        }]
    });
    let resp = s
        .client
        .post(format!("{}/api/v1/rum/replay", s.base_url))
        .header(hk, &hv)
        .json(&body)
        .send()
        .await
        .unwrap();
    let code = resp.status().as_u16();
    assert!(
        (200..300).contains(&code),
        "RUM replay ingest failed: {code}"
    );
}

#[tokio::test]
async fn rum_actions_rejects_bad_body() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let resp = s
        .client
        .post(format!("{}/api/v1/rum/actions", s.base_url))
        .header(hk, &hv)
        .body("not-json")
        .send()
        .await
        .unwrap();
    let code = resp.status().as_u16();
    assert!(
        code == 400 || code == 415 || code == 422,
        "bad payload should be 4xx, got {code}"
    );
}

#[tokio::test]
async fn legacy_double_version_path_is_not_registered() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let resp = s
        .client
        .post(format!("{}/api/v1/rum/v1/errors", s.base_url))
        .header(hk, &hv)
        .json(&serde_json::json!([]))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status().as_u16(), 404);
}
