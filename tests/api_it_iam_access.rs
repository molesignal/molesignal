// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

mod common;

use common::{TestServer, skip_unless_enabled};
use molesignal::{
    domain::iam::{
        IamMembership,
        api_token::{ApiToken, ApiTokenKind},
    },
    infra::persistence::repositories::api_tokens::{
        assemble_token, generate_token_parts, hash_secret,
    },
    shared::{ids::Id, time::TimestampMicros},
};
use reqwest::StatusCode;
use serde_json::{Value, json};

#[tokio::test]
async fn disabling_user_immediately_invalidates_existing_jwt_and_api_tokens() {
    if skip_unless_enabled() {
        return;
    }

    let server = TestServer::start().await;
    let user = server
        .state
        .iam
        .service
        .create_user(
            "disabled-user@test.example".into(),
            "Disabled User".into(),
            "userpass",
        )
        .await
        .expect("create user");
    let viewer_role_id = server
        .state
        .iam
        .service
        .iam_memberships
        .role_id_for_purpose(&server.root_org_id, "self_service_signup")
        .await
        .expect("resolve viewer role");
    server
        .state
        .iam
        .service
        .iam_memberships
        .upsert(
            IamMembership {
                user_id: user.id.clone(),
                org_id: server.root_org_id.clone(),
                joined_at: TimestampMicros::now(),
            },
            &[viewer_role_id],
            &server.root_user_id,
        )
        .await
        .expect("create user membership");

    let jwt = server
        .state
        .iam
        .service
        .issue_token(&user.id, &server.root_org_id)
        .expect("issue JWT");
    let api_role_id = server
        .state
        .iam
        .service
        .iam_memberships
        .role_id_for_purpose(&server.root_org_id, "default_api_token")
        .await
        .expect("resolve API token role");
    let (prefix, secret) = generate_token_parts();
    let api_token = assemble_token(&prefix, &secret);
    server
        .state
        .iam
        .api_tokens
        .create(ApiToken {
            id: Id::new(),
            prefix,
            secret_hash: hash_secret(&secret).expect("hash API token secret"),
            org_id: server.root_org_id.clone(),
            user_id: user.id.clone(),
            role_id: api_role_id,
            name: "disabled-user-token".into(),
            expires_at: None,
            last_used_at: None,
            revoked: false,
            created_at: TimestampMicros::now(),
            is_default: false,
            token_kind: ApiTokenKind::Personal,
            application_id: None,
        })
        .await
        .expect("create API token");

    for credential in [&jwt, &api_token] {
        server
            .client
            .get(format!("{}/api/v1/iam/capabilities", server.base_url))
            .bearer_auth(credential)
            .send()
            .await
            .expect("request capabilities before disable")
            .error_for_status()
            .expect("credential works before disable");
    }

    server
        .client
        .patch(format!("{}/api/v1/users/{}", server.base_url, user.id))
        .bearer_auth(&server.root_token)
        .json(&json!({"disabled": true}))
        .send()
        .await
        .expect("disable user")
        .error_for_status()
        .expect("user disable succeeds");

    for credential in [&jwt, &api_token] {
        let response = server
            .client
            .get(format!("{}/api/v1/iam/capabilities", server.base_url))
            .bearer_auth(credential)
            .send()
            .await
            .expect("request capabilities after disable");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn system_capabilities_resolve_role_name_and_permissions_from_database() {
    if skip_unless_enabled() {
        return;
    }

    let server = TestServer::start().await;
    let tenant_dashboards: Value = server
        .client
        .get(format!("{}/api/v1/dashboards", server.base_url))
        .bearer_auth(&server.root_token)
        .send()
        .await
        .expect("list tenant dashboards")
        .error_for_status()
        .expect("tenant dashboard list succeeds")
        .json()
        .await
        .expect("tenant dashboard list JSON");
    assert_eq!(tenant_dashboards, json!([]));

    server
        .state
        .iam
        .platform_administrators
        .bootstrap_root(&server.root_user_id)
        .await
        .expect("reconcile root platform administrator");

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&server.settings.store.meta.dsn)
        .await
        .expect("connect test database");
    let role_id: String = sqlx::query_scalar(
        "SELECT role.id
           FROM iam_builtin_role_purposes purpose
           JOIN iam_roles role
             ON role.org_id = $1
            AND role.role_key = purpose.role_key
          WHERE purpose.purpose = 'platform_administrator'",
    )
    .bind(server.state.iam.system_org_id.as_str())
    .fetch_one(&pool)
    .await
    .expect("resolve materialized system role");
    sqlx::query("UPDATE iam_roles SET name = 'Platform Steward' WHERE id = $1")
        .bind(&role_id)
        .execute(&pool)
        .await
        .expect("rename system role");
    let system_token = server
        .state
        .iam
        .service
        .issue_system_token(&server.root_user_id, &server.state.iam.system_org_id)
        .expect("issue system token");
    let capabilities: Value = server
        .client
        .get(format!("{}/api/v1/iam/capabilities", server.base_url))
        .bearer_auth(&system_token)
        .send()
        .await
        .expect("get system capabilities")
        .error_for_status()
        .expect("system capabilities succeed")
        .json()
        .await
        .expect("system capabilities JSON");

    assert_eq!(capabilities["display_role"], "Platform Steward");
    assert_eq!(capabilities["roles"][0]["id"], role_id);
    assert!(
        capabilities["permissions"]
            .as_array()
            .expect("permissions array")
            .iter()
            .any(|permission| permission == "sys.licenses.read")
    );
    assert!(
        capabilities["permissions"]
            .as_array()
            .expect("permissions array")
            .iter()
            .any(|permission| permission == "sys.dashboards.read")
    );
    assert!(
        capabilities["permissions"]
            .as_array()
            .expect("permissions array")
            .iter()
            .any(|permission| permission == "sys.organizations.manage")
    );
    assert!(
        capabilities["permissions"]
            .as_array()
            .expect("permissions array")
            .iter()
            .any(|permission| permission == "sys.licenses.manage")
    );
    assert!(
        capabilities["routes"]
            .as_array()
            .expect("route decisions")
            .iter()
            .any(|route| { route["id"] == "iam.organizations" && route["allowed"] == true })
    );

    let system_dashboards: Value = server
        .client
        .get(format!("{}/api/v1/dashboards", server.base_url))
        .bearer_auth(&system_token)
        .send()
        .await
        .expect("list system dashboards")
        .error_for_status()
        .expect("system dashboard list succeeds")
        .json()
        .await
        .expect("system dashboard list JSON");
    let dashboards = system_dashboards
        .as_array()
        .expect("system dashboards array");
    assert_eq!(dashboards.len(), 1);
    assert_eq!(dashboards[0]["uid"], "molesignal-system-overview");
    assert_eq!(
        dashboards[0]["org_id"],
        server.state.iam.system_org_id.as_str()
    );
    assert_eq!(dashboards[0]["model"]["editable"], false);
    assert_eq!(dashboards[0]["model"]["defaultDashboard"], true);
    let dashboard_id = dashboards[0]["id"].as_str().expect("built-in dashboard id");

    let system_dashboard = server
        .client
        .get(format!(
            "{}/api/v1/dashboards/{dashboard_id}",
            server.base_url
        ))
        .bearer_auth(&system_token)
        .send()
        .await
        .expect("read built-in system dashboard");
    assert_eq!(system_dashboard.status(), StatusCode::OK);

    let tenant_cross_org_read = server
        .client
        .get(format!(
            "{}/api/v1/dashboards/{dashboard_id}",
            server.base_url
        ))
        .bearer_auth(&server.root_token)
        .send()
        .await
        .expect("attempt tenant read of built-in system dashboard");
    assert_eq!(tenant_cross_org_read.status(), StatusCode::NOT_FOUND);

    let system_folders = server
        .client
        .get(format!("{}/api/v1/folders", server.base_url))
        .bearer_auth(&system_token)
        .send()
        .await
        .expect("list system dashboard folders");
    assert_eq!(system_folders.status(), StatusCode::OK);

    let denied_system_create = server
        .client
        .post(format!("{}/api/v1/dashboards", server.base_url))
        .bearer_auth(&system_token)
        .json(&json!({"model": dashboard_model("system-copy", "System copy")}))
        .send()
        .await
        .expect("attempt dashboard creation in system scope");
    assert_eq!(denied_system_create.status(), StatusCode::FORBIDDEN);

    let read_response = server
        .client
        .get(format!("{}/api/v1/system/license", server.base_url))
        .bearer_auth(&system_token)
        .send()
        .await
        .expect("read license with database permission");
    assert_eq!(read_response.status(), StatusCode::OK);

    let organizations: Value = server
        .client
        .get(format!("{}/api/v1/orgs", server.base_url))
        .bearer_auth(&server.root_token)
        .send()
        .await
        .expect("list organizations")
        .error_for_status()
        .expect("organization list succeeds")
        .json()
        .await
        .expect("organization list JSON");
    let system = organizations
        .as_array()
        .expect("organization array")
        .iter()
        .find(|organization| organization["system"] == true)
        .expect("system organization");
    assert_eq!(system["display_role"], "Platform Steward");
    assert_eq!(system["roles"][0]["id"], role_id);

    let system_organizations: Value = server
        .client
        .get(format!("{}/api/v1/orgs", server.base_url))
        .bearer_auth(&system_token)
        .send()
        .await
        .expect("list organizations from system scope")
        .error_for_status()
        .expect("system organization list succeeds")
        .json()
        .await
        .expect("system organization list JSON");
    let root_organization = system_organizations
        .as_array()
        .expect("system organization array")
        .iter()
        .find(|organization| organization["id"] == server.root_org_id.as_str())
        .expect("root tenant in system organization list");
    assert_eq!(root_organization["display_role"], "Owner");
    assert_eq!(root_organization["roles"][0]["key"], "owner");

    let created_organization: Value = server
        .client
        .post(format!("{}/api/v1/orgs", server.base_url))
        .bearer_auth(&system_token)
        .json(&json!({
            "name": "Root automatic access",
            "slug": "root-automatic-access"
        }))
        .send()
        .await
        .expect("create organization")
        .error_for_status()
        .expect("root creates organization")
        .json()
        .await
        .expect("created organization JSON");
    assert_eq!(created_organization["display_role"], "Owner");
    let created_organization_id = created_organization["id"]
        .as_str()
        .expect("created organization id");
    let root_membership_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM iam_memberships
             WHERE user_id = $1 AND org_id = $2
         )",
    )
    .bind(server.root_user_id.as_str())
    .bind(created_organization_id)
    .fetch_one(&pool)
    .await
    .expect("check root membership independence");
    assert!(
        !root_membership_exists,
        "root tenant access must not depend on ordinary organization membership"
    );

    let selected: Value = server
        .client
        .post(format!(
            "{}/api/v1/orgs/{created_organization_id}/select",
            server.base_url
        ))
        .bearer_auth(&system_token)
        .send()
        .await
        .expect("select membership-free organization")
        .error_for_status()
        .expect("root can select every organization")
        .json()
        .await
        .expect("selected organization JSON");
    assert_eq!(selected["display_role"], "Owner");
}

