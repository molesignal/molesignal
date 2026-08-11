// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! teams CRUD + member_ids + OrgAdmin write gate + validation.

mod common;

use molesignal::{domain::iam::IamMembership, shared::time::TimestampMicros};
use serde_json::Value;

#[tokio::test]
async fn teams_crud_with_members() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    // create with one member (the root user)
    let member = s.root_user_id.0.clone();
    let create = s
        .client
        .post(format!("{}/api/v1/teams", s.base_url))
        .header(hk, &hv)
        .json(&serde_json::json!({ "name": "platform", "member_ids": [member] }))
        .send()
        .await
        .unwrap();
    assert!(
        create.status().is_success(),
        "create failed: {}",
        create.status()
    );
    let created: Value = create.json().await.unwrap();
    let id = created
        .get("id")
        .and_then(Value::as_str)
        .expect("created id")
        .to_string();
    assert_eq!(
        created
            .get("member_ids")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1)
    );

    // get
    let got = s
        .client
        .get(format!("{}/api/v1/teams/{id}", s.base_url))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert!(got.status().is_success());
    assert_eq!(
        got.json::<Value>()
            .await
            .unwrap()
            .get("name")
            .and_then(Value::as_str),
        Some("platform")
    );

    // update: rename + clear members
    let upd = s
        .client
        .put(format!("{}/api/v1/teams/{id}", s.base_url))
        .header(hk, &hv)
        .json(&serde_json::json!({ "name": "platform-eng", "member_ids": [] }))
        .send()
        .await
        .unwrap();
    assert!(upd.status().is_success());
    let upd_v: Value = upd.json().await.unwrap();
    assert_eq!(
        upd_v.get("name").and_then(Value::as_str),
        Some("platform-eng")
    );
    assert_eq!(
        upd_v
            .get("member_ids")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );

    // list — present
    let list = s
        .client
        .get(format!("{}/api/v1/teams", s.base_url))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert!(list.status().is_success());
    let items: Value = list.json().await.unwrap();
    assert!(
        items
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.get("id").and_then(Value::as_str) == Some(id.as_str()))
    );

    // delete → 404 after
    let del = s
        .client
        .delete(format!("{}/api/v1/teams/{id}", s.base_url))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert!(del.status().is_success());
    let after = s
        .client
        .get(format!("{}/api/v1/teams/{id}", s.base_url))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert_eq!(after.status().as_u16(), 404);
}

#[tokio::test]
async fn team_writes_require_org_admin() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    // JWT 不携带角色；权限始终从当前 IAM role bindings 解析。
    // 创建一个只具有默认自助注册角色的用户来验证写权限拒绝。
    let viewer_user = s
        .state
        .iam
        .service
        .create_user(
            "teams-viewer@test.example".into(),
            "Teams Viewer".into(),
            "viewerpass",
        )
        .await
        .expect("create viewer user");
    let viewer_role_id = s
        .state
        .iam
        .service
        .iam_memberships
        .role_id_for_purpose(&s.root_org_id, "self_service_signup")
        .await
        .expect("resolve signup IAM role");
    s.state
        .iam
        .service
        .iam_memberships
        .upsert(
            IamMembership {
                user_id: viewer_user.id.clone(),
                org_id: s.root_org_id.clone(),
                joined_at: TimestampMicros::now(),
            },
            &[viewer_role_id],
            &s.root_user_id,
        )
        .await
        .expect("assign viewer IAM role");
    let viewer = s
        .state
        .iam
        .service
        .issue_token(&viewer_user.id, &s.root_org_id)
        .expect("viewer token");
    let vhdr = format!("Bearer {viewer}");

    let create = s
        .client
        .post(format!("{}/api/v1/teams", s.base_url))
        .header("authorization", &vhdr)
        .json(&serde_json::json!({ "name": "nope", "member_ids": [] }))
        .send()
        .await
        .unwrap();
    assert_eq!(create.status().as_u16(), 403, "viewer create should be 403");

    let list = s
        .client
        .get(format!("{}/api/v1/teams", s.base_url))
        .header("authorization", &vhdr)
        .send()
        .await
        .unwrap();
    assert!(
        list.status().is_success(),
        "viewer list should be 200; got {}",
        list.status()
    );
}

#[tokio::test]
async fn team_member_ids_must_be_org_members() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();
    // A user id that isn't a member of this org must be rejected (400), not
    // silently embedded as a phantom escalation target.
    let resp = s
        .client
        .post(format!("{}/api/v1/teams", s.base_url))
        .header(hk, &hv)
        .json(&serde_json::json!({ "name": "ghosts", "member_ids": ["not-a-member-id"] }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 400, "non-member id should be 400");
}

#[tokio::test]
async fn team_validation_rejects_blank_name() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();
    let resp = s
        .client
        .post(format!("{}/api/v1/teams", s.base_url))
        .header(hk, &hv)
        .json(&serde_json::json!({ "name": "  ", "member_ids": [] }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 400);
}
