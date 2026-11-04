// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 登录 / 鉴权链路冒烟。
//!
//! 默认跳过；设 `MS_RUN_IT=1` 才跑（依赖 docker postgres）。

mod common;

use common::{TestServer, session_cookie, set_cookie, skip_unless_enabled};
use molesignal::shared::time::TimestampMicros;
use serde_json::json;
use sha2::{Digest, Sha256};

#[tokio::test]
async fn sign_in_sets_the_session_cookies_that_protected_routes_require() {
    if skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }
    let s = TestServer::start().await;

    // 1. 登录成功：access token 与 refresh token 只在 Cookie 里
    let resp = s
        .client
        .post(format!("{}/api/v1/auth/signin", s.base_url))
        .json(&json!({"email": s.root_email, "password": s.root_password}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "sign-in should succeed");
    let access = set_cookie(&resp, "accessToken").expect("access cookie");
    let refresh = set_cookie(&resp, "refreshToken").expect("refresh cookie");
    assert!(!access.is_empty() && !refresh.is_empty());
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body.get("token").is_none(),
        "tokens never travel in the body"
    );
    assert_eq!(body["login"]["userId"], s.root_user_id.0);
    assert_eq!(body["login"]["orgId"], s.root_org_id.0);

    // 2. 密码错 → 401
    let resp = s
        .client
        .post(format!("{}/api/v1/auth/signin", s.base_url))
        .json(&json!({"email": s.root_email, "password": "wrong"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // 3. 不存在的邮箱也是 401（同样消息，避免 user-enumeration）
    let resp = s
        .client
        .post(format!("{}/api/v1/auth/signin", s.base_url))
        .json(&json!({"email": "nobody@nope", "password": "any"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // 4. 没有凭据调受保护接口 → 401
    let dashboards = format!("{}/api/v1/dashboards", s.base_url);
    let resp = s.client.get(&dashboards).send().await.unwrap();
    assert_eq!(resp.status(), 401);

    // 5. 带会话 Cookie → 200
    let resp = s
        .client
        .get(&dashboards)
        .header("cookie", format!("accessToken={access}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // 6. 会话 token 放进 Authorization 不算数；refresh token 也不能当 access token 用
    let resp = s
        .client
        .get(&dashboards)
        .bearer_auth(&access)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
    let resp = s
        .client
        .get(&dashboards)
        .header("cookie", format!("accessToken={refresh}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // 7. 别的网站发起的写请求被拒
    let resp = s
        .client
        .post(format!("{}/api/v1/teams", s.base_url))
        .header("cookie", format!("accessToken={access}"))
        .header("sec-fetch-site", "cross-site")
        .json(&json!({"name": "csrf", "memberIds": []}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);

    // 8. refresh token 换新的 access token
    let resp = s
        .client
        .post(format!("{}/api/v1/auth/refresh", s.base_url))
        .header("cookie", format!("refreshToken={refresh}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 204);
    let renewed = set_cookie(&resp, "accessToken").expect("renewed access cookie");
    let resp = s
        .client
        .get(&dashboards)
        .header("cookie", format!("accessToken={renewed}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // 9. access token 换不了新的
    let resp = s
        .client
        .post(format!("{}/api/v1/auth/refresh", s.base_url))
        .header("cookie", format!("refreshToken={access}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // 10. 退出登录删掉两个 Cookie
    let resp = s
        .client
        .post(format!("{}/api/v1/auth/signout", s.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 204);
    assert_eq!(set_cookie(&resp, "accessToken").as_deref(), Some(""));
    assert_eq!(set_cookie(&resp, "refreshToken").as_deref(), Some(""));

    // 11. healthz 是公开的，无凭据也通
    let resp = s
        .client
        .get(format!("{}/api/v1/healthz", s.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn default_intake_token_is_redisplayable_and_usable() {
    if skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }
    let s = TestServer::start().await;

    // 登录拿会话 Cookie
    let signin = s
        .client
        .post(format!("{}/api/v1/auth/signin", s.base_url))
        .json(&json!({"email": s.root_email, "password": s.root_password}))
        .send()
        .await
        .unwrap();
    let session = format!("accessToken={}", session_cookie(&signin));

    // 第一次取默认接入 token：自动创建并回显完整明文
    let first: serde_json::Value = s
        .client
        .get(format!("{}/api/v1/me/api-tokens/default", s.base_url))
        .header("cookie", &session)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let token1 = first["token"].as_str().expect("token").to_string();
    assert!(
        token1.starts_with("mspat-"),
        "default token is a PAT: {token1}"
    );

    // 第二次取：必须返回完全相同的明文（可重复回显，而非每次新建）
    let second: serde_json::Value = s
        .client
        .get(format!("{}/api/v1/me/api-tokens/default", s.base_url))
        .header("cookie", &session)
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
    assert!(permissions.iter().any(|value| value == "logs.write"));
    assert!(!permissions.iter().any(|value| value == "streams.read"));

    let dashboards = s
        .client
        .get(format!("{}/api/v1/dashboards", s.base_url))
        .header("authorization", format!("Bearer {token1}"))
        .send()
        .await
        .unwrap();
    assert_eq!(dashboards.status(), 403, "intake token is write-only");
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
