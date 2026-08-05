// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! OSS 编译版 license gate 行为。
//!
//! 默认编译（no `` feature）下：
//! - `state.platform.license` 是 `CommunityLicense`
//! - SSO 端点 → 403 "sso feature not licensed"
//! - remote_clusters 端点 → 403 "federated_search feature not licensed"
//! - Intelligence 路由不注册 → 404

mod common;

#[tokio::test]
async fn sso_login_returns_403_in_oss() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();
    let resp = s
        .client
        .get(format!("{}/api/v1/auth/sso/login", s.base_url))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 403, "OSS SSO must be 403");
}

#[tokio::test]
async fn remote_clusters_list_returns_403_in_oss() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();
    let resp = s
        .client
        .get(format!("{}/api/v1/clusters", s.base_url))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status().as_u16(),
        403,
        "OSS federated_search must be 403"
    );
}

#[tokio::test]
async fn federated_query_returns_403_in_oss() {
    // POST /query?clusters=<远端> 在 OSS 下必须 403。
    // 纯本地查询（无 clusters / clusters=local）不受闸门影响，仍可执行。
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();
    let resp = s
        .client
        .post(format!("{}/api/v1/query?clusters=sf", s.base_url))
        .header(hk, &hv)
        .json(&serde_json::json!({
            // org_id 是 QueryRequest 必填字段（虽然 handler 会用 ctx 覆盖）；
            // 漏了它会在 body 反序列化阶段返 422，到不了 federated_search 闸门。
            "org_id": s.root_org_id.0,
            "language": "sql",
            "statement": "SELECT 1",
            "time_range": { "start": 0_i64, "end": 1_000_000_i64 }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status().as_u16(),
        403,
        "OSS federated query (clusters=sf) must be 403"
    );
}

#[tokio::test]
async fn intelligence_endpoints_404_in_oss_build() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();
    // 当前架构：intelligence 路由永远编译，由 handler 内 `license.has_feature("intelligence")` 拦截。
    // OSS CommunityLicense 触发 403；与 it_intelligence_fanout / oss_premium_routes_return_404 一致。
    let resp = s
        .client
        .get(format!("{}/api/v1/intelligence/stats", s.base_url))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    let code = resp.status().as_u16();
    assert!(
        code == 403 || code == 404,
        "OSS intelligence route must be unregistered or license-gated, got {code}"
    );
}

// === OSS 路由 license-gate 全覆盖 ===

#[tokio::test]
async fn oss_premium_routes_return_404() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();
    let routes = [
        "/api/v1/intelligence/mcp",
        "/api/v1/intelligence/chat",
        "/api/v1/marketplace/subscriptions",
        "/api/v1/domains",
    ];
    for path in routes {
        let resp = s
            .client
            .get(format!("{}{}", s.base_url, path))
            .header(hk, &hv)
            .send()
            .await
            .unwrap();
        let code = resp.status().as_u16();
        assert!(
            code == 404 || code == 403,
            "OSS {path} should be unregistered/blocked, got {code}"
        );
    }
}
