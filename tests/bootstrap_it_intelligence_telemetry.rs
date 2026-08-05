// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Intelligence 模型遥测 license-gate 冒烟。
//!
//!  feature 启用 + license OFF → 4 个 intelligence 路由全返 403。
//! 实际 license ON 路径由 it_intelligence_chat / it_intelligence_mcp 覆盖（依赖 wiremock 假 LLM）；
//! 本文件只关心 gate 是否生效，跑得最快。

mod common;

#[tokio::test]
async fn intelligence_routes_blocked_when_license_off() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    // OSS 默认 CommunityLicense → has_feature("intelligence") == false → 路由返 403
    let routes = [
        ("POST", "/api/v1/intelligence/chat"),
        ("GET", "/api/v1/intelligence/chat"),
        ("POST", "/api/v1/intelligence/chat/chat-id/messages"),
        ("GET", "/api/v1/intelligence/stats"),
        (
            "GET",
            "/api/v1/intelligence/dashboard-authoring/capabilities",
        ),
    ];
    for (method, path) in routes {
        let url = format!("{}{}", s.base_url, path);
        let req = match method {
            "GET" => s.client.get(&url),
            "POST" => s.client.post(&url).json(&serde_json::json!({})),
            _ => unreachable!(),
        };
        let resp = req.header(hk, &hv).send().await.unwrap();
        let code = resp.status().as_u16();
        assert!(
            code == 403 || code == 404,
            "{method} {path}: expected 403/404 license gate, got {code}"
        );
    }
}
