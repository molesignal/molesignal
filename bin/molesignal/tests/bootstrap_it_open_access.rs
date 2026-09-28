// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Product capabilities remain available to authorized organization users.
mod common;

#[tokio::test]
async fn authorized_users_can_list_product_resources() {
    if common::skip_unless_enabled() {
        return;
    }
    let server = common::TestServer::start().await;
    let (header, token) = server.auth_header();
    for path in [
        "/api/v1/clusters",
        "/api/v1/domains",
        "/api/v1/agent/chat",
        "/api/v1/auth/sso/providers",
    ] {
        let response = server
            .client
            .get(format!("{}{path}", server.base_url))
            .header(header, &token)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 200, "{path}");
    }
}
