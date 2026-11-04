// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! IAM directory HTTP CRUD：users / orgs / memberships 基础端点。
//!
//! - `GET    /users`              list（`org.members.read`；其他登录成员只拿名字，见 [`member_names`]）
//! - `POST   /users`              create
//! - `GET    /users/:id`          get（自己，或本组织成员的 Admin+）
//! - `PATCH  /users/:id`          update profile（自己，或只属于本组织的成员的 Admin+）
//! - `DELETE /users/:id`          delete（`org.members.remove`，同上）
//!
//! 管理员能处理哪些账号见 [`member_scope`]。
//! - `GET    /orgs`               list（all）
//! - `POST   /orgs`               create
//!
//! Organization CRUD and selection live in the cohesive `organizations`
//! submodule; membership routes remain here because they enforce member IAM.
//!
//! - `POST   /orgs/:id/members`   upsert membership（Admin+ of that org）
//! - `DELETE /orgs/:id/members/:user_id`
//!
//! Permission 实装走 capability key；越权统一返 403。

use std::collections::HashMap;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};

use super::grant_limit;
use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::{IamAssignedRole, IamMembership, User, UserStatus},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

mod accounts;
mod approval_emails;
mod member_names;
mod member_scope;
mod organizations;

pub(crate) use approval_emails::notify_admins_pending_user;
pub(crate) use member_scope::{account_of_current_org, member_of_current_org};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/users", get(list_users).post(create_user))
        .route("/users/{id}/approve", post(accounts::approve_user))
        .route("/users/{id}/reject", post(accounts::reject_user))
        .route(
            "/users/{id}",
            get(accounts::get_user)
                .patch(accounts::update_user)
                .delete(accounts::delete_user),
        )
        .route("/orgs/{id}/members", post(upsert_membership))
        .route("/orgs/{id}/members/{userId}", delete(remove_membership))
        .merge(organizations::routes())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UserView {
    id: String,
    email: String,
    display_name: String,
    /// 引导创建的实例 root 账户。该标记由服务端判定，前端不得自行猜测。
    is_root: bool,
    avatar_url: Option<String>,
    disabled: bool,
    /// "active" | "pending"（待审批）| "rejected"（被拒）。
    status: String,
    /// 当前组织内由 IAM binding 动态解析的角色。
    display_role: Option<String>,
    roles: Vec<IamAssignedRole>,
    team_names: Vec<String>,
    /// password | oidc | saml
    login_method: String,
    last_active_at: Option<i64>,
    joined_at: Option<i64>,
    created_at: i64,
    /// 开了多因素认证。只在成员列表和单个成员里填。
    mfa_enabled: bool,
}

impl UserView {
    fn from_user(u: User, configured_root_email: &str) -> Self {
        let status = match u.status {
            UserStatus::Active => "active",
            UserStatus::Pending => "pending",
            UserStatus::Rejected => "rejected",
        };

        let is_root = is_root_user(&u, configured_root_email);

        Self {
            id: u.id.0,
            email: u.email,
            display_name: u.display_name,
            is_root,
            avatar_url: u.avatar_url,
            disabled: u.disabled,
            status: status.into(),
            display_role: None,
            roles: Vec::new(),
            team_names: Vec::new(),
            login_method: "password".into(),
            last_active_at: None,
            joined_at: None,
            created_at: u.created_at.0,
            mfa_enabled: false,
        }
    }

