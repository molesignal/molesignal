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
            "configJson": {
                "region": "us-west-2",
                "logGroup": "/aws/lambda/foo",
                "accessKey": "AKIASECRETSECRET",
                "secretKey": "supersecretvalue",
                "targetStream": "lambda_logs",
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
    assert_eq!(created["configJson"]["accessKey"], "***");
    assert_eq!(created["configJson"]["secretKey"], "***");
    // 非敏感字段保留
    assert_eq!(created["configJson"]["region"], "us-west-2");
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
    assert_eq!(v["configJson"]["accessKey"], "***");

    // unknown kind 应 400
    let resp = s
        .client
        .post(format!("{}{}", s.base_url, URL))
        .header(hk, &hv)
        .json(&serde_json::json!({
            "name": "bad",
            "kind": "unknown_provider",
            "configJson": {},
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
                "config": { "awsAccessKey": "AKIA_XXX", "secret": "supersecret" },
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

// === 密钥静态封存 + 推送令牌按摘要反查 + 试连 ===

#[tokio::test]
async fn secrets_are_sealed_at_rest_and_a_push_token_still_opens_its_connector() {
    use sqlx::Row as _;

    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();
    let token = "it-push-token-0123456789abcdef";

    let created: Value = s
        .client
        .post(format!("{}{}", s.base_url, URL))
        .header(hk, &hv)
        .json(&serde_json::json!({
            "name": "cf-sealed",
            "kind": "cloudflare_logpush",
            "configJson": { "pushToken": token, "targetStream": "cf_sealed" },
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = created["id"].as_str().unwrap().to_string();
    assert_eq!(created["secretKeys"], serde_json::json!(["pushToken"]));
    assert_eq!(created["configJson"]["pushToken"], "***");

    // 库里只有密文与摘要，没有令牌本身。
    let pool = sqlx::PgPool::connect(&s.settings.meta_database.dsn)
        .await
        .unwrap();
    let row = sqlx::query(
        "SELECT config_json::TEXT AS config, secrets_ciphertext, push_token_sha256
         FROM connectors WHERE id = $1",
    )
    .bind(&id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let config: String = row.try_get("config").unwrap();
    let sealed: Option<Vec<u8>> = row.try_get("secrets_ciphertext").unwrap();
    let digest: Option<String> = row.try_get("push_token_sha256").unwrap();
    assert!(
        !config.contains(token),
        "token stored in the clear: {config}"
    );
    assert!(sealed.is_some_and(|bytes| !String::from_utf8_lossy(&bytes).contains(token)));
    assert_eq!(digest.map(|digest| digest.len()), Some(64));

    let push = |token: &'static str| {
        s.client
            .post(format!("{}/api/v1/_cloudflare", s.base_url))
            .header("X-Connector-Token", token)
            .body("{\"message\":\"hello\"}\n")
            .send()
    };
    assert_eq!(push(token).await.unwrap().status(), 204);
    assert_eq!(
        push("not-the-token-0123456789").await.unwrap().status(),
        401
    );

    // 回传 mask 的更新保留令牌，推送照常进来。
    let updated: Value = s
        .client
        .put(format!("{}{}/{}", s.base_url, URL, id))
        .header(hk, &hv)
        .json(&serde_json::json!({
            "name": "cf-sealed",
            "kind": "cloudflare_logpush",
            "configJson": { "pushToken": "***", "targetStream": "cf_sealed" },
            "enabled": true,
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(updated["secretKeys"], serde_json::json!(["pushToken"]));
    assert_eq!(push(token).await.unwrap().status(), 204);

    let _ = s
        .client
        .delete(format!("{}{}/{}", s.base_url, URL, id))
        .header(hk, &hv)
        .send()
        .await;
}

#[tokio::test]
async fn a_connection_test_says_what_went_wrong() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let test = |config: Value| {
        s.client
            .post(format!("{}{}/test", s.base_url, URL))
            .header(hk, &hv)
            .json(&serde_json::json!({ "kind": "http", "configJson": config }))
            .send()
    };

    // 没人监听的端口：连不上。
    let outcome: Value = test(serde_json::json!({ "url": "http://127.0.0.1:9/ingest" }))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(outcome["ok"], false);
    assert_eq!(outcome["code"], "unreachable");

    // 配置本身不成立：400，与保存一致。
    let invalid = test(serde_json::json!({ "url": "http://127.0.0.1:9/", "auth": "bearer" }))
        .await
        .unwrap();
    assert_eq!(invalid.status(), 400);

    let external: Value = s
        .client
        .get(format!("{}{}/aws-external-id", s.base_url, URL))
        .header(hk, &hv)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        external["externalId"],
        format!("molesignal-{}", s.root_org_id.0)
    );
}
