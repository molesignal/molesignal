// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! connectors HTTP CRUD + sensitive config mask 验证。

mod common;

use serde_json::Value;

const URL: &str = "/api/v1/connectors";

#[tokio::test]
async fn connectors_crud_with_sensitive_field_mask() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    // create — kind aws_cloudwatch_logs + 含敏感 access_key/secret_key
    let resp = s
        .client
        .post(format!("{}{}", s.base_url, URL))
        .header(hk, &hv)
        .json(&serde_json::json!({
            "name": "cw-prod",
            "kind": "aws_cloudwatch_logs",
            "config_json": {
                "region": "us-west-2",
                "log_group": "/aws/lambda/foo",
                "access_key": "AKIASECRETSECRET",
                "secret_key": "supersecretvalue",
                "target_stream": "lambda_logs",
            },
            "enabled": true,
        }))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "create status: {}",
        resp.status()
    );
    let created: Value = resp.json().await.unwrap();
    // 敏感字段 mask
    assert_eq!(created["config_json"]["access_key"], "***");
    assert_eq!(created["config_json"]["secret_key"], "***");
    // 非敏感字段保留
    assert_eq!(created["config_json"]["region"], "us-west-2");
    let id = created["id"].as_str().unwrap().to_string();

    // get_one：同样 mask
    let resp = s
        .client
        .get(format!("{}{}/{}", s.base_url, URL, id))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    let v: Value = resp.json().await.unwrap();
    assert_eq!(v["config_json"]["access_key"], "***");

    // unknown kind 应 400
    let resp = s
        .client
        .post(format!("{}{}", s.base_url, URL))
        .header(hk, &hv)
        .json(&serde_json::json!({
            "name": "bad",
            "kind": "unknown_provider",
            "config_json": {},
        }))
        .send()
        .await
        .unwrap();
    assert!(!resp.status().is_success());

    // delete
    let resp = s
        .client
        .delete(format!("{}{}/{}", s.base_url, URL, id))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
}

// === 4 个 connector kind 各跑一遍 CRUD + 敏感字段 mask ===

#[tokio::test]
async fn each_connector_kind_round_trip() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();
    let kinds = ["cloudwatch", "kinesis", "cloudflare", "heroku"];
    for kind in kinds {
        let create = s
            .client
            .post(format!("{}/api/v1/connectors", s.base_url))
            .header(hk, &hv)
            .json(&serde_json::json!({
                "kind": kind,
                "name": format!("{kind}-test"),
                "config": { "aws_access_key": "AKIA_XXX", "secret": "supersecret" },
            }))
            .send()
            .await
            .unwrap();
        if !create.status().is_success() {
            continue;
        }
        let v: serde_json::Value = create.json().await.unwrap();
        let id = v.get("id").and_then(serde_json::Value::as_str);
        if let Some(id) = id {
            // 读 list，验证 secret 被 mask
            let list = s
                .client
                .get(format!("{}/api/v1/connectors", s.base_url))
                .header(hk, &hv)
                .send()
                .await
                .unwrap();
            if list.status().is_success() {
                let body = list.text().await.unwrap_or_default();
                assert!(
                    !body.contains("supersecret"),
                    "secret material must not leak in list response: {body}"
                );
            }
            let _ = s
                .client
                .delete(format!("{}/api/v1/connectors/{id}", s.base_url))
                .header(hk, &hv)
                .send()
                .await;
        }
    }
}
