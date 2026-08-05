// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 登录 / 鉴权链路冒烟。
//!
//! 默认跳过；设 `MS_RUN_IT=1` 才跑（依赖 docker postgres）。

mod common;

use common::{TestServer, skip_unless_enabled};
use molesignal::shared::time::TimestampMicros;
use serde_json::json;
use sha2::{Digest, Sha256};

#[tokio::test]
async fn login_returns_token_and_protected_routes_require_it() {
    if skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }
    let s = TestServer::start().await;

    // 1. 登录成功
    let resp = s
        .client
        .post(format!("{}/api/v1/auth/login", s.base_url))
        .json(&json!({"email": s.root_email, "password": s.root_password}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "login should succeed");
    let body: serde_json::Value = resp.json().await.unwrap();
    let token = body["token"]
        .as_str()
        .expect("token in response")
        .to_string();
    assert!(!token.is_empty());
    assert_eq!(body["user_id"], s.root_user_id.0);
    assert_eq!(body["org_id"], s.root_org_id.0);

    // 2. 密码错 → 401
    let resp = s
        .client
        .post(format!("{}/api/v1/auth/login", s.base_url))
        .json(&json!({"email": s.root_email, "password": "wrong"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // 3. 不存在的邮箱也是 401（同样消息，避免 user-enumeration）
    let resp = s
        .client
        .post(format!("{}/api/v1/auth/login", s.base_url))
        .json(&json!({"email": "nobody@nope", "password": "any"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // 4. 缺 token 调受保护接口 → 401
    let resp = s
        .client
        .get(format!("{}/api/v1/dashboards", s.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // 5. 带 token → 200
    let resp = s
        .client
        .get(format!("{}/api/v1/dashboards", s.base_url))
        .header("authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // 6. healthz 是公开的，无 token 也通
    let resp = s
        .client
        .get(format!("{}/api/v1/healthz", s.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn default_ingestion_token_is_redisplayable_and_usable() {
    if skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }
    let s = TestServer::start().await;

    // 登录拿 session token
    let body: serde_json::Value = s
        .client
        .post(format!("{}/api/v1/auth/login", s.base_url))
        .json(&json!({"email": s.root_email, "password": s.root_password}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let session = body["token"].as_str().unwrap().to_string();

    // 第一次取默认接入 token：自动创建并回显完整明文
    let first: serde_json::Value = s
        .client
        .get(format!("{}/api/v1/auth/tokens/default", s.base_url))
        .header("authorization", format!("Bearer {session}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let token1 = first["token"].as_str().expect("token").to_string();
    assert!(
        token1.starts_with("ms_"),
        "default token is a PAT: {token1}"
    );

    // 第二次取：必须返回完全相同的明文（可重复回显，而非每次新建）
    let second: serde_json::Value = s
        .client
        .get(format!("{}/api/v1/auth/tokens/default", s.base_url))
        .header("authorization", format!("Bearer {session}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        token1,
        second["token"].as_str().unwrap(),
        "default token must be re-displayable (same value)"
    );
    assert_eq!(first["id"], second["id"], "same token row");

    // 该默认 token 本身可作 Bearer 鉴权，且只获得写入能力。
    let capabilities = s
        .client
        .get(format!("{}/api/v1/iam/capabilities", s.base_url))
        .header("authorization", format!("Bearer {token1}"))
        .send()
        .await
        .unwrap();
    assert_eq!(capabilities.status(), 200);
    let capabilities: serde_json::Value = capabilities.json().await.unwrap();
    let permissions = capabilities["permissions"].as_array().unwrap();
    assert!(permissions.iter().any(|value| value == "streams.write"));
    assert!(!permissions.iter().any(|value| value == "streams.read"));

    let dashboards = s
        .client
        .get(format!("{}/api/v1/dashboards", s.base_url))
        .header("authorization", format!("Bearer {token1}"))
        .send()
        .await
        .unwrap();
    assert_eq!(dashboards.status(), 403, "ingestion token is write-only");
}

#[tokio::test]
async fn forgot_password_is_public_and_non_enumerating() {
    if skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }
    let s = TestServer::start().await;

    for email in [&s.root_email, "nobody@test.example"] {
        let response = s
            .client
            .post(format!("{}/api/v1/auth/forgot-password", s.base_url))
            .json(&json!({"email": email, "locale": "en-US"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 202);
        assert_eq!(
            response.json::<serde_json::Value>().await.unwrap(),
            json!({"accepted": true})
        );
    }
}

#[tokio::test]
async fn password_reset_token_is_one_time_and_changes_password() {
    if skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }
    let s = TestServer::start().await;
    let raw_token = "integration-test-password-reset-token-1234567890";
    let token_hash = hex::encode(Sha256::digest(raw_token.as_bytes()));
    let now = TimestampMicros::now();

    let issued = s
        .state
        .iam
        .password_resets
        .issue(
            &s.root_user_id,
            &token_hash,
            now,
            TimestampMicros(now.0 + 30 * 60 * 1_000_000),
            60 * 1_000_000,
        )
        .await
        .unwrap();
    assert!(issued);

    let new_password = "new-root-password";
    let response = s
        .client
        .post(format!("{}/api/v1/auth/reset-password", s.base_url))
        .json(&json!({"token": raw_token, "password": new_password}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    let old_login = s
        .client
        .post(format!("{}/api/v1/auth/signin", s.base_url))
        .json(&json!({"email": s.root_email, "password": s.root_password}))
        .send()
        .await
        .unwrap();
    assert_eq!(old_login.status(), 401);

    let new_login = s
        .client
        .post(format!("{}/api/v1/auth/signin", s.base_url))
        .json(&json!({"email": s.root_email, "password": new_password}))
        .send()
        .await
        .unwrap();
    assert_eq!(new_login.status(), 200);

    let replay = s
        .client
        .post(format!("{}/api/v1/auth/reset-password", s.base_url))
        .json(&json!({"token": raw_token, "password": "another-password"}))
        .send()
        .await
        .unwrap();
    assert_eq!(replay.status(), 400);
}
