// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `/api/v1/me/*` 端到端：头像上传到对象存储 + 公开服务、邮箱不可修改。

mod common;

#[tokio::test]
async fn avatar_upload_serves_publicly_and_email_is_immutable() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    // 上传：handler 只看 content-type / 大小 / 非空，不解码图片，所以用占位字节即可。
    let img: Vec<u8> = b"\x89PNG\r\n\x1a\nmolesignal-avatar-test".to_vec();
    let resp = s
        .client
        .post(format!("{}/api/v1/me/avatar", s.base_url))
        .header(hk, &hv)
        .header("content-type", "image/png")
        .body(img.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "avatar upload should succeed");
    let body: serde_json::Value = resp.json().await.unwrap();
    let avatar_url = body["avatar_url"]
        .as_str()
        .expect("avatar_url set")
        .to_string();
    assert!(
        avatar_url.starts_with("/api/v1/public/avatars/"),
        "avatar_url should point at the public serve endpoint, got {avatar_url}"
    );

    // 公开读取（不带 Authorization）→ 200 + 原始字节 + image/png。
    let serve = s
        .client
        .get(format!("{}{}", s.base_url, avatar_url))
        .send()
        .await
        .unwrap();
    assert_eq!(serve.status(), 200, "public avatar serve (no auth)");
    assert_eq!(
        serve
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("image/png")
    );
    let served = serve.bytes().await.unwrap();
    assert_eq!(served.as_ref(), img.as_slice(), "served bytes match upload");

    // 非图片 content-type → 400。
    let bad = s
        .client
        .post(format!("{}/api/v1/me/avatar", s.base_url))
        .header(hk, &hv)
        .header("content-type", "text/plain")
        .body(b"nope".to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), 400, "non-image upload rejected");

    // 邮箱不可修改：PUT 带新 email → email 不变，但 display_name 仍可改。
    let before: serde_json::Value = s
        .client
        .get(format!("{}/api/v1/me/profile", s.base_url))
        .header(hk, &hv)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let original_email = before["email"].as_str().unwrap().to_string();

    let put: serde_json::Value = s
        .client
        .put(format!("{}/api/v1/me/profile", s.base_url))
        .header(hk, &hv)
        .json(&serde_json::json!({
            "display_name": "Renamed User",
            "email": "evil@attacker.test"
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(put["email"], original_email, "email must stay immutable");
    assert_eq!(
        put["display_name"], "Renamed User",
        "display_name still updates"
    );

    // 通用用户更新端点也必须执行同一不可变约束，不能绕过 `/me/profile`。
    let directory_patch = s
        .client
        .patch(format!("{}/api/v1/users/{}", s.base_url, s.root_user_id))
        .header(hk, &hv)
        .json(&serde_json::json!({"email": "other@attacker.test"}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        directory_patch.status(),
        400,
        "directory endpoint must reject email changes"
    );

    let after: serde_json::Value = s
        .client
        .get(format!("{}/api/v1/me/profile", s.base_url))
        .header(hk, &hv)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(after["email"], original_email);
}
