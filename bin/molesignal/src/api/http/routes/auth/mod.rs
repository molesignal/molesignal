// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 账号登录：密码登录、自助注册（[`signup`]）、登录会话和密码重置。
//!
//! 登录成功后会话写进 Cookie（[`crate::api::http::session_cookie`]），响应体 `{"login": …}` 只有
//! 页面要显示的身份。账号开了多因素认证时，密码对了也先不写 Cookie，回答 `{"mfa": …}` 等第二步
//! （[`crate::api::http::routes::mfa`]）。输错太多次时回答 429，不再核对密码
//! （[`crate::app::iam::signin_limit`]）。

use axum::{Json, Router, extract::State, http::HeaderMap, response::Response, routing::post};
use serde::{Deserialize, Serialize};

use crate::{
    api::{
        AppState,
        http::{
            routes::mfa,
            session_cookie::{self, SessionCookies},
            signin_limit::SigninSource,
        },
    },
    app::iam::{
        SignInMethod,
        signin_limit::{SigninAccount, SigninError},
        signin_policy::Admission,
    },
    domain::iam::{IamAssignedRole, IamScope, Organization, User, super_administrator},
    shared::{Error, Result, ids::Id},
};

mod password_reset;
mod session;
mod signup;

pub(crate) use signup::can_verify_email;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/signin", post(signin))
        .merge(signup::routes())
        .merge(session::routes())
        .merge(password_reset::routes())
}

/// 页面显示的登录身份：密码登录、单点登录和 `GET /auth/session` 都回答这个形状。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionView {
    pub user_id: String,
    pub email: String,
    pub display_name: String,
    pub org_id: String,
    pub org_name: String,
    pub display_role: String,
    pub roles: Vec<IamAssignedRole>,
    pub scope: IamScope,
}

impl SessionView {
    pub(crate) fn new(
        user: User,
        organization: Organization,
        roles: Vec<IamAssignedRole>,
        scope: IamScope,
    ) -> Self {
        Self {
            user_id: user.id.0,
            email: user.email,
            display_name: user.display_name,
            org_id: organization.id.0,
            org_name: organization.name,
            display_role: display_role(&roles),
            roles,
            scope,
        }
    }

    /// 为这个身份开始一次登录，返回写 Cookie 的响应头。`method` 记下这次是怎么登录的。
    pub(crate) fn sign_in(
        &self,
        state: &AppState,
        headers: &HeaderMap,
        remember: bool,
        method: SignInMethod,
    ) -> Result<SessionCookies> {
        session_cookie::sign_in(
            state,
            headers,
            (
                &Id::from_string(self.user_id.clone()),
                &Id::from_string(self.org_id.clone()),
                self.scope,
            ),
            remember,
            method,
        )
    }
}

pub(crate) fn display_role(roles: &[IamAssignedRole]) -> String {
    roles
        .iter()
        .map(|role| role.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

#[derive(Deserialize)]
pub struct SigninReq {
    pub email: String,
    pub password: String,
    /// 记住我：关掉浏览器后仍保持登录。
    #[serde(default)]
    pub remember: bool,
}

/// 回答 `{"login": …}` 并写 Cookie；账号开了多因素认证时回答 `{"mfa": …}`，等第二步。
async fn signin(
    State(state): State<AppState>,
    SigninSource(source): SigninSource,
    headers: HeaderMap,
    Json(req): Json<SigninReq>,
) -> Result<Response, SigninError> {
    let attempt = state
        .iam
        .signin_limiter
        .begin(&SigninAccount::Email(&req.email), source)
        .await?;

    let outcome = state
        .iam
        .service
        .authenticate(&req.email, &req.password)
        .await;

    attempt.settle(&outcome).await;
    let user = outcome?;

    let mfa_on = mfa::enabled(&state, &user.id).await?;
    let method = mfa::after_first_step(mfa_on);
    let (org, roles, scope) = signin_target(&state, &user.id, &method).await?;
    let session = SessionView::new(user, org, roles, scope);

    Ok(mfa::finish_first_step(&state, &headers, session, req.remember).await?)
}

/// 经 `method` 的这次登录进入哪个组织：第一个放行的成员组织，没有时 root 和有平台授权的账号进入平台
/// 组织。都不放行时，第一个只差多因素认证的组织：调用方让账号现场开启。只有只允许单点登录的组织时说明原因。
pub(crate) async fn signin_target(
    state: &AppState,
    user_id: &Id,
    method: &SignInMethod,
) -> Result<(Organization, Vec<IamAssignedRole>, IamScope)> {
    let root = state.iam.platform_administrators.is_active(user_id).await?;
    let mut needs_mfa = None;
    let mut needs_sso = None;

    for membership in state
        .iam
        .service
        .iam_memberships
        .list_for_user(user_id)
        .await?
    {
        let organization = state.iam.service.orgs.get(&membership.org_id).await?;

        if organization.system || organization.disabled {
            continue;
        }

        match mfa::admission(state, &organization.id, user_id, method).await? {
            Admission::Allowed => return member_target(state, user_id, root, organization).await,
            Admission::NeedsMfa => {
                needs_mfa.get_or_insert(organization);
            }
            Admission::NeedsSso => {
                needs_sso.get_or_insert(organization);
            }
        }
    }

    if root || state.iam.access.holds_platform_permissions(user_id).await? {
        let organization = state.iam.service.orgs.get(&state.iam.system_org_id).await?;
        let admission = mfa::admission(state, &organization.id, user_id, method).await?;

        if admission == Admission::Allowed || needs_mfa.is_none() {
            return platform_target(state, user_id, root, organization).await;
        }
    }

    if let Some(organization) = needs_mfa {
        return member_target(state, user_id, root, organization).await;
    }

    match needs_sso {
        Some(organization) => Err(mfa::sso_only(&organization.name)),
        None => Err(Error::forbidden(
            "user has no enabled organization membership",
        )),
    }
}

async fn member_target(
    state: &AppState,
    user_id: &Id,
    root: bool,
    organization: Organization,
) -> Result<(Organization, Vec<IamAssignedRole>, IamScope)> {
    let roles = if root {
        vec![super_administrator()]
    } else {
        state
            .iam
            .service
            .iam_memberships
            .assigned_roles(user_id, &organization.id)
            .await?
    };

    Ok((organization, roles, IamScope::Organization))
}

async fn platform_target(
    state: &AppState,
    user_id: &Id,
    root: bool,
    organization: Organization,
) -> Result<(Organization, Vec<IamAssignedRole>, IamScope)> {
    let roles = if root {
        vec![super_administrator()]
    } else {
        state.iam.access.platform_grant(user_id).await?.roles
    };

    Ok((organization, roles, IamScope::System))
}
