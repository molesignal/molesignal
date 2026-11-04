// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

mod common;

use common::{Credential, TestServer, session_cookie, skip_unless_enabled, system_session_token};
use molesignal::{
    domain::iam::{
        IamMembership,
        api_token::{ApiToken, ApiTokenKind, ApiTokenScope},
    },
    infra::persistence::repositories::api_tokens::{
        assemble_token, generate_token_parts, hash_secret,
    },
    shared::{ids::Id, time::TimestampMicros},
};
use reqwest::StatusCode;
use serde_json::{Value, json};

#[tokio::test]
async fn creating_service_account_atomically_returns_one_time_bound_api_token() {
    if skip_unless_enabled() {
        return;
    }

    let server = TestServer::start().await;
    let role_id = server
        .state
        .iam
        .service
        .iam_memberships
        .role_id_for_purpose(&server.root_org_id, "self_service_signup")
        .await
        .expect("resolve viewer role");
    let response = server
        .client
        .post(format!("{}/api/v1/service-accounts", server.base_url))
        .credential(&server.root_token)
        .json(&json!({
            "name": "integration-collector",
            "description": "non-interactive integration identity",
            "roleId": role_id,
        }))
        .send()
        .await
        .expect("create service account");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[reqwest::header::CACHE_CONTROL],
        "private, no-store"
    );
    let payload: Value = response.json().await.expect("decode provision response");
    let account_id = payload["serviceAccount"]["id"]
        .as_str()
        .expect("service account id");
    let plaintext = payload["apiToken"]["token"]
        .as_str()
        .expect("one-time API token");
    assert!(plaintext.starts_with("mspat-"));
    assert_eq!(
        payload["apiToken"]["serviceAccountId"].as_str(),
        Some(account_id)
    );

    let accounts = server
        .state
        .iam
        .service_accounts
        .list(&server.root_org_id)
        .await
        .expect("list service accounts");
    let account = accounts
        .iter()
        .find(|account| account.id.0 == account_id)
        .expect("created service account persisted");
    let tokens = server
        .state
        .iam
        .api_tokens
        .list_by_org(&server.root_org_id)
        .await
        .expect("list API tokens");
    let token = tokens
        .iter()
        .find(|token| token.service_account_id.as_ref() == Some(&account.id))
        .expect("initial bound API token persisted");
    assert_eq!(token.token_kind, ApiTokenKind::ServiceAccount);
    assert_eq!(token.scope, ApiTokenScope::Principal);

    server
        .client
        .get(format!("{}/api/v1/iam/capabilities", server.base_url))
        .credential(plaintext)
        .send()
        .await
        .expect("authenticate initial service-account token")
        .error_for_status()
        .expect("initial service-account token is usable");

    let listed = server
        .client
        .get(format!("{}/api/v1/auth/tokens", server.base_url))
        .credential(&server.root_token)
        .send()
        .await
        .expect("list API token metadata")
        .error_for_status()
        .expect("list API token metadata succeeds")
        .text()
        .await
        .expect("read API token metadata");
    assert!(!listed.contains(plaintext));
}

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
    let (prefix, secret) = generate_token_parts();
    let api_token = assemble_token(&prefix, &secret);
    server
        .state
        .iam
        .api_tokens
        .create(ApiToken {
            id: Id::new(),
            prefix,
            secret_hash: hash_secret(&secret),
            org_id: server.root_org_id.clone(),
            user_id: user.id.clone(),
            scope: ApiTokenScope::Principal,
            name: "disabled-user-token".into(),
            expires_at: None,
            last_used_at: None,
            revoked: false,
            created_at: TimestampMicros::now(),
            is_default: false,
            token_kind: ApiTokenKind::Personal,
            application_id: None,
            service_account_id: None,
        })
        .await
        .expect("create API token");

    for credential in [&jwt, &api_token] {
        server
            .client
            .get(format!("{}/api/v1/iam/capabilities", server.base_url))
            .credential(credential)
            .send()
            .await
            .expect("request capabilities before disable")
            .error_for_status()
            .expect("credential works before disable");
    }

    server
        .client
        .patch(format!("{}/api/v1/users/{}", server.base_url, user.id))
        .credential(&server.root_token)
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
            .credential(credential)
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
        .credential(&server.root_token)
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
        .connect(&server.settings.meta_database.dsn)
        .await
        .expect("connect test database");
    let system_token = system_session_token(&server.state, &server.root_user_id);
    let capabilities: Value = server
        .client
        .get(format!("{}/api/v1/iam/capabilities", server.base_url))
        .credential(&system_token)
        .send()
        .await
        .expect("get system capabilities")
        .error_for_status()
        .expect("system capabilities succeed")
        .json()
        .await
        .expect("system capabilities JSON");

    assert_eq!(capabilities["displayRole"], "Super administrator");
    assert_eq!(capabilities["roles"][0]["key"], "super_administrator");
    assert!(
        capabilities["permissions"]
            .as_array()
            .expect("permissions array")
            .iter()
            .any(|permission| permission == "sys.telemetry.read")
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
            .any(|permission| permission == "sys.organizations.read")
    );
    assert!(
        capabilities["permissions"]
            .as_array()
            .expect("permissions array")
            .iter()
            .any(|permission| permission == "sys.telemetry.manage")
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
        .credential(&system_token)
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

    for dashboard in dashboards {
        assert_eq!(dashboard["orgId"], server.state.iam.system_org_id.as_str());
        assert_eq!(dashboard["model"]["editable"], false);
    }

    let overview = dashboards
        .iter()
        .find(|dashboard| dashboard["uid"] == "molesignal-system-overview")
        .expect("the built-in system overview");

    assert_eq!(overview["model"]["defaultDashboard"], true);
    let dashboard_id = overview["id"].as_str().expect("built-in dashboard id");

    let system_dashboard = server
        .client
        .get(format!(
            "{}/api/v1/dashboards/{dashboard_id}",
            server.base_url
        ))
        .credential(&system_token)
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
        .credential(&server.root_token)
        .send()
        .await
        .expect("attempt tenant read of built-in system dashboard");
    assert_eq!(tenant_cross_org_read.status(), StatusCode::NOT_FOUND);

    let system_folders = server
        .client
        .get(format!("{}/api/v1/folders", server.base_url))
        .credential(&system_token)
        .send()
        .await
        .expect("list system dashboard folders");
    assert_eq!(system_folders.status(), StatusCode::OK);

    let denied_system_create = server
        .client
        .post(format!("{}/api/v1/dashboards", server.base_url))
        .credential(&system_token)
        .json(&json!({"model": dashboard_model("system-copy", "System copy")}))
        .send()
        .await
        .expect("attempt dashboard creation in system scope");
    assert_eq!(denied_system_create.status(), StatusCode::FORBIDDEN);

    let read_response = server
        .client
        .get(format!("{}/api/v1/system/telemetry", server.base_url))
        .credential(&system_token)
        .send()
        .await
        .expect("read trace policy with database permission");
    assert_eq!(read_response.status(), StatusCode::OK);

    let organizations: Value = server
        .client
        .get(format!("{}/api/v1/orgs", server.base_url))
        .credential(&server.root_token)
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
    assert_eq!(system["displayRole"], "Super administrator");
    assert_eq!(system["roles"][0]["key"], "super_administrator");

    let system_organizations: Value = server
        .client
        .get(format!("{}/api/v1/orgs", server.base_url))
        .credential(&system_token)
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
    assert_eq!(root_organization["displayRole"], "Super administrator");
    assert_eq!(root_organization["roles"][0]["key"], "super_administrator");

    let created_organization: Value = server
        .client
        .post(format!("{}/api/v1/orgs", server.base_url))
        .credential(&system_token)
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
    assert_eq!(created_organization["displayRole"], "Super administrator");
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
        .credential(&system_token)
        .send()
        .await
        .expect("select membership-free organization")
        .error_for_status()
        .expect("root can select every organization")
        .json()
        .await
        .expect("selected organization JSON");
    assert_eq!(selected["displayRole"], "Super administrator");
}

