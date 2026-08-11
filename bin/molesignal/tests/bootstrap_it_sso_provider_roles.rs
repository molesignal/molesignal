// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

mod common;

use common::{TestServer, skip_unless_enabled};
use reqwest::StatusCode;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct AssignableRole {
    id: String,
    name: String,
}

#[tokio::test]
async fn sso_provider_role_options_include_organization_roles() {
    if skip_unless_enabled() {
        return;
    }

    let server = TestServer::start().await;
    let response = server
        .client
        .get(format!("{}/api/v1/sso/providers/roles", server.base_url))
        .header("authorization", format!("Bearer {}", server.root_token))
        .send()
        .await
        .expect("list SSO-assignable roles");

    assert_eq!(response.status(), StatusCode::OK);
    let roles = response
        .json::<Vec<AssignableRole>>()
        .await
        .expect("decode assignable roles");
    assert!(
        !roles.is_empty(),
        "a bootstrapped organization must expose assignable roles"
    );
    assert!(
        roles
            .iter()
            .all(|role| !role.id.is_empty() && !role.name.is_empty())
    );
}