    fn for_membership(
        u: User,
        membership: IamMembership,
        roles: Vec<IamAssignedRole>,
        team_names: Vec<String>,
        login_method: Option<String>,
        last_active_at_micros: Option<i64>,
        configured_root_email: &str,
    ) -> Self {
        let mut view = Self::from_user(u, configured_root_email);

        let display_role = roles
            .iter()
            .map(|role| role.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");

        view.display_role = (!display_role.is_empty()).then_some(display_role);
        view.roles = roles;
        view.team_names = team_names;
        view.login_method = login_method.unwrap_or_else(|| "password".into());
        view.last_active_at = last_active_at_micros;
        view.joined_at = Some(membership.joined_at.0);
        view
    }
}

fn teams_by_user(teams: Vec<crate::domain::iam::Team>) -> HashMap<String, Vec<String>> {
    let mut by_user = HashMap::<String, Vec<String>>::new();

    for team in teams {
        for member_id in team.member_ids {
            by_user
                .entry(member_id.0)
                .or_default()
                .push(team.name.clone());
        }
    }

    for names in by_user.values_mut() {
        names.sort();
    }

    by_user
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateUserReq {
    email: String,
    display_name: String,
    password: String,
}

/// `GET /users` 的回答：完整的成员，或者只有名字（见 [`member_names`]）。
#[derive(Serialize)]
#[serde(untagged)]
enum MemberList {
    Full(Vec<UserView>),
    Names(Vec<member_names::MemberName>),
}

async fn list_users(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<IamContext>,
) -> Result<Json<MemberList>> {
    if member_names::sees_names_only(&ctx) {
        let member_ids = state
            .iam
            .service
            .iam_memberships
            .list_for_org(&ctx.org_id)
            .await?
            .into_iter()
            .map(|membership| membership.user_id)
            .collect::<Vec<_>>();

        let users = state.iam.service.users.get_many(&member_ids).await?;

        return Ok(Json(MemberList::Names(member_names::member_names(users))));
    }

    require_key(&ctx, "org.members.read")?;

    let memberships = state
        .iam
        .service
        .iam_memberships
        .list_for_org(&ctx.org_id)
        .await?;

    let mut team_names = teams_by_user(state.iam.teams.list(&ctx.org_id).await?);
    let mut users = Vec::with_capacity(memberships.len());

    for membership in memberships {
        let user = state.iam.service.users.get(&membership.user_id).await?;

        let identities = state
            .iam
            .sso_identities
            .list_for_user(&membership.user_id)
            .await?;

        let latest_identity = identities.first();
        let member_team_names = team_names.remove(&membership.user_id.0).unwrap_or_default();

        let roles = state
            .iam
            .service
            .iam_memberships
            .assigned_roles(&membership.user_id, &ctx.org_id)
            .await?;

        users.push(UserView::for_membership(
            user,
            membership,
            roles,
            member_team_names,
            latest_identity.map(|identity| identity.kind.as_str().to_owned()),
            latest_identity.map(|identity| identity.last_login_at.0),
            &state.iam.service.auth_settings().root_email,
        ));
    }

    let ids = users
        .iter()
        .map(|user| Id::from_string(user.id.clone()))
        .collect::<Vec<_>>();

    let with_mfa = state.iam.mfa_factors.enabled_among(&ids).await?;

    for (user, id) in users.iter_mut().zip(&ids) {
        user.mfa_enabled = with_mfa.contains(id);
    }

    users.sort_by(|a, b| {
        a.display_name
            .to_lowercase()
            .cmp(&b.display_name.to_lowercase())
            .then_with(|| a.email.cmp(&b.email))
    });

    Ok(Json(MemberList::Full(users)))
}

async fn create_user(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<IamContext>,
    Json(req): Json<CreateUserReq>,
) -> Result<(StatusCode, Json<UserView>)> {
    require_key(&ctx, "org.members.invite")?;

    let user = state
        .iam
        .service
        .create_user(req.email, req.display_name, &req.password)
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(UserView::from_user(
            user,
            &state.iam.service.auth_settings().root_email,
        )),
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MembershipReq {
    user_id: String,
    #[serde(default)]
    role_ids: Vec<String>,
}

async fn upsert_membership(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<IamContext>,
    Path(org_id): Path<String>,
    Json(req): Json<MembershipReq>,
) -> Result<StatusCode> {
    let target_org = Id::from_string(org_id);

    if state.iam.service.orgs.get(&target_org).await?.system {
        return Err(Error::forbidden(
            "system organization does not accept membership",
        ));
    }

    if ctx.org_id != target_org || !ctx.has_permission("org.members.update") {
        return Err(Error::forbidden("not allowed"));
    }

    let joined_at = state
        .iam
        .service
        .iam_memberships
        .list_for_org(&target_org)
        .await?
        .into_iter()
        .find(|membership| membership.user_id.0 == req.user_id)
        .map(|membership| membership.joined_at)
        .unwrap_or_else(TimestampMicros::now);

    let user_id = Id::from_string(req.user_id);
    state.iam.service.users.get(&user_id).await?;
    crate::api::http::routes::sso::ensure_membership_allowed(&state, &user_id, &target_org).await?;

    let role_ids = req
        .role_ids
        .into_iter()
        .map(Id::from_string)
        .collect::<Vec<_>>();

    grant_limit::require_roles_held(&state, &ctx, &role_ids).await?;
    grant_limit::require_member_within_reach(&state, &ctx, &user_id).await?;

    state
        .iam
        .service
        .iam_memberships
        .upsert(
            IamMembership {
                user_id,
                org_id: target_org,
                joined_at,
            },
            &role_ids,
            &ctx.user_id,
        )
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

async fn remove_membership(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<IamContext>,
    Path((org_id, user_id)): Path<(String, String)>,
) -> Result<StatusCode> {
    let target_org = Id::from_string(org_id);

    if state.iam.service.orgs.get(&target_org).await?.system {
        return Err(Error::forbidden(
            "system organization membership is immutable",
        ));
    }

    if ctx.org_id != target_org || !ctx.has_permission("org.members.remove") {
        return Err(Error::forbidden("not allowed"));
    }

    let target_user = Id::from_string(user_id);
    let user = state.iam.service.users.get(&target_user).await?;
    ensure_user_removable(&user, &state.iam.service.auth_settings().root_email)?;

    if ctx.user_id == target_user {
        return Err(Error::invalid(
            "cannot remove your own membership from the active organization",
        ));
    }

    grant_limit::require_member_within_reach(&state, &ctx, &target_user).await?;

    state
        .iam
        .service
        .iam_memberships
        .remove(&target_user, &target_org)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

fn require_key(ctx: &IamContext, permission: &str) -> Result<()> {
    if ctx.has_permission(permission) {
        Ok(())
    } else {
        Err(Error::forbidden(format!(
            "scope {:?} lacks permission {permission}",
            ctx.scope
        )))
    }
}

fn is_root_user(user: &User, configured_root_email: &str) -> bool {
    let configured_root_email = configured_root_email.trim();

    if configured_root_email.is_empty() {
        user.display_name.trim().eq_ignore_ascii_case("root")
    } else {
        user.email
            .trim()
            .eq_ignore_ascii_case(configured_root_email)
    }
}

fn ensure_user_removable(user: &User, configured_root_email: &str) -> Result<()> {
    if is_root_user(user, configured_root_email) {
        Err(Error::forbidden("root user cannot be removed"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::iam::Team;

    #[test]
    fn groups_and_sorts_team_names_by_user() {
        let teams = vec![
            Team {
                id: Id::from_string("team-platform"),
                org_id: Id::from_string("org"),
                name: "Platform".into(),
                member_ids: vec![Id::from_string("user-1")],
            },
            Team {
                id: Id::from_string("team-observability"),
                org_id: Id::from_string("org"),
                name: "Observability".into(),
                member_ids: vec![Id::from_string("user-1"), Id::from_string("user-2")],
            },
        ];

        let grouped = teams_by_user(teams);

        assert_eq!(
            grouped.get("user-1"),
            Some(&vec!["Observability".to_string(), "Platform".to_string()])
        );
        assert_eq!(
            grouped.get("user-2"),
            Some(&vec!["Observability".to_string()])
        );
    }

    #[test]
    fn organization_user_view_includes_membership_metadata() {
        let user = User {
            id: Id::from_string("user-1"),
            email: "lead@example.com".into(),
            display_name: "Team Lead".into(),
            avatar_url: None,
            bio: String::new(),
            password_hash: "hash".into(),
            disabled: false,
            status: UserStatus::Active,
            created_at: TimestampMicros(10),
            sessions_valid_after: TimestampMicros(0),
        };
        let membership = IamMembership {
            user_id: Id::from_string("user-1"),
            org_id: Id::from_string("org"),
            joined_at: TimestampMicros(20),
        };
        let roles = vec![IamAssignedRole {
            id: Id::from_string("role-sre-lead"),
            key: "sre_lead".into(),
            name: "SRE Lead".into(),
            builtin: false,
        }];

        let view = UserView::for_membership(
            user,
            membership,
            roles,
            vec!["Platform".into()],
            Some("oidc".into()),
            Some(30),
            "",
        );

        assert!(!view.is_root);
        assert_eq!(view.display_role.as_deref(), Some("SRE Lead"));
        assert_eq!(view.roles[0].key, "sre_lead");
        assert_eq!(view.team_names, vec!["Platform"]);
        assert_eq!(view.login_method, "oidc");
        assert_eq!(view.last_active_at, Some(30));
        assert_eq!(view.joined_at, Some(20));
        assert_eq!(view.created_at, 10);
    }

    #[test]
    fn identifies_root_from_configured_email() {
        let user = User {
            id: Id::from_string("root-user"),
            email: "Root@Example.com".into(),
            display_name: "Renamed administrator".into(),
            avatar_url: None,
            bio: String::new(),
            password_hash: "hash".into(),
            disabled: false,
            status: UserStatus::Active,
            created_at: TimestampMicros(10),
            sessions_valid_after: TimestampMicros(0),
        };

        assert!(is_root_user(&user, "root@example.com"));
        let error = ensure_user_removable(&user, "root@example.com").unwrap_err();
        assert!(matches!(&error, Error::Forbidden(_)));
        assert_eq!(error.http_status_code(), 403);
    }

    #[test]
    fn identifies_legacy_root_by_name_without_configured_email() {
        let user = User {
            id: Id::from_string("root-user"),
            email: "bootstrap@example.com".into(),
            display_name: "ROOT".into(),
            avatar_url: None,
            bio: String::new(),
            password_hash: "hash".into(),
            disabled: false,
            status: UserStatus::Active,
            created_at: TimestampMicros(10),
            sessions_valid_after: TimestampMicros(0),
        };

        assert!(is_root_user(&user, ""));
        let error = ensure_user_removable(&user, "").unwrap_err();
        assert!(matches!(&error, Error::Forbidden(_)));
        assert_eq!(error.http_status_code(), 403);
    }
}