#[tokio::test]
async fn organization_identity_and_system_organization_are_immutable() {
    if skip_unless_enabled() {
        return;
    }

    let server = TestServer::start().await;
    let organizations: Value = server
        .client
        .get(format!("{}/api/v1/orgs", server.base_url))
        .bearer_auth(&server.root_token)
        .send()
        .await
        .expect("list organizations")
        .error_for_status()
        .expect("organization list succeeds")
        .json()
        .await
        .expect("organization list JSON");
    let root_org = organizations
        .as_array()
        .expect("organization array")
        .iter()
        .find(|organization| organization["id"] == server.root_org_id.as_str())
        .expect("root organization");
    let original_slug = root_org["slug"].as_str().expect("root organization slug");

    for immutable_patch in [
        json!({"slug": "renamed-workspace"}),
        json!({"id": "replacement-id", "name": "Renamed"}),
    ] {
        let response = server
            .client
            .patch(format!(
                "{}/api/v1/orgs/{}",
                server.base_url, server.root_org_id
            ))
            .bearer_auth(&server.root_token)
            .json(&immutable_patch)
            .send()
            .await
            .expect("patch immutable organization field");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    let renamed: Value = server
        .client
        .patch(format!(
            "{}/api/v1/orgs/{}",
            server.base_url, server.root_org_id
        ))
        .bearer_auth(&server.root_token)
        .json(&json!({"name": "Renamed Workspace"}))
        .send()
        .await
        .expect("rename organization")
        .error_for_status()
        .expect("organization name remains mutable")
        .json()
        .await
        .expect("renamed organization JSON");
    assert_eq!(renamed["name"], "Renamed Workspace");
    assert_eq!(renamed["slug"], original_slug);

    let delete_current = server
        .client
        .delete(format!(
            "{}/api/v1/orgs/{}",
            server.base_url, server.root_org_id
        ))
        .bearer_auth(&server.root_token)
        .send()
        .await
        .expect("delete current organization");
    assert_eq!(delete_current.status(), StatusCode::BAD_REQUEST);

    server
        .state
        .iam
        .platform_administrators
        .bootstrap_root(&server.root_user_id)
        .await
        .expect("reconcile root platform administrator");
    let system_token = server
        .state
        .iam
        .service
        .issue_system_token(&server.root_user_id, &server.state.iam.system_org_id)
        .expect("issue system token");

    let tenant_price_write = server
        .client
        .post(format!("{}/api/v1/model_prices", server.base_url))
        .bearer_auth(&server.root_token)
        .json(&json!({
            "provider": "test",
            "model": "tenant-denied",
            "prompt_usd_per_1k": 0.001,
            "completion_usd_per_1k": 0.002
        }))
        .send()
        .await
        .expect("tenant model price write");
    assert_eq!(tenant_price_write.status(), StatusCode::FORBIDDEN);

    server
        .client
        .post(format!("{}/api/v1/model_prices", server.base_url))
        .bearer_auth(&system_token)
        .json(&json!({
            "provider": "test",
            "model": "platform-managed",
            "prompt_usd_per_1k": 0.001,
            "completion_usd_per_1k": 0.002
        }))
        .send()
        .await
        .expect("platform model price write")
        .error_for_status()
        .expect("sys.settings.manage permits model price writes");
    server
        .client
        .get(format!("{}/api/v1/model_prices", server.base_url))
        .bearer_auth(&system_token)
        .send()
        .await
        .expect("platform model price list")
        .error_for_status()
        .expect("sys.settings.manage permits model price reads");

    let delete_last = server
        .client
        .delete(format!(
            "{}/api/v1/orgs/{}",
            server.base_url, server.root_org_id
        ))
        .bearer_auth(&system_token)
        .send()
        .await
        .expect("delete last organization");
    assert_eq!(delete_last.status(), StatusCode::BAD_REQUEST);

    let update_system = server
        .client
        .patch(format!(
            "{}/api/v1/orgs/{}",
            server.base_url, server.state.iam.system_org_id
        ))
        .bearer_auth(&system_token)
        .json(&json!({"name": "Mutable System"}))
        .send()
        .await
        .expect("update system organization");
    assert_eq!(update_system.status(), StatusCode::FORBIDDEN);

    let delete_system = server
        .client
        .delete(format!(
            "{}/api/v1/orgs/{}",
            server.base_url, server.state.iam.system_org_id
        ))
        .bearer_auth(&system_token)
        .send()
        .await
        .expect("delete system organization");
    assert_eq!(delete_system.status(), StatusCode::FORBIDDEN);

    let mutate_system_members = server
        .client
        .post(format!(
            "{}/api/v1/orgs/{}/members",
            server.base_url, server.state.iam.system_org_id
        ))
        .bearer_auth(&system_token)
        .json(&json!({
            "user_id": server.root_user_id.as_str(),
            "role_ids": []
        }))
        .send()
        .await
        .expect("mutate system organization members");
    assert_eq!(mutate_system_members.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn organization_status_blocks_existing_credentials_and_preserves_recovery() {
    if skip_unless_enabled() {
        return;
    }

    let server = TestServer::start().await;
    server
        .state
        .iam
        .platform_administrators
        .bootstrap_root(&server.root_user_id)
        .await
        .expect("reconcile root platform administrator");
    let system_token = server
        .state
        .iam
        .service
        .issue_system_token(&server.root_user_id, &server.state.iam.system_org_id)
        .expect("issue system token");

    let disable_system = server
        .client
        .patch(format!(
            "{}/api/v1/orgs/{}/status",
            server.base_url, server.state.iam.system_org_id
        ))
        .bearer_auth(&system_token)
        .json(&json!({"disabled": true}))
        .send()
        .await
        .expect("attempt to disable system organization");
    assert_eq!(disable_system.status(), StatusCode::FORBIDDEN);

    let disable_last_enabled = server
        .client
        .patch(format!(
            "{}/api/v1/orgs/{}/status",
            server.base_url, server.root_org_id
        ))
        .bearer_auth(&system_token)
        .json(&json!({"disabled": true}))
        .send()
        .await
        .expect("attempt to disable last enabled tenant");
    assert_eq!(disable_last_enabled.status(), StatusCode::BAD_REQUEST);

    let created: Value = server
        .client
        .post(format!("{}/api/v1/orgs", server.base_url))
        .bearer_auth(&system_token)
        .json(&json!({"name": "Paused tenant", "slug": "paused-tenant"}))
        .send()
        .await
        .expect("create tenant")
        .error_for_status()
        .expect("tenant creation succeeds")
        .json()
        .await
        .expect("tenant JSON");
    let target_org_id = created["id"].as_str().expect("target organization id");
    assert_eq!(created["disabled"], false);

    let selected: Value = server
        .client
        .post(format!(
            "{}/api/v1/orgs/{target_org_id}/select",
            server.base_url
        ))
        .bearer_auth(&server.root_token)
        .send()
        .await
        .expect("select target tenant")
        .error_for_status()
        .expect("target tenant selection succeeds")
        .json()
        .await
        .expect("selection JSON");
    let target_token = selected["token"].as_str().expect("target token");

    let disabled: Value = server
        .client
        .patch(format!(
            "{}/api/v1/orgs/{target_org_id}/status",
            server.base_url
        ))
        .bearer_auth(&system_token)
        .json(&json!({"disabled": true}))
        .send()
        .await
        .expect("disable tenant")
        .error_for_status()
        .expect("tenant disable succeeds")
        .json()
        .await
        .expect("disabled tenant JSON");
    assert_eq!(disabled["disabled"], true);

    let blocked = server
        .client
        .get(format!("{}/api/v1/dashboards", server.base_url))
        .bearer_auth(target_token)
        .send()
        .await
        .expect("use credential for disabled tenant");
    assert_eq!(blocked.status(), StatusCode::FORBIDDEN);

    server
        .client
        .get(format!("{}/api/v1/orgs", server.base_url))
        .bearer_auth(target_token)
        .send()
        .await
        .expect("list organizations from disabled tenant")
        .error_for_status()
        .expect("organization recovery list remains available");

    let enabled: Value = server
        .client
        .patch(format!(
            "{}/api/v1/orgs/{target_org_id}/status",
            server.base_url
        ))
        .bearer_auth(&system_token)
        .json(&json!({"disabled": false}))
        .send()
        .await
        .expect("enable tenant")
        .error_for_status()
        .expect("tenant enable succeeds")
        .json()
        .await
        .expect("enabled tenant JSON");
    assert_eq!(enabled["disabled"], false);

    server
        .client
        .get(format!("{}/api/v1/dashboards", server.base_url))
        .bearer_auth(target_token)
        .send()
        .await
        .expect("reuse credential after tenant enable")
        .error_for_status()
        .expect("existing credential resumes after enable");
}

#[tokio::test]
async fn settings_read_capability_allows_get_and_denies_write() {
    if skip_unless_enabled() {
        return;
    }

    let server = TestServer::start().await;
    let reader = server
        .state
        .iam
        .service
        .create_user(
            "settings-reader@test.example".into(),
            "Settings Reader".into(),
            "readerpass",
        )
        .await
        .expect("create settings reader");
    let role: Value = server
        .client
        .post(format!("{}/api/v1/roles", server.base_url))
        .bearer_auth(&server.root_token)
        .json(&json!({
            "key": "settings_reader",
            "name": "Settings Reader",
            "permissions": ["org.settings.read"]
        }))
        .send()
        .await
        .expect("create settings reader role")
        .error_for_status()
        .expect("settings reader role creation succeeds")
        .json()
        .await
        .expect("settings reader role JSON");
    let role_id = role["id"].as_str().expect("settings reader role id");
    server
        .state
        .iam
        .service
        .iam_memberships
        .upsert(
            IamMembership {
                user_id: reader.id.clone(),
                org_id: server.root_org_id.clone(),
                joined_at: TimestampMicros::now(),
            },
            &[molesignal::shared::ids::Id::from_string(role_id)],
            &server.root_user_id,
        )
        .await
        .expect("assign settings reader role");
    let reader_token = server
        .state
        .iam
        .service
        .issue_token(&reader.id, &server.root_org_id)
        .expect("issue settings reader token");

    server
        .client
        .get(format!("{}/api/v1/settings/signup", server.base_url))
        .bearer_auth(&reader_token)
        .send()
        .await
        .expect("read signup settings")
        .error_for_status()
        .expect("org.settings.read permits settings GET");
    server
        .client
        .get(format!("{}/api/v1/workspace/preferences", server.base_url))
        .bearer_auth(&reader_token)
        .send()
        .await
        .expect("read workspace preference defaults")
        .error_for_status()
        .expect("org.settings.read permits workspace preferences GET");

    let write = server
        .client
        .put(format!("{}/api/v1/settings/signup", server.base_url))
        .bearer_auth(&reader_token)
        .json(&json!({
            "signup_enabled": true,
            "signup_require_approval": true
        }))
        .send()
        .await
        .expect("write signup settings");
    assert_eq!(write.status(), StatusCode::FORBIDDEN);

    let preferences_write = server
        .client
        .put(format!("{}/api/v1/workspace/preferences", server.base_url))
        .bearer_auth(&reader_token)
        .json(&json!({
            "theme": "system",
            "density": "normal",
            "language": "en-us",
            "default_home_route": "/home",
            "time_format": "iso_24h",
            "date_format": "yyyy_mm_dd_dash",
            "timezone": "",
            "keyboard_shortcuts_enabled": true
        }))
        .send()
        .await
        .expect("write workspace preference defaults");
    assert_eq!(preferences_write.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn iam_http_uses_capabilities_for_custom_roles_and_revocation() {
    if skip_unless_enabled() {
        return;
    }

    let server = TestServer::start().await;
    let viewer = server
        .state
        .iam
        .service
        .create_user(
            "iam-viewer@test.example".into(),
            "IAM Viewer".into(),
            "viewerpass",
        )
        .await
        .expect("create viewer");
    let viewer_role_id = server
        .state
        .iam
        .service
        .iam_memberships
        .role_id_for_purpose(&server.root_org_id, "self_service_signup")
        .await
        .expect("resolve signup IAM role");
    server
        .state
        .iam
        .service
        .iam_memberships
        .upsert(
            IamMembership {
                user_id: viewer.id.clone(),
                org_id: server.root_org_id.clone(),
                joined_at: TimestampMicros::now(),
            },
            &[viewer_role_id],
            &server.root_user_id,
        )
        .await
        .expect("create viewer membership");
    let viewer_token = server
        .state
        .iam
        .service
        .issue_token(&viewer.id, &server.root_org_id)
        .expect("issue viewer token");
    let protected_dashboard = server
        .state
        .dashboard
        .create(
            server.root_org_id.clone(),
            None,
            server.root_user_id.clone(),
            dashboard_model("iam-protected", "Protected dashboard"),
        )
        .await
        .expect("create protected dashboard");
    let unbound_dashboard = server
        .state
        .dashboard
        .create(
            server.root_org_id.clone(),
            None,
            server.root_user_id.clone(),
            dashboard_model("iam-unbound", "Unbound dashboard"),
        )
        .await
        .expect("create unbound dashboard");

    let initial_capabilities: Value = server
        .client
        .get(format!("{}/api/v1/iam/capabilities", server.base_url))
        .bearer_auth(&viewer_token)
        .send()
        .await
        .expect("get viewer capabilities")
        .error_for_status()
        .expect("viewer capabilities succeed")
        .json()
        .await
        .expect("viewer capabilities JSON");
    assert_eq!(initial_capabilities["display_role"], "viewer");
    assert!(
        !initial_capabilities["permissions"]
            .as_array()
            .expect("permissions array")
            .iter()
            .any(|permission| permission == "dashboards.edit")
    );
    let route_decisions = initial_capabilities["routes"]
        .as_array()
        .expect("route decisions array");
    assert!(
        route_decisions
            .iter()
            .any(|route| { route["id"] == "dashboards" && route["allowed"] == true })
    );
    assert!(
        route_decisions
            .iter()
            .any(|route| { route["id"] == "dashboard.new.edit" && route["allowed"] == false })
    );
    assert!(
        route_decisions
            .iter()
            .any(|route| { route["id"] == "iam.users" && route["allowed"] == false })
    );

    let catalog_denied = server
        .client
        .get(format!("{}/api/v1/iam/permissions", server.base_url))
        .bearer_auth(&viewer_token)
        .send()
        .await
        .expect("viewer permission catalog request");
    assert_eq!(catalog_denied.status(), StatusCode::FORBIDDEN);

    let role: Value = server
        .client
        .post(format!("{}/api/v1/roles", server.base_url))
        .bearer_auth(&server.root_token)
        .json(&json!({
            "key": "dashboard_curator",
            "name": "Dashboard Curator",
            "permissions": ["dashboards.edit"]
        }))
        .send()
        .await
        .expect("create custom role")
        .error_for_status()
        .expect("custom role creation succeeds")
        .json()
        .await
        .expect("custom role JSON");

    let binding: Value = server
        .client
        .post(format!("{}/api/v1/iam/role-bindings", server.base_url))
        .bearer_auth(&server.root_token)
        .json(&json!({
            "role_id": role["id"],
            "principal_type": "user",
            "principal_id": viewer.id.as_str(),
            "resource_type": "dashboard",
            "resource_id": protected_dashboard.id.as_str(),
            "conditions": {}
        }))
        .send()
        .await
        .expect("create resource role binding")
        .error_for_status()
        .expect("resource role binding succeeds")
        .json()
        .await
        .expect("resource role binding JSON");
    let binding_id = binding["value"]["id"]
        .as_str()
        .expect("binding id")
        .to_owned();

    let allowed =
        evaluate_dashboard_edit(&server, &viewer_token, protected_dashboard.id.as_str()).await;
    assert_eq!(allowed["decisions"][0]["allowed"], true);
    assert_eq!(allowed["decisions"][0]["reason"], "resource_role_binding");

    let routed_update = server
        .client
        .put(format!(
            "{}/api/v1/dashboards/{}",
            server.base_url, protected_dashboard.id
        ))
        .bearer_auth(&viewer_token)
        .json(&json!({
            "model": dashboard_model("iam-protected", "Updated through resource macro"),
            "folder_id": null
        }))
        .send()
        .await
        .expect("update resource-bound dashboard");
    assert_eq!(routed_update.status(), StatusCode::OK);

    let unbound_update = server
        .client
        .put(format!(
            "{}/api/v1/dashboards/{}",
            server.base_url, unbound_dashboard.id
        ))
        .bearer_auth(&viewer_token)
        .json(&json!({
            "model": dashboard_model("iam-unbound", "Must remain denied"),
            "folder_id": null
        }))
        .send()
        .await
        .expect("attempt unbound dashboard update");
    assert_eq!(unbound_update.status(), StatusCode::FORBIDDEN);

    server
        .client
        .delete(format!(
            "{}/api/v1/iam/role-bindings/{binding_id}",
            server.base_url
        ))
        .bearer_auth(&server.root_token)
        .send()
        .await
        .expect("delete resource role binding")
        .error_for_status()
        .expect("resource role binding deletion succeeds");

    let denied =
        evaluate_dashboard_edit(&server, &viewer_token, protected_dashboard.id.as_str()).await;
    assert_eq!(denied["decisions"][0]["allowed"], false);
    assert_eq!(denied["decisions"][0]["reason"], "default_deny");

    let relationship: Value = server
        .client
        .post(format!("{}/api/v1/iam/relationships", server.base_url))
        .bearer_auth(&server.root_token)
        .json(&json!({
            "resource_type": "dashboard",
            "resource_id": protected_dashboard.id.as_str(),
            "role_id": role["id"],
            "subject_type": "user",
            "subject_id": viewer.id.as_str()
        }))
        .send()
        .await
        .expect("create role-backed relationship")
        .error_for_status()
        .expect("role-backed relationship succeeds")
        .json()
        .await
        .expect("role-backed relationship JSON");
    assert_eq!(relationship["value"]["role_id"], role["id"]);

    let relationship_allowed =
        evaluate_dashboard_edit(&server, &viewer_token, protected_dashboard.id.as_str()).await;
    assert_eq!(relationship_allowed["decisions"][0]["allowed"], true);
    assert_eq!(
        relationship_allowed["decisions"][0]["reason"],
        "resource_relationship"
    );

    server
        .client
        .patch(format!(
            "{}/api/v1/roles/{}",
            server.base_url,
            role["id"].as_str().expect("role id")
        ))
        .bearer_auth(&server.root_token)
        .json(&json!({
            "name": "Dashboard Curator",
            "description": "permissions are resolved from the current role",
            "permissions": []
        }))
        .send()
        .await
        .expect("update relationship role")
        .error_for_status()
        .expect("relationship role update succeeds");

    let relationship_revoked =
        evaluate_dashboard_edit(&server, &viewer_token, protected_dashboard.id.as_str()).await;
    assert_eq!(relationship_revoked["decisions"][0]["allowed"], false);
    assert_eq!(
        relationship_revoked["decisions"][0]["reason"],
        "default_deny"
    );
}

#[tokio::test]
async fn cross_org_grant_requires_acceptance_and_stops_after_revocation() {
    if skip_unless_enabled() {
        return;
    }

    let server = TestServer::start().await;
    let shared_dashboard = server
        .state
        .dashboard
        .create(
            server.root_org_id.clone(),
            None,
            server.root_user_id.clone(),
            dashboard_model("cross-org-dashboard", "Cross organization dashboard"),
        )
        .await
        .expect("create cross-organization dashboard");
    let target: Value = server
        .client
        .post(format!("{}/api/v1/orgs", server.base_url))
        .bearer_auth(&server.root_token)
        .json(&json!({"name": "Target", "slug": "target"}))
        .send()
        .await
        .expect("create target organization")
        .error_for_status()
        .expect("target organization creation succeeds")
        .json()
        .await
        .expect("target organization JSON");
    let target_org_id = target["id"].as_str().expect("target org id");
    let target_selection: Value = server
        .client
        .post(format!(
            "{}/api/v1/orgs/{target_org_id}/select",
            server.base_url
        ))
        .bearer_auth(&server.root_token)
        .send()
        .await
        .expect("select target organization")
        .error_for_status()
        .expect("target organization selection succeeds")
        .json()
        .await
        .expect("target organization selection JSON");
    let target_token = target_selection["token"]
        .as_str()
        .expect("target token")
        .to_owned();

    let grant: Value = server
        .client
        .post(format!("{}/api/v1/iam/cross-org-grants", server.base_url))
        .bearer_auth(&server.root_token)
        .json(&json!({
            "target_organization_id": target_org_id,
            "grantee_type": "organization",
            "grantee_id": target_org_id,
            "resource_type": "dashboard",
            "resource_selector": {"ids": [shared_dashboard.id.as_str()]},
            "permissions": ["dashboards.read"],
            "conditions": {}
        }))
        .send()
        .await
        .expect("create cross-org grant")
        .error_for_status()
        .expect("cross-org grant creation succeeds")
        .json()
        .await
        .expect("cross-org grant JSON");
    let grant_id = grant["value"]["id"].as_str().expect("grant id");
    assert_eq!(grant["value"]["status"], "pending");

    let pending =
        evaluate_cross_org_read(&server, &target_token, shared_dashboard.id.as_str()).await;
    assert_eq!(pending["decisions"][0]["allowed"], false);
    assert_eq!(pending["decisions"][0]["reason"], "tenant_isolation");
    let pending_route = server
        .client
        .get(format!(
            "{}/api/v1/dashboards/{}",
            server.base_url, shared_dashboard.id
        ))
        .bearer_auth(&target_token)
        .send()
        .await
        .expect("read pending cross-org dashboard");
    assert_eq!(pending_route.status(), StatusCode::NOT_FOUND);

    server
        .client
        .post(format!(
            "{}/api/v1/iam/cross-org-grants/{grant_id}/accept",
            server.base_url
        ))
        .bearer_auth(&target_token)
        .send()
        .await
        .expect("accept cross-org grant")
        .error_for_status()
        .expect("cross-org grant acceptance succeeds");

    let active =
        evaluate_cross_org_read(&server, &target_token, shared_dashboard.id.as_str()).await;
    assert_eq!(active["decisions"][0]["allowed"], true);
    assert_eq!(active["decisions"][0]["reason"], "cross_organization_grant");
    let active_route = server
        .client
        .get(format!(
            "{}/api/v1/dashboards/{}",
            server.base_url, shared_dashboard.id
        ))
        .bearer_auth(&target_token)
        .send()
        .await
        .expect("read active cross-org dashboard");
    assert_eq!(active_route.status(), StatusCode::OK);

    server
        .client
        .post(format!(
            "{}/api/v1/iam/cross-org-grants/{grant_id}/revoke",
            server.base_url
        ))
        .bearer_auth(&server.root_token)
        .send()
        .await
        .expect("revoke cross-org grant")
        .error_for_status()
        .expect("cross-org grant revocation succeeds");

    let revoked =
        evaluate_cross_org_read(&server, &target_token, shared_dashboard.id.as_str()).await;
    assert_eq!(revoked["decisions"][0]["allowed"], false);
    assert_eq!(revoked["decisions"][0]["reason"], "tenant_isolation");
    let revoked_route = server
        .client
        .get(format!(
            "{}/api/v1/dashboards/{}",
            server.base_url, shared_dashboard.id
        ))
        .bearer_auth(&target_token)
        .send()
        .await
        .expect("read revoked cross-org dashboard");
    assert_eq!(revoked_route.status(), StatusCode::NOT_FOUND);
}

async fn evaluate_dashboard_edit(server: &TestServer, token: &str, resource_id: &str) -> Value {
    server
        .client
        .post(format!("{}/api/v1/iam/evaluate-batch", server.base_url))
        .bearer_auth(token)
        .json(&json!({
            "requests": [{
                "permission": "dashboards.edit",
                "target": {
                    "resource_type": "dashboard",
                    "resource_id": resource_id
                }
            }]
        }))
        .send()
        .await
        .expect("evaluate dashboard edit")
        .error_for_status()
        .expect("dashboard edit evaluation succeeds")
        .json()
        .await
        .expect("dashboard edit evaluation JSON")
}

fn dashboard_model(uid: &str, title: &str) -> Value {
    json!({
        "engine": "molesignal-dashboard",
        "schemaVersion": 2,
        "uid": uid,
        "title": title,
        "tags": [],
        "editable": true,
        "defaultDashboard": false,
        "timeSettings": {
            "defaultFrom": "now-6h",
            "defaultTo": "now",
            "timezone": "browser"
        },
        "refreshSettings": {
            "enabled": false,
            "mode": "off",
            "defaultInterval": "30s",
            "allowedIntervals": ["off", "30s"]
        },
        "variables": [],
        "annotations": [],
        "links": [],
        "layout": {
            "type": "grid",
            "columns": 24,
            "rowHeight": 8,
            "gap": 8
        },
        "elements": []
    })
}

async fn evaluate_cross_org_read(server: &TestServer, token: &str, resource_id: &str) -> Value {
    server
        .client
        .post(format!("{}/api/v1/iam/evaluate-batch", server.base_url))
        .bearer_auth(token)
        .json(&json!({
            "requests": [{
                "permission": "dashboards.read",
                "target": {
                    "organization_id": server.root_org_id.as_str(),
                    "resource_type": "dashboard",
                    "resource_id": resource_id
                }
            }]
        }))
        .send()
        .await
        .expect("evaluate cross-org dashboard read")
        .error_for_status()
        .expect("cross-org dashboard read evaluation succeeds")
        .json()
        .await
        .expect("cross-org dashboard read evaluation JSON")
}