#[tokio::test]
async fn organization_owner_holds_the_organization_catalog_and_stays_out_of_the_platform() {
    if skip_unless_enabled() {
        return;
    }

    let server = TestServer::start().await;
    let memberships = &server.state.iam.service.iam_memberships;

    // Two ordinary users in the root tenant: one made an owner, one left a viewer.
    let mut tokens = Vec::new();

    for (email, purpose) in [
        ("second-owner@test.example", "organization_bootstrap"),
        ("plain-viewer@test.example", "self_service_signup"),
    ] {
        let user = server
            .state
            .iam
            .service
            .create_user(email.into(), email.into(), "userpass")
            .await
            .expect("create user");
        let role_id = memberships
            .role_id_for_purpose(&server.root_org_id, purpose)
            .await
            .expect("resolve role");
        memberships
            .upsert(
                IamMembership {
                    user_id: user.id.clone(),
                    org_id: server.root_org_id.clone(),
                    joined_at: TimestampMicros::now(),
                },
                &[role_id],
                &server.root_user_id,
            )
            .await
            .expect("create membership");
        let token = server
            .state
            .iam
            .service
            .issue_token(&user.id, &server.root_org_id)
            .expect("issue JWT");
        tokens.push((user.id, token));
    }

    let (owner_id, owner_token) = tokens[0].clone();
    let (_, viewer_token) = tokens[1].clone();

    let get = |path: &'static str, token: String| {
        let request = server
            .client
            .get(format!("{}/api/v1{path}", server.base_url))
            .credential(token);
        async move {
            request
                .send()
                .await
                .expect("request")
                .error_for_status()
                .expect("request succeeds")
                .json::<Value>()
                .await
                .expect("JSON")
        }
    };
    let keys = |value: &Value| {
        let mut keys = value
            .as_array()
            .expect("array")
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| item["key"].as_str().expect("key").to_owned())
            })
            .collect::<Vec<_>>();
        keys.sort();
        keys
    };

    // The owner holds every organization permission of the catalog and none of the platform's, and
    // so does root in an organization: a platform permission counts in the platform only.
    let catalog = get("/iam/permissions", owner_token.clone()).await;
    let every_key = keys(&catalog["permissions"]);
    assert!(every_key.iter().any(|key| key == "sys.organizations.read"));
    let organization_keys = every_key
        .iter()
        .filter(|key| !key.starts_with("sys."))
        .cloned()
        .collect::<Vec<_>>();
    assert!(organization_keys.iter().any(|key| key == "org.delete"));
    let owner_capabilities = get("/iam/capabilities", owner_token.clone()).await;
    assert_eq!(keys(&owner_capabilities["permissions"]), organization_keys);
    let root_capabilities = get("/iam/capabilities", server.root_token.clone()).await;
    assert_eq!(keys(&root_capabilities["permissions"]), organization_keys);

    let roles = get("/roles", owner_token.clone()).await;
    let owner_role = roles
        .as_array()
        .expect("roles array")
        .iter()
        .find(|role| role["key"] == "owner")
        .expect("owner role");
    assert_eq!(keys(&owner_role["permissions"]), organization_keys);

    // Neither an owner nor a viewer can see or enter the platform organization.
    let system_org_id = server.state.iam.system_org_id.as_str().to_owned();
    for token in [&owner_token, &viewer_token] {
        let organizations = get("/orgs", token.clone()).await;
        assert!(
            organizations
                .as_array()
                .expect("organizations array")
                .iter()
                .all(|organization| organization["system"] == false)
        );

        let select = server
            .client
            .post(format!(
                "{}/api/v1/orgs/{system_org_id}/select",
                server.base_url
            ))
            .credential(token)
            .send()
            .await
            .expect("select the platform organization");
        assert_eq!(select.status(), StatusCode::NOT_FOUND);
    }

    // What covers the whole instance is out of the owner's reach, from its own organization too.
    for (method, path, body) in [
        (
            "PUT",
            "/settings/stream_retention",
            json!({"retentionDays": 1}),
        ),
        (
            "PUT",
            "/settings/audit_retention",
            json!({"retentionDays": 1}),
        ),
        (
            "PUT",
            "/settings/signup",
            json!({"signupEnabled": true, "signupRequireApproval": false}),
        ),
        ("POST", "/node/drain", json!({})),
        ("GET", "/clusters", json!({})),
    ] {
        let response = server
            .client
            .request(
                method.parse().expect("HTTP method"),
                format!("{}/api/v1{path}", server.base_url),
            )
            .credential(&owner_token)
            .json(&body)
            .send()
            .await
            .expect("request an instance-wide endpoint");
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{method} {path}");
    }

    // Another organization, made by root: the owner of the first can neither delete nor disable it.
    server
        .state
        .iam
        .platform_administrators
        .bootstrap_root(&server.root_user_id)
        .await
        .expect("reconcile root platform administrator");
    let platform_token = system_session_token(&server.state, &server.root_user_id);
    let other: Value = server
        .client
        .post(format!("{}/api/v1/orgs", server.base_url))
        .credential(&platform_token)
        .json(&json!({"name": "Other tenant", "slug": "other-tenant"}))
        .send()
        .await
        .expect("create another organization")
        .error_for_status()
        .expect("root creates an organization")
        .json()
        .await
        .expect("created organization JSON");
    let other_id = other["id"].as_str().expect("created organization id");
    let foreign_delete = server
        .client
        .delete(format!("{}/api/v1/orgs/{other_id}", server.base_url))
        .credential(&owner_token)
        .send()
        .await
        .expect("delete another organization");
    assert_eq!(foreign_delete.status(), StatusCode::FORBIDDEN);
    let foreign_status = server
        .client
        .patch(format!("{}/api/v1/orgs/{other_id}/status", server.base_url))
        .credential(&owner_token)
        .json(&json!({"disabled": true}))
        .send()
        .await
        .expect("disable another organization");
    assert_eq!(foreign_status.status(), StatusCode::FORBIDDEN);

    // An owner there deletes that one from inside it; a viewer there cannot.
    let owner_of_other = Id::from_string(owner_id.0.clone());
    let owner_role_id = memberships
        .role_id_for_purpose(&Id::from_string(other_id), "organization_bootstrap")
        .await
        .expect("resolve owner role");
    memberships
        .upsert(
            IamMembership {
                user_id: owner_of_other.clone(),
                org_id: Id::from_string(other_id),
                joined_at: TimestampMicros::now(),
            },
            &[owner_role_id],
            &server.root_user_id,
        )
        .await
        .expect("make the owner a member of the other organization");
    let inside_token = server
        .state
        .iam
        .service
        .issue_token(&owner_of_other, &Id::from_string(other_id))
        .expect("issue JWT in the other organization");
    let viewer_delete = server
        .client
        .delete(format!(
            "{}/api/v1/orgs/{}",
            server.base_url, server.root_org_id
        ))
        .credential(&viewer_token)
        .send()
        .await
        .expect("viewer deletes its organization");
    assert_eq!(viewer_delete.status(), StatusCode::FORBIDDEN);
    let own_delete = server
        .client
        .delete(format!("{}/api/v1/orgs/{other_id}", server.base_url))
        .credential(&inside_token)
        .send()
        .await
        .expect("owner deletes its organization");
    assert_eq!(own_delete.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn nobody_hands_out_or_acts_on_more_than_they_hold() {
    if skip_unless_enabled() {
        return;
    }

    let server = TestServer::start().await;
    let memberships = &server.state.iam.service.iam_memberships;

    let mut tokens = Vec::new();

    for (email, purpose) in [
        ("an-admin@test.example", None),
        ("an-owner@test.example", Some("organization_bootstrap")),
        ("a-viewer@test.example", Some("self_service_signup")),
    ] {
        let user = server
            .state
            .iam
            .service
            .create_user(email.into(), email.into(), "userpass")
            .await
            .expect("create user");
        let role_id = match purpose {
            Some(purpose) => memberships
                .role_id_for_purpose(&server.root_org_id, purpose)
                .await
                .expect("resolve role"),
            None => {
                let roles: Value = server
                    .client
                    .get(format!("{}/api/v1/roles", server.base_url))
                    .credential(&server.root_token)
                    .send()
                    .await
                    .expect("list roles")
                    .error_for_status()
                    .expect("roles succeed")
                    .json()
                    .await
                    .expect("roles JSON");
                let admin = roles
                    .as_array()
                    .expect("roles array")
                    .iter()
                    .find(|role| role["key"] == "admin")
                    .expect("admin role");
                Id::from_string(admin["id"].as_str().expect("admin role id"))
            }
        };
        memberships
            .upsert(
                IamMembership {
                    user_id: user.id.clone(),
                    org_id: server.root_org_id.clone(),
                    joined_at: TimestampMicros::now(),
                },
                std::slice::from_ref(&role_id),
                &server.root_user_id,
            )
            .await
            .expect("create membership");
        let token = server
            .state
            .iam
            .service
            .issue_token(&user.id, &server.root_org_id)
            .expect("issue JWT");
        tokens.push((user.id, role_id, token));
    }

    let (admin_id, admin_role, admin_token) = tokens[0].clone();
    let (owner_id, owner_role, owner_token) = tokens[1].clone();
    let (viewer_id, viewer_role, _) = tokens[2].clone();
    let members_url = format!(
        "{}/api/v1/orgs/{}/members",
        server.base_url, server.root_org_id
    );
    let set_roles = |token: String, user: &Id, role: &Id| {
        server
            .client
            .post(&members_url)
            .credential(&token)
            .json(&json!({"userId": user.0, "roleIds": [role.0]}))
            .send()
    };

    // An Admin lacks org.delete, so it can neither promote itself or anyone to Owner...
    for target in [&admin_id, &viewer_id] {
        let promoted = set_roles(admin_token.clone(), target, &owner_role)
            .await
            .expect("promote to owner");
        assert_eq!(promoted.status(), StatusCode::FORBIDDEN);
    }

    // ...nor demote, remove or disable an Owner...
    let demoted = set_roles(admin_token.clone(), &owner_id, &viewer_role)
        .await
        .expect("demote the owner");
    assert_eq!(demoted.status(), StatusCode::FORBIDDEN);
    let removed = server
        .client
        .delete(format!("{members_url}/{}", owner_id.0))
        .credential(&admin_token)
        .send()
        .await
        .expect("remove the owner");
    assert_eq!(removed.status(), StatusCode::FORBIDDEN);
    let disabled = server
        .client
        .patch(format!("{}/api/v1/users/{}", server.base_url, owner_id.0))
        .credential(&admin_token)
        .json(&json!({"disabled": true}))
        .send()
        .await
        .expect("disable the owner");
    assert_eq!(disabled.status(), StatusCode::FORBIDDEN);

    // ...nor write org.delete into a role or a policy, nor invite anyone as Owner.
    let role = server
        .client
        .post(format!("{}/api/v1/roles", server.base_url))
        .credential(&admin_token)
        .json(&json!({"key": "escalator", "name": "Escalator", "permissions": ["org.delete"]}))
        .send()
        .await
        .expect("create a role beyond the caller");
    assert_eq!(role.status(), StatusCode::FORBIDDEN);
    let invitation = server
        .client
        .post(format!("{}/api/v1/invitations", server.base_url))
        .credential(&admin_token)
        .json(&json!({"email": "someone@test.example", "roleId": owner_role.0}))
        .send()
        .await
        .expect("invite as owner");
    assert_eq!(invitation.status(), StatusCode::FORBIDDEN);

    // What the Admin does hold stays its to give, and an Owner can give anything here.
    let same_level = set_roles(admin_token.clone(), &viewer_id, &admin_role)
        .await
        .expect("promote to admin");
    assert!(same_level.status().is_success());
    let by_owner = set_roles(owner_token.clone(), &viewer_id, &owner_role)
        .await
        .expect("owner promotes to owner");
    assert!(by_owner.status().is_success());
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
        .credential(&server.root_token)
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

    for (immutable_patch, refused) in [
        (
            json!({"slug": "renamed-workspace"}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"id": "replacement-id", "name": "Renamed"}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
    ] {
        let response = server
            .client
            .patch(format!(
                "{}/api/v1/orgs/{}",
                server.base_url, server.root_org_id
            ))
            .credential(&server.root_token)
            .json(&immutable_patch)
            .send()
            .await
            .expect("patch immutable organization field");
        assert_eq!(response.status(), refused);
    }

    let renamed: Value = server
        .client
        .patch(format!(
            "{}/api/v1/orgs/{}",
            server.base_url, server.root_org_id
        ))
        .credential(&server.root_token)
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
        .credential(&server.root_token)
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
    let system_token = system_session_token(&server.state, &server.root_user_id);

    let delete_last = server
        .client
        .delete(format!(
            "{}/api/v1/orgs/{}",
            server.base_url, server.root_org_id
        ))
        .credential(&system_token)
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
        .credential(&system_token)
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
        .credential(&system_token)
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
        .credential(&system_token)
        .json(&json!({
            "userId": server.root_user_id.as_str(),
            "roleIds": []
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
    let system_token = system_session_token(&server.state, &server.root_user_id);

    let disable_system = server
        .client
        .patch(format!(
            "{}/api/v1/orgs/{}/status",
            server.base_url, server.state.iam.system_org_id
        ))
        .credential(&system_token)
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
        .credential(&system_token)
        .json(&json!({"disabled": true}))
        .send()
        .await
        .expect("attempt to disable last enabled tenant");
    assert_eq!(disable_last_enabled.status(), StatusCode::BAD_REQUEST);

    let created: Value = server
        .client
        .post(format!("{}/api/v1/orgs", server.base_url))
        .credential(&system_token)
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

    let selection = server
        .client
        .post(format!(
            "{}/api/v1/orgs/{target_org_id}/select",
            server.base_url
        ))
        .credential(&server.root_token)
        .send()
        .await
        .expect("select target tenant")
        .error_for_status()
        .expect("target tenant selection succeeds");
    let target_token = &session_cookie(&selection);

    let disabled: Value = server
        .client
        .patch(format!(
            "{}/api/v1/orgs/{target_org_id}/status",
            server.base_url
        ))
        .credential(&system_token)
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
        .credential(target_token)
        .send()
        .await
        .expect("use credential for disabled tenant");
    assert_eq!(blocked.status(), StatusCode::FORBIDDEN);

    server
        .client
        .get(format!("{}/api/v1/orgs", server.base_url))
        .credential(target_token)
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
        .credential(&system_token)
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
        .credential(target_token)
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
        .credential(&server.root_token)
        .json(&json!({
            "key": "settings_reader",
            "name": "Settings Reader",
            "permissions": ["org.preferences.read"]
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

    // The signup policy covers the whole instance: it is no organization's to read.
    let signup = server
        .client
        .get(format!("{}/api/v1/settings/signup", server.base_url))
        .credential(&reader_token)
        .send()
        .await
        .expect("read signup settings");
    assert_eq!(signup.status(), StatusCode::FORBIDDEN);
    server
        .client
        .get(format!("{}/api/v1/workspace/preferences", server.base_url))
        .credential(&reader_token)
        .send()
        .await
        .expect("read workspace preference defaults")
        .error_for_status()
        .expect("org.preferences.read permits workspace preferences GET");

    let write = server
        .client
        .put(format!("{}/api/v1/settings/signup", server.base_url))
        .credential(&reader_token)
        .json(&json!({
            "signupEnabled": true,
            "signupRequireApproval": true
        }))
        .send()
        .await
        .expect("write signup settings");
    assert_eq!(write.status(), StatusCode::FORBIDDEN);

    let preferences_write = server
        .client
        .put(format!("{}/api/v1/workspace/preferences", server.base_url))
        .credential(&reader_token)
        .json(&json!({
            "theme": "system",
            "density": "normal",
            "language": "en-us",
            "defaultHomeRoute": "/home",
            "timeFormat": "iso_24h",
            "dateFormat": "yyyy_mm_dd_dash",
            "timezone": "",
            "keyboardShortcutsEnabled": true
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
        .credential(&viewer_token)
        .send()
        .await
        .expect("get viewer capabilities")
        .error_for_status()
        .expect("viewer capabilities succeed")
        .json()
        .await
        .expect("viewer capabilities JSON");
    assert_eq!(initial_capabilities["displayRole"], "Viewer");
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
        .credential(&viewer_token)
        .send()
        .await
        .expect("viewer permission catalog request");
    assert_eq!(catalog_denied.status(), StatusCode::FORBIDDEN);

    let role: Value = server
        .client
        .post(format!("{}/api/v1/roles", server.base_url))
        .credential(&server.root_token)
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
        .credential(&server.root_token)
        .json(&json!({
            "roleId": role["id"],
            "principalType": "user",
            "principalId": viewer.id.as_str(),
            "resourceType": "dashboard",
            "resourceId": protected_dashboard.id.as_str(),
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
        .credential(&viewer_token)
        .json(&json!({
            "model": dashboard_model("iam-protected", "Updated through resource macro"),
            "folderId": null
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
        .credential(&viewer_token)
        .json(&json!({
            "model": dashboard_model("iam-unbound", "Must remain denied"),
            "folderId": null
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
        .credential(&server.root_token)
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
        .credential(&server.root_token)
        .json(&json!({
            "resourceType": "dashboard",
            "resourceId": protected_dashboard.id.as_str(),
            "roleId": role["id"],
            "subjectType": "user",
            "subjectId": viewer.id.as_str()
        }))
        .send()
        .await
        .expect("create role-backed relationship")
        .error_for_status()
        .expect("role-backed relationship succeeds")
        .json()
        .await
        .expect("role-backed relationship JSON");
    assert_eq!(relationship["value"]["roleId"], role["id"]);

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
        .credential(&server.root_token)
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
    server
        .state
        .iam
        .platform_administrators
        .bootstrap_root(&server.root_user_id)
        .await
        .expect("reconcile root platform administrator");
    let target: Value = server
        .client
        .post(format!("{}/api/v1/orgs", server.base_url))
        .credential(system_session_token(&server.state, &server.root_user_id))
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
    let target_selection = server
        .client
        .post(format!(
            "{}/api/v1/orgs/{target_org_id}/select",
            server.base_url
        ))
        .credential(&server.root_token)
        .send()
        .await
        .expect("select target organization")
        .error_for_status()
        .expect("target organization selection succeeds");
    let target_token = session_cookie(&target_selection);

    let grant: Value = server
        .client
        .post(format!("{}/api/v1/iam/cross-org-grants", server.base_url))
        .credential(&server.root_token)
        .json(&json!({
            "targetOrganizationId": target_org_id,
            "granteeType": "organization",
            "granteeId": target_org_id,
            "resourceType": "dashboard",
            "resourceSelector": {"ids": [shared_dashboard.id.as_str()]},
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
        .credential(&target_token)
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
        .credential(&target_token)
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
        .credential(&target_token)
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
        .credential(&server.root_token)
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
        .credential(&target_token)
        .send()
        .await
        .expect("read revoked cross-org dashboard");
    assert_eq!(revoked_route.status(), StatusCode::NOT_FOUND);
}

async fn evaluate_dashboard_edit(server: &TestServer, token: &str, resource_id: &str) -> Value {
    server
        .client
        .post(format!("{}/api/v1/iam/evaluate-batch", server.base_url))
        .credential(token)
        .json(&json!({
            "requests": [{
                "permission": "dashboards.edit",
                "target": {
                    "resourceType": "dashboard",
                    "resourceId": resource_id
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
        .credential(token)
        .json(&json!({
            "requests": [{
                "permission": "dashboards.read",
                "target": {
                    "organizationId": server.root_org_id.as_str(),
                    "resourceType": "dashboard",
                    "resourceId": resource_id
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

#[tokio::test]
async fn organization_administrators_reach_only_accounts_of_their_own_organization() {
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
    let other: Value = server
        .client
        .post(format!("{}/api/v1/orgs", server.base_url))
        .credential(system_session_token(&server.state, &server.root_user_id))
        .json(&json!({"name": "Other", "slug": "other"}))
        .send()
        .await
        .expect("create other organization")
        .error_for_status()
        .expect("other organization creation succeeds")
        .json()
        .await
        .expect("other organization JSON");
    let other_org_id = Id::from_string(other["id"].as_str().expect("other org id"));

    let admin = member_with_role(
        &server,
        "org-admin@test.example",
        &server.root_org_id,
        "organization_bootstrap",
    )
    .await;
    let outsider = member_with_role(
        &server,
        "outsider@test.example",
        &other_org_id,
        "self_service_signup",
    )
    .await;
    let shared = member_with_role(
        &server,
        "shared@test.example",
        &server.root_org_id,
        "self_service_signup",
    )
    .await;
    join_with_role(&server, &shared, &other_org_id, "self_service_signup").await;

    let admin_token = server
        .state
        .iam
        .service
        .issue_token(&admin, &server.root_org_id)
        .expect("issue administrator JWT");
    let users = format!("{}/api/v1/users", server.base_url);
    let outsider_url = format!("{users}/{}", outsider.as_str());

    let requests = [
        server.client.get(&outsider_url),
        server
            .client
            .patch(&outsider_url)
            .json(&json!({"displayName": "Renamed"})),
        server
            .client
            .patch(&outsider_url)
            .json(&json!({"disabled": true})),
        server.client.delete(&outsider_url),
        server.client.post(format!("{outsider_url}/approve")),
        server.client.post(format!("{outsider_url}/reject")),
    ];

    for request in requests {
        let response = request
            .credential(&admin_token)
            .send()
            .await
            .expect("reach another organization's account");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    let untouched = server
        .state
        .iam
        .service
        .users
        .get(&outsider)
        .await
        .expect("the other organization's account still exists");
    assert!(!untouched.disabled);
    assert_eq!(untouched.display_name, "outsider@test.example");

    // A member shared with another organization can be read here, but changing the account would
    // change it for the other organization too.
    let shared_url = format!("{users}/{}", shared.as_str());
    let read = server
        .client
        .get(&shared_url)
        .credential(&admin_token)
        .send()
        .await
        .expect("read shared member");
    assert_eq!(read.status(), StatusCode::OK);

    let disable = server
        .client
        .patch(&shared_url)
        .credential(&admin_token)
        .json(&json!({"disabled": true}))
        .send()
        .await
        .expect("disable shared member");
    assert_eq!(disable.status(), StatusCode::FORBIDDEN);

    let delete = server
        .client
        .delete(&shared_url)
        .credential(&admin_token)
        .send()
        .await
        .expect("delete shared member");
    assert_eq!(delete.status(), StatusCode::FORBIDDEN);

    // The platform administrator may change a shared account.
    server
        .client
        .patch(&shared_url)
        .credential(&server.root_token)
        .json(&json!({"disabled": true}))
        .send()
        .await
        .expect("platform administrator disables shared member")
        .error_for_status()
        .expect("platform administrator may disable a shared member");
}

#[tokio::test]
async fn members_read_each_others_names_and_the_permission_reads_the_rest() {
    if skip_unless_enabled() {
        return;
    }

    let server = TestServer::start().await;
    let iam = &server.state.iam.service;

    let viewer = iam
        .create_user("viewer@test.example".into(), "Vera".into(), "userpass")
        .await
        .expect("create viewer")
        .id;

    join_with_role(&server, &viewer, &server.root_org_id, "self_service_signup").await;

    let stranger = iam
        .create_user("stranger@test.example".into(), "Sam".into(), "userpass")
        .await
        .expect("create stranger")
        .id;

    let viewer_session = iam
        .issue_token(&viewer, &server.root_org_id)
        .expect("issue viewer JWT");

    let users = format!("{}/api/v1/users", server.base_url);

    let names: Value = server
        .client
        .get(&users)
        .credential(&viewer_session)
        .send()
        .await
        .expect("list members as a viewer")
        .error_for_status()
        .expect("a viewer reads the names")
        .json()
        .await
        .expect("names JSON");

    let names = names.as_array().expect("a list of names");

    assert!(names.contains(&json!({
        "id": viewer.as_str(),
        "displayName": "Vera",
        "avatarUrl": null,
    })));

    assert!(names.iter().all(|name| name.get("email").is_none()));
    assert!(names.iter().all(|name| name["id"] != stranger.as_str()));

    let members: Value = server
        .client
        .get(&users)
        .credential(&server.root_token)
        .send()
        .await
        .expect("list members as the owner")
        .error_for_status()
        .expect("the owner reads the members")
        .json()
        .await
        .expect("members JSON");

    let vera = members
        .as_array()
        .expect("a list of members")
        .iter()
        .find(|member| member["id"] == viewer.as_str())
        .expect("the viewer is a member");

    assert_eq!(vera["email"], "viewer@test.example");

    // A token acts for a program: without the permission it reads nothing, not even names.
    let (prefix, secret) = generate_token_parts();
    let viewer_api_token = assemble_token(&prefix, &secret);

    server
        .state
        .iam
        .api_tokens
        .create(ApiToken {
            id: Id::new(),
            prefix,
            secret_hash: hash_secret(&secret),
            org_id: server.root_org_id.clone(),
            user_id: viewer.clone(),
            scope: ApiTokenScope::Principal,
            name: "viewer-token".into(),
            expires_at: None,
            last_used_at: None,
            revoked: false,
            created_at: TimestampMicros::now(),
            is_default: false,
            token_kind: ApiTokenKind::Personal,
            application_id: None,
            service_account_id: None,
        })
        .await
        .expect("create API token");

    let refused = server
        .client
        .get(&users)
        .credential(&viewer_api_token)
        .send()
        .await
        .expect("list members with a token");

    assert_eq!(refused.status(), StatusCode::FORBIDDEN);
}

async fn member_with_role(server: &TestServer, email: &str, org_id: &Id, purpose: &str) -> Id {
    let user = server
        .state
        .iam
        .service
        .create_user(email.into(), email.into(), "userpass")
        .await
        .expect("create user");

    join_with_role(server, &user.id, org_id, purpose).await;
    user.id
}

async fn join_with_role(server: &TestServer, user_id: &Id, org_id: &Id, purpose: &str) {
    let role_id = server
        .state
        .iam
        .service
        .iam_memberships
        .role_id_for_purpose(org_id, purpose)
        .await
        .expect("resolve role");

    server
        .state
        .iam
        .service
        .iam_memberships
        .upsert(
            IamMembership {
                user_id: user_id.clone(),
                org_id: org_id.clone(),
                joined_at: TimestampMicros::now(),
            },
            &[role_id],
            &server.root_user_id,
        )
        .await
        .expect("create membership");
}

/// A session of the platform organization for an account root granted something to, through the
/// way in a browser takes.
async fn platform_session_of(server: &TestServer, token: &str) -> String {
    let selection = server
        .client
        .post(format!(
            "{}/api/v1/orgs/{}/select",
            server.base_url, server.state.iam.system_org_id
        ))
        .credential(token)
        .send()
        .await
        .expect("enter the platform organization")
        .error_for_status()
        .expect("the grant lets the account in");

    session_cookie(&selection)
}

#[tokio::test]
async fn who_holds_what_on_the_platform_is_configured_by_roles_and_bindings() {
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

    let root = system_session_token(&server.state, &server.root_user_id);
    let url = |path: &str| format!("{}/api/v1{path}", server.base_url);

    // An account that belongs to no organization but the root's, with no role there.
    let keeper = server
        .state
        .iam
        .service
        .create_user(
            "retention-keeper@test.example".into(),
            "Retention keeper".into(),
            "userpass",
        )
        .await
        .expect("create account");

    // Signed in somewhere, with no role there.
    server
        .state
        .iam
        .service
        .iam_memberships
        .upsert(
            IamMembership {
                user_id: keeper.id.clone(),
                org_id: server.root_org_id.clone(),
                joined_at: TimestampMicros::now(),
            },
            &[],
            &server.root_user_id,
        )
        .await
        .expect("join with no role");

    let keeper_token = server
        .state
        .iam
        .service
        .issue_token(&keeper.id, &server.root_org_id)
        .expect("issue JWT");

    // A platform role holds platform permissions, and nothing of an organization's.
    let role: Value = server
        .client
        .post(url("/roles"))
        .credential(&root)
        .json(&json!({
            "key": "retention_keeper",
            "name": "Retention keeper",
            "permissions": ["sys.settings.retention.read", "sys.settings.retention.manage"],
        }))
        .send()
        .await
        .expect("create platform role")
        .error_for_status()
        .expect("root creates a platform role")
        .json()
        .await
        .expect("role JSON");

    assert_eq!(role["scope"], "platform");
    assert_eq!(
        role["effectivePermissions"].as_array().map(Vec::len),
        Some(2)
    );

    let across_scopes = server
        .client
        .post(url("/roles"))
        .credential(&root)
        .json(&json!({"key": "mixed", "name": "Mixed", "permissions": ["dashboards.read"]}))
        .send()
        .await
        .expect("create a role across scopes");

    assert_eq!(across_scopes.status(), StatusCode::FORBIDDEN);

    // The built-in policies are an organization's: they are not offered on the platform.
    let policies: Value = server
        .client
        .get(url("/iam/policies"))
        .credential(&root)
        .send()
        .await
        .expect("list platform policies")
        .error_for_status()
        .expect("root lists platform policies")
        .json()
        .await
        .expect("policies JSON");

    assert_eq!(policies, json!([]));

    // The account is found by its email, without belonging to the platform organization.
    let found: Value = server
        .client
        .get(url("/iam/platform/accounts?q=retention-keeper%40"))
        .credential(&root)
        .send()
        .await
        .expect("find account")
        .error_for_status()
        .expect("root finds accounts")
        .json()
        .await
        .expect("accounts JSON");

    assert_eq!(found[0]["id"], keeper.id.0.as_str());

    // Before the grant it cannot enter, and is not told the organization exists.
    let refused = server
        .client
        .post(url(&format!(
            "/orgs/{}/select",
            server.state.iam.system_org_id
        )))
        .credential(&keeper_token)
        .send()
        .await
        .expect("enter without a grant");

    assert_eq!(refused.status(), StatusCode::NOT_FOUND);

    let binding: Value = server
        .client
        .post(url("/iam/role-bindings"))
        .credential(&root)
        .json(&json!({
            "roleId": role["id"],
            "principalType": "user",
            "principalId": keeper.id.0,
        }))
        .send()
        .await
        .expect("grant the role")
        .error_for_status()
        .expect("root grants the role")
        .json()
        .await
        .expect("binding JSON");

    // With it, the account holds exactly what the role does.
    let session = platform_session_of(&server, &keeper_token).await;

    let capabilities: Value = server
        .client
        .get(url("/iam/capabilities"))
        .credential(&session)
        .send()
        .await
        .expect("get capabilities")
        .error_for_status()
        .expect("capabilities succeed")
        .json()
        .await
        .expect("capabilities JSON");

    assert_eq!(
        capabilities["permissions"],
        json!([
            "sys.settings.retention.manage",
            "sys.settings.retention.read"
        ])
    );
    assert_eq!(capabilities["displayRole"], "Retention keeper");

    for (path, status) in [
        ("/settings/stream_retention", StatusCode::OK),
        ("/settings/series_limits", StatusCode::FORBIDDEN),
        ("/clusters", StatusCode::FORBIDDEN),
        ("/roles", StatusCode::FORBIDDEN),
    ] {
        let response = server
            .client
            .get(url(path))
            .credential(&session)
            .send()
            .await
            .expect("request");

        assert_eq!(response.status(), status, "{path}");
    }

    // It cannot create an organization, and its list of them does not fail for that.
    let create = server
        .client
        .post(url("/orgs"))
        .credential(&session)
        .json(&json!({"name": "Theirs", "slug": "theirs"}))
        .send()
        .await
        .expect("create organization");

    assert_eq!(create.status(), StatusCode::FORBIDDEN);

    let listed: Value = server
        .client
        .get(url("/orgs"))
        .credential(&session)
        .send()
        .await
        .expect("list organizations")
        .error_for_status()
        .expect("the list answers whoever is in the platform organization")
        .json()
        .await
        .expect("organizations JSON");

    assert!(
        listed
            .as_array()
            .expect("organizations array")
            .iter()
            .any(|organization| organization["system"] == true)
    );

    // Taking the binding away ends the session that is already open.
    let id = binding["value"]["id"].as_str().expect("binding id");

    server
        .client
        .delete(url(&format!("/iam/role-bindings/{id}")))
        .credential(&root)
        .send()
        .await
        .expect("remove the grant")
        .error_for_status()
        .expect("root removes the grant");

    let after = server
        .client
        .get(url("/iam/capabilities"))
        .credential(&session)
        .send()
        .await
        .expect("get capabilities after");

    assert_eq!(after.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_platform_binding_narrowed_to_an_organization_reaches_that_organization_alone() {
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

    let root = system_session_token(&server.state, &server.root_user_id);
    let url = |path: &str| format!("{}/api/v1{path}", server.base_url);

    let mut organizations = Vec::new();

    for slug in ["narrowed-here", "narrowed-elsewhere"] {
        let organization: Value = server
            .client
            .post(url("/orgs"))
            .credential(&root)
            .json(&json!({"name": slug, "slug": slug}))
            .send()
            .await
            .expect("create organization")
            .error_for_status()
            .expect("root creates organizations")
            .json()
            .await
            .expect("organization JSON");

        organizations.push(organization["id"].as_str().expect("id").to_owned());
    }

    let (here, elsewhere) = (&organizations[0], &organizations[1]);

    let keeper = server
        .state
        .iam
        .service
        .create_user(
            "tenant-keeper@test.example".into(),
            "Tenant keeper".into(),
            "userpass",
        )
        .await
        .expect("create account");

    // Signed in somewhere, with no role there.
    server
        .state
        .iam
        .service
        .iam_memberships
        .upsert(
            IamMembership {
                user_id: keeper.id.clone(),
                org_id: server.root_org_id.clone(),
                joined_at: TimestampMicros::now(),
            },
            &[],
            &server.root_user_id,
        )
        .await
        .expect("join with no role");

    let keeper_token = server
        .state
        .iam
        .service
        .issue_token(&keeper.id, &server.root_org_id)
        .expect("issue JWT");

    let role: Value = server
        .client
        .post(url("/roles"))
        .credential(&root)
        .json(&json!({
            "key": "tenant_keeper",
            "name": "Tenant keeper",
            "permissions": [
                "sys.organizations.read",
                "sys.organizations.update",
                "sys.settings.retention.read",
            ],
        }))
        .send()
        .await
        .expect("create role")
        .error_for_status()
        .expect("root creates the role")
        .json()
        .await
        .expect("role JSON");

    server
        .client
        .post(url("/iam/role-bindings"))
        .credential(&root)
        .json(&json!({
            "roleId": role["id"],
            "principalType": "user",
            "principalId": keeper.id.0,
            "resourceType": "organization",
            "resourceId": here,
        }))
        .send()
        .await
        .expect("grant the role on one organization")
        .error_for_status()
        .expect("root grants the role");

    // A role with nothing to do with organizations cannot be narrowed to one.
    let unrelated: Value = server
        .client
        .post(url("/roles"))
        .credential(&root)
        .json(&json!({
            "key": "unrelated",
            "name": "Unrelated",
            "permissions": ["sys.settings.retention.read"],
        }))
        .send()
        .await
        .expect("create role")
        .error_for_status()
        .expect("root creates the role")
        .json()
        .await
        .expect("role JSON");

    let refused = server
        .client
        .post(url("/iam/role-bindings"))
        .credential(&root)
        .json(&json!({
            "roleId": unrelated["id"],
            "principalType": "user",
            "principalId": keeper.id.0,
            "resourceType": "organization",
            "resourceId": here,
        }))
        .send()
        .await
        .expect("narrow an unrelated role");

    assert_eq!(refused.status(), StatusCode::BAD_REQUEST);

    let session = platform_session_of(&server, &keeper_token).await;

    let capabilities: Value = server
        .client
        .get(url("/iam/capabilities"))
        .credential(&session)
        .send()
        .await
        .expect("get capabilities")
        .error_for_status()
        .expect("capabilities succeed")
        .json()
        .await
        .expect("capabilities JSON");

    // Nothing is held across the platform: an instance-wide permission does not narrow.
    assert_eq!(capabilities["permissions"], json!([]));
    assert_eq!(
        capabilities["organizationPermissions"]["sys.organizations.update"],
        json!([here])
    );
    assert!(
        capabilities["routes"]
            .as_array()
            .expect("route decisions")
            .iter()
            .any(|route| route["id"] == "iam.organizations" && route["allowed"] == true)
    );

    let listed: Value = server
        .client
        .get(url("/orgs"))
        .credential(&session)
        .send()
        .await
        .expect("list organizations")
        .error_for_status()
        .expect("list succeeds")
        .json()
        .await
        .expect("organizations JSON");

    let listed = listed.as_array().expect("organizations array");
    assert!(
        listed
            .iter()
            .any(|organization| organization["id"] == *here)
    );
    assert!(
        !listed
            .iter()
            .any(|organization| organization["id"] == *elsewhere)
    );

    for (organization, status) in [(here, StatusCode::OK), (elsewhere, StatusCode::FORBIDDEN)] {
        let renamed = server
            .client
            .patch(url(&format!("/orgs/{organization}")))
            .credential(&session)
            .json(&json!({"name": "Renamed"}))
            .send()
            .await
            .expect("rename organization");

        assert_eq!(renamed.status(), status);
    }

    // What the role does not hold stays out of reach there, and so does entering it.
    let deleted = server
        .client
        .delete(url(&format!("/orgs/{here}")))
        .credential(&session)
        .send()
        .await
        .expect("delete organization");

    assert_eq!(deleted.status(), StatusCode::FORBIDDEN);

    let entered = server
        .client
        .post(url(&format!("/orgs/{here}/select")))
        .credential(&session)
        .send()
        .await
        .expect("enter organization");

    assert_eq!(entered.status(), StatusCode::FORBIDDEN);

    // Root deletes the organization and the binding goes with it, so the way in closes.
    server
        .client
        .delete(url(&format!("/orgs/{here}")))
        .credential(&root)
        .send()
        .await
        .expect("delete organization")
        .error_for_status()
        .expect("root deletes the organization");

    let after = server
        .client
        .get(url("/iam/capabilities"))
        .credential(&session)
        .send()
        .await
        .expect("get capabilities after");

    assert_eq!(after.status(), StatusCode::FORBIDDEN);
}
