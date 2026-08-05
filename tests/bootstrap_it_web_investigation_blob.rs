// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `/api/v1/web/investigation/blob` create + fetch + size cap。
//!
//! happy: POST 小 payload → GET 拿回原值
//! sad:   POST > 64 KiB → 400
//! 隔离: 跨 org（另起一个 token？此处仅复用 root org，校验"另一个 blob_id 404"）

mod common;

use serde_json::{Value, json};

#[tokio::test]
async fn blob_create_and_fetch_roundtrip() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let payload = json!({ "frames": [{ "kind": "trace", "id": "t1" }] });
    let resp = s
        .client
        .post(format!("{}/api/v1/web/investigation/blob", s.base_url))
        .header(hk, &hv)
        .json(&payload)
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "create failed: {}",
        resp.status()
    );
    let body: Value = resp.json().await.unwrap();
    let blob_id = body
        .get("blob_id")
        .and_then(Value::as_str)
        .expect("blob_id")
        .to_string();

    let (hk, hv) = s.auth_header();
    let resp = s
        .client
        .get(format!(
            "{}/api/v1/web/investigation/blob/{blob_id}",
            s.base_url
        ))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let got: Value = resp.json().await.unwrap();
    assert_eq!(got, payload);
}

#[tokio::test]
async fn blob_oversize_rejected() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    // 256 KiB（> 旧 64 KiB 上限）现在应当成功——内容落对象存储、PG 只留指针——
    // 并能原样取回，证明上限已放宽 + S3 存取链路通。
    let medium = "m".repeat(256 * 1024);
    let medium_payload = json!({ "big": medium });
    let resp = s
        .client
        .post(format!("{}/api/v1/web/investigation/blob", s.base_url))
        .header(hk, &hv)
        .json(&medium_payload)
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "256 KiB blob should now be accepted (S3-backed), got {}",
        resp.status()
    );
    let blob_id = resp.json::<Value>().await.unwrap()["blob_id"]
        .as_str()
        .expect("blob_id")
        .to_string();
    let got: Value = s
        .client
        .get(format!(
            "{}/api/v1/web/investigation/blob/{blob_id}",
            s.base_url
        ))
        .header(hk, &hv)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        got, medium_payload,
        "256 KiB blob roundtrips via object store"
    );

    // 1.5 MiB（> 新 1 MiB 上限，仍在 axum 2 MiB body limit 之下）→ 400。
    let huge = "x".repeat(1_500 * 1024);
    let resp = s
        .client
        .post(format!("{}/api/v1/web/investigation/blob", s.base_url))
        .header(hk, &hv)
        .json(&json!({ "big": huge }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 400, "oversize must 400");
}

#[tokio::test]
async fn blob_unknown_id_returns_404() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let resp = s
        .client
        .get(format!(
            "{}/api/v1/web/investigation/blob/nonexistent-id-xxx",
            s.base_url
        ))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 404);
}
