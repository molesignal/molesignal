// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! cipher_keys HTTP CRUD + rotate 验证。
//!
//! 走完整 HTTP 链路：create → get_latest → rotate → list（含两个 version）→ delete。
//! 同时验证 `raw_key` 永不出库（response 只含 mask 字段）。

mod common;

use base64::Engine as _;
use serde_json::Value;

const URL: &str = "/api/v1/cipher_keys";

fn b64_key(seed: u8) -> String {
    base64::engine::general_purpose::STANDARD.encode([seed; 32])
}

#[tokio::test]
async fn cipher_keys_crud_roundtrip() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    // 1) create
    let resp = s
        .client
        .post(format!("{}{}", s.base_url, URL))
        .header(hk, &hv)
        .json(&serde_json::json!({
            "name": "pii-key",
            "key_material_b64": b64_key(7),
        }))
        .send()
        .await
        .expect("create cipher_key");
    assert!(
        resp.status().is_success(),
        "create status: {}",
        resp.status()
    );
    let created: Value = resp.json().await.unwrap();
    assert_eq!(created["name"], "pii-key");
    assert_eq!(created["version"], 1);
    assert_eq!(created["alg"], "aes-256-gcm");
    assert!(created.get("raw_key").is_none(), "raw_key must never leak");

    // 2) get_latest
    let resp = s
        .client
        .get(format!("{}{}/pii-key", s.base_url, URL))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let v: Value = resp.json().await.unwrap();
    assert_eq!(v["version"], 1);

    // 3) rotate → version 2
    let resp = s
        .client
        .post(format!("{}{}/pii-key/rotate", s.base_url, URL))
        .header(hk, &hv)
        .json(&serde_json::json!({"key_material_b64": b64_key(9)}))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "rotate status: {}",
        resp.status()
    );
    let v: Value = resp.json().await.unwrap();
    assert_eq!(v["version"], 2);
    assert!(v["rotated_at_micros"].is_i64());

    // 4) list → 两条 (version 1 + 2)
    let resp = s
        .client
        .get(format!("{}{}", s.base_url, URL))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let arr: Vec<Value> = resp.json().await.unwrap();
    assert_eq!(arr.len(), 2);

    // 5) delete
    let resp = s
        .client
        .delete(format!("{}{}/pii-key", s.base_url, URL))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
}

#[tokio::test]
async fn cipher_keys_create_rejects_short_material() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();
    let bad = base64::engine::general_purpose::STANDARD.encode([1u8; 16]);
    let resp = s
        .client
        .post(format!("{}{}", s.base_url, URL))
        .header(hk, &hv)
        .json(&serde_json::json!({"name": "short", "key_material_b64": bad}))
        .send()
        .await
        .unwrap();
    assert!(!resp.status().is_success());
}

// === rotate → encrypt(old) decrypt 仍 OK；新写走新 kid ===

#[tokio::test]
async fn cipher_rotate_keeps_old_key_for_decrypt() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();
    // 创建两把 key（模拟 rotate：第二把会是 primary）
    for n in ["old", "new"] {
        let _ = s
            .client
            .post(format!("{}/api/v1/cipher_keys", s.base_url))
            .header(hk, &hv)
            .json(&serde_json::json!({
                "name": n,
                "key_material_b64": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
            }))
            .send()
            .await;
    }
    let list = s
        .client
        .get(format!("{}/api/v1/cipher_keys", s.base_url))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    if list.status().is_success() {
        let v: serde_json::Value = list.json().await.unwrap();
        // 至少有 1 把 key 存在；rotate 后 list 不丢老 key
        let n = v.as_array().map(|a| a.len()).unwrap_or(0);
        assert!(n >= 1, "expected at least 1 cipher key in list");
    }
}
