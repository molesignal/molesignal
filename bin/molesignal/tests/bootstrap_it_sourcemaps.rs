// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! RUM debug artifact route replacement smoke tests.

mod common;

#[tokio::test]
async fn debug_artifact_upload_endpoint_reachable() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    // 当前 reqwest workspace 配置未启用 multipart feature；本测试只做
    // application/json 体的冒烟（spec scenario "upload" 的功能性由 handler unit 覆盖；
    // 这里只验证路由挂载 + 鉴权放行）。
    let resp = s
        .client
        .post(format!("{}/api/v1/debug-artifacts", s.base_url))
        .header(hk, &hv)
        .header("content-type", "application/json")
        .json(&serde_json::json!({
            "application_id": "storefront",
            "service": "web",
            "release": "v1.0.0",
            "kind": "javascript_sourcemap",
            "platform": "web",
        }))
        .send()
        .await
        .unwrap();
    let code = resp.status().as_u16();
    // 端点存在即可，未 wire 时 200/201/400/422 均可，不应 404/401/403
    assert!(
        code != 404 && code != 401 && code != 403,
        "debug-artifacts upload route should be reachable, got {code}"
    );
}

#[tokio::test]
async fn legacy_sourcemaps_route_is_not_exposed() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let resp = s
        .client
        .get(format!("{}/api/v1/sourcemaps", s.base_url))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 404);
}
