// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! IAM directory HTTP CRUD：users / orgs / memberships 基础端点。
//!
//! - `GET    /users`              list（`org.members.read`）
//! - `POST   /users`              create
//! - `GET    /users/:id`          get（自己或 Admin+）
//! - `PATCH  /users/:id`          update profile（自己或 Admin+）
//! - `DELETE /users/:id`          delete（`org.members.manage`）
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

use crate::{
    api::AppState,
    app::iam::{IamContext, IamSubject},
    domain::iam::{IamAssignedRole, IamMembership, IamScope, User, UserStatus},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

mod organizations;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/users", get(list_users).post(create_user))
        .route("/users/{id}/approve", post(approve_user))
        .route("/users/{id}/reject", post(reject_user))
        .route(
            "/users/{id}",
            get(get_user).patch(update_user).delete(delete_user),
        )
        .route("/orgs/{id}/members", post(upsert_membership))
        .route("/orgs/{id}/members/{user_id}", delete(remove_membership))
        .merge(organizations::routes())
}

#[derive(Serialize)]
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
    last_active_at_micros: Option<i64>,
    joined_at_micros: Option<i64>,
    created_at_micros: i64,
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
            last_active_at_micros: None,
            joined_at_micros: None,
            created_at_micros: u.created_at.0,
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
        view.last_active_at_micros = last_active_at_micros;
        view.joined_at_micros = Some(membership.joined_at.0);
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
struct CreateUserReq {
    email: String,
    display_name: String,
    password: String,
}

async fn list_users(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<IamContext>,
) -> Result<Json<Vec<UserView>>> {
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
        let sessions = state
            .iam
            .sso_sessions
            .list_for_user(&membership.user_id)
            .await?;
        let latest_session = sessions.first();
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
            latest_session.map(|session| session.provider.clone()),
            latest_session.map(|session| session.last_login_at.0),
            &state.iam.service.auth_settings().root_email,
        ));
    }
    users.sort_by(|a, b| {
        a.display_name
            .to_lowercase()
            .cmp(&b.display_name.to_lowercase())
            .then_with(|| a.email.cmp(&b.email))
    });
    Ok(Json(users))
}

