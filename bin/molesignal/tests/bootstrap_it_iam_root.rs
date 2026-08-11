// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

mod common;

use common::{TestServer, skip_unless_enabled};
use reqwest::StatusCode;

#[tokio::test]
async fn root_user_cannot_be_removed_by_iam_apis() {
    if skip_unless_enabled() {
        return;
    }

    let server = TestServer::start().await;
    let authorization = format!("Bearer {}", server.root_token);

    let membership_response = server
        .client
        .delete(format!(
            "{}/api/v1/orgs/{}/members/{}",
            server.base_url, server.root_org_id.0, server.root_user_id.0
        ))
        .header("authorization", &authorization)
        .send()
        .await
        .expect("remove root membership");
    assert_eq!(membership_response.status(), StatusCode::FORBIDDEN);

    let user_response = server
        .client
        .delete(format!(
            "{}/api/v1/users/{}",
            server.base_url, server.root_user_id.0
        ))
        .header("authorization", &authorization)
        .send()
        .await
        .expect("delete root user");
    assert_eq!(user_response.status(), StatusCode::FORBIDDEN);

    server
        .state
        .iam
        .service
        .users
        .get(&server.root_user_id)
        .await
        .expect("root user must still exist");
    let memberships = server
        .state
        .iam
        .service
        .iam_memberships
        .list_for_user(&server.root_user_id)
        .await
        .expect("root memberships must still be readable");
    assert!(
        memberships
            .iter()
            .any(|membership| membership.org_id == server.root_org_id)
    );
}
