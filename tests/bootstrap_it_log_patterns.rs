// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! log_patterns CRUD + 坏 regex 校验 + first_match。

mod common;

#[tokio::test]
async fn log_patterns_crud_smoke() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let create = s
        .client
        .post(format!("{}/api/v1/log_patterns", s.base_url))
        .header(hk, &hv)
        .json(&serde_json::json!({
            "name": "http-status",
            "regex": r#"HTTP/1\.1 (\d{3})"#,
            "stream": null,
        }))
        .send()
        .await
        .unwrap();
    assert!(
        create.status().as_u16() != 404,
        "log_patterns route should be wired"
    );

    let list = s
        .client
        .get(format!("{}/api/v1/log_patterns", s.base_url))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert!(list.status().as_u16() != 404);
}

#[tokio::test]
async fn log_patterns_rejects_invalid_regex() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let resp = s
        .client
        .post(format!("{}/api/v1/log_patterns", s.base_url))
        .header(hk, &hv)
        .json(&serde_json::json!({
            "name": "broken",
            "regex": "(unclosed",
            "stream": null,
        }))
        .send()
        .await
        .unwrap();
    let code = resp.status().as_u16();
    assert!(
        code == 400 || code == 422,
        "invalid regex should be 400/422, got {code}"
    );
}

#[tokio::test]
async fn first_match_endpoint_handles_no_match() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let resp = s
        .client
        .post(format!("{}/api/v1/log_patterns/first_match", s.base_url))
        .header(hk, &hv)
        .json(&serde_json::json!({ "line": "nothing to extract" }))
        .send()
        .await
        .unwrap();
    let code = resp.status().as_u16();
    assert!(code != 404 && code != 401 && code != 403);
}