async fn create_user(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<IamContext>,
    Json(req): Json<CreateUserReq>,
) -> Result<(StatusCode, Json<UserView>)> {
    require_key(&ctx, "org.members.manage")?;
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

/// 审批自助注册的 pending 用户 → active（Owner/Admin）。
async fn approve_user(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<UserView>> {
    require_key(&ctx, "org.members.manage")?;
    let target = Id::from_string(id);
    state
        .iam
        .service
        .users
        .set_status(&target, UserStatus::Active)
        .await?;
    let user = state.iam.service.users.get(&target).await?;
    notify_user_decision(&state, &user.email, true).await;
    Ok(Json(UserView::from_user(
        user,
        &state.iam.service.auth_settings().root_email,
    )))
}

/// 拒绝 pending 用户 → rejected（保留记录、不可登录）+ 邮件通知本人（Owner/Admin）。
async fn reject_user(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<UserView>> {
    require_key(&ctx, "org.members.manage")?;
    let target = Id::from_string(id);
    state
        .iam
        .service
        .users
        .set_status(&target, UserStatus::Rejected)
        .await?;
    let user = state.iam.service.users.get(&target).await?;
    notify_user_decision(&state, &user.email, false).await;
    Ok(Json(UserView::from_user(
        user,
        &state.iam.service.auth_settings().root_email,
    )))
}

/// 收集拥有成员管理能力的邮箱（审批通知收件人）。
async fn collect_org_admin_emails(state: &AppState, org_id: &Id) -> Vec<String> {
    let members = match state.iam.service.iam_memberships.list_for_org(org_id).await {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(error = %e, "approval-notify: list org members failed");
            return Vec::new();
        }
    };
    let mut emails = Vec::new();
    for m in members {
        let can_manage_members = state
            .iam
            .access
            .capabilities(&IamSubject {
                user_id: m.user_id.clone(),
                organization_id: org_id.clone(),
                credential_role_id: None,
                credential_application_id: None,
                scope: IamScope::Organization,
            })
            .await
            .is_ok_and(|capabilities| capabilities.has("org.members.manage"));
        if can_manage_members && let Ok(u) = state.iam.service.users.get(&m.user_id).await {
            emails.push(u.email);
        }
    }
    emails
}

/// best-effort 通知 org 管理员有新用户待审批（无 SMTP / 失败仅 warn，绝不阻塞注册）。
pub(crate) async fn notify_admins_pending_user(state: &AppState, org_id: &Id, pending_email: &str) {
    let Some(sender) = state.iam.email_sender.as_ref() else {
        return;
    };
    let admins = collect_org_admin_emails(state, org_id).await;
    if admins.is_empty() {
        return;
    }
    let subject = "MoleSignal: a new user is awaiting approval";
    let base = state.platform.external_url.trim_end_matches('/');
    let body = if base.is_empty() {
        format!(
            "A new user has signed up and is awaiting your approval:\n\n  {pending_email}\n\nReview pending registrations in IAM \u{2192} Approvals.\n"
        )
    } else {
        format!(
            "A new user has signed up and is awaiting your approval:\n\n  {pending_email}\n\nReview: {base}/iam/approvals\n"
        )
    };
    if let Err(e) = sender.send_text(&admins, subject, &body).await {
        tracing::warn!(error = %e, "approval-notify: send to admins failed");
    }
}

/// best-effort 通知用户审批结果（无 SMTP / 失败仅 warn）。
async fn notify_user_decision(state: &AppState, email: &str, approved: bool) {
    let Some(sender) = state.iam.email_sender.as_ref() else {
        return;
    };
    let base = state.platform.external_url.trim_end_matches('/');
    let (subject, body) = if approved {
        let body = if base.is_empty() {
            "Good news \u{2014} your registration has been approved. You can now sign in.\n"
                .to_string()
        } else {
            format!(
                "Good news \u{2014} your registration has been approved. Sign in: {base}/signin\n"
            )
        };
        ("MoleSignal: your account has been approved", body)
    } else {
        (
            "MoleSignal: your registration was declined",
            "Your registration request has been declined by an administrator.\n".to_string(),
        )
    };
    if let Err(e) = sender.send_text(&[email.to_string()], subject, &body).await {
        tracing::warn!(error = %e, approved, "approval-notify: send to user failed");
    }
}

async fn get_user(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<UserView>> {
    let target = Id::from_string(id);
    // 自己或拥有成员读取权限的管理员。
    if target != ctx.user_id && !ctx.has_permission("org.members.read") {
        return Err(Error::forbidden("not allowed"));
    }
    let user = state.iam.service.users.get(&target).await?;
    Ok(Json(UserView::from_user(
        user,
        &state.iam.service.auth_settings().root_email,
    )))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateUserReq {
    /// Kept only to return an explicit immutable-field error to legacy or
    /// hand-written clients. A user's sign-in identity is never profile data.
    email: Option<String>,
    display_name: Option<String>,
    avatar_url: Option<String>,
    disabled: Option<bool>,
}

async fn update_user(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<UpdateUserReq>,
) -> Result<Json<UserView>> {
    let target = Id::from_string(id);
    if target != ctx.user_id && !ctx.has_permission("org.members.manage") {
        return Err(Error::forbidden("not allowed"));
    }
    let mut user = state.iam.service.users.get(&target).await?;
    if req.email.is_some() {
        return Err(Error::invalid("user email is immutable"));
    }
    if let Some(display_name) = req.display_name {
        let display_name = display_name.trim().to_string();
        if display_name.is_empty() {
            return Err(Error::invalid("display_name must not be empty"));
        }
        user.display_name = display_name;
    }
    if let Some(avatar_url) = req.avatar_url {
        user.avatar_url = normalize_avatar_url(avatar_url)?;
    }
    if let Some(disabled) = req.disabled {
        require_key(&ctx, "org.members.manage")?;
        if target == ctx.user_id && disabled {
            return Err(Error::invalid("cannot disable your own account"));
        }
        if is_root_user(&user, &state.iam.service.auth_settings().root_email) {
            return Err(Error::forbidden("root user cannot be disabled"));
        }
        if disabled {
            ensure_platform_admin_remains(&state, &target).await?;
        }
        user.disabled = disabled;
    }
    let user = state.iam.service.users.update(user).await?;
    Ok(Json(UserView::from_user(
        user,
        &state.iam.service.auth_settings().root_email,
    )))
}

async fn delete_user(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    require_key(&ctx, "org.members.manage")?;
    let target = Id::from_string(id);
    if target == ctx.user_id {
        return Err(Error::invalid("cannot delete your own account"));
    }
    let user = state.iam.service.users.get(&target).await?;
    ensure_user_removable(&user, &state.iam.service.auth_settings().root_email)?;
    ensure_platform_admin_remains(&state, &target).await?;
    state.iam.service.users.delete(&target).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
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
    if ctx.org_id != target_org || !ctx.has_permission("org.members.manage") {
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
    let role_ids = req
        .role_ids
        .into_iter()
        .map(Id::from_string)
        .collect::<Vec<_>>();
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
    if ctx.org_id != target_org || !ctx.has_permission("org.members.manage") {
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

fn require_system_organization_management(system_org_id: &Id, ctx: &IamContext) -> Result<()> {
    if ctx.scope == IamScope::System
        && ctx.org_id == *system_org_id
        && ctx.has_permission("sys.organizations.manage")
    {
        Ok(())
    } else {
        Err(Error::forbidden(
            "organization management requires _sys scope and sys.organizations.manage",
        ))
    }
}

async fn ensure_platform_admin_remains(state: &AppState, user_id: &Id) -> Result<()> {
    if state.iam.platform_administrators.is_active(user_id).await? {
        Err(Error::conflict(
            "configured root user cannot be made unusable",
        ))
    } else {
        Ok(())
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

fn normalize_avatar_url(value: String) -> Result<Option<String>> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() > 2048 {
        return Err(Error::invalid("avatar_url must be at most 2048 characters"));
    }
    if !(value.starts_with("https://") || value.starts_with("http://")) {
        return Err(Error::invalid("avatar_url must be an http(s) URL"));
    }
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::iam::Team;

    fn organization_management_context(
        scope: IamScope,
        org_id: &str,
        permissions: &[&str],
    ) -> IamContext {
        IamContext {
            user_id: Id::from_string("user-1"),
            org_id: Id::from_string(org_id),
            display_role: "Platform Owner".into(),
            roles: Vec::new(),
            credential_role_id: None,
            credential_application_id: None,
            scope,
            permissions: permissions
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
            features: Default::default(),
            policy_version: 1,
        }
    }

    #[test]
    fn organization_management_requires_system_scope_permission() {
        let system_org_id = Id::from_string("_sys-id");
        let authorized = organization_management_context(
            IamScope::System,
            "_sys-id",
            &["sys.organizations.manage"],
        );
        assert!(require_system_organization_management(&system_org_id, &authorized).is_ok());

        for denied in [
            organization_management_context(
                IamScope::Organization,
                "tenant-a",
                &["sys.organizations.manage", "org.settings.manage"],
            ),
            organization_management_context(
                IamScope::System,
                "tenant-a",
                &["sys.organizations.manage"],
            ),
            organization_management_context(IamScope::System, "_sys-id", &[]),
        ] {
            let error =
                require_system_organization_management(&system_org_id, &denied).unwrap_err();
            assert!(matches!(&error, Error::Forbidden(_)));
            assert_eq!(error.http_status_code(), 403);
        }
    }

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
        assert_eq!(view.last_active_at_micros, Some(30));
        assert_eq!(view.joined_at_micros, Some(20));
        assert_eq!(view.created_at_micros, 10);
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
        };

        assert!(is_root_user(&user, ""));
        let error = ensure_user_removable(&user, "").unwrap_err();
        assert!(matches!(&error, Error::Forbidden(_)));
        assert_eq!(error.http_status_code(), 403);
    }
}
