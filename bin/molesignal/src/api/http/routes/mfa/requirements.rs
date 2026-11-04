// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 组织的登录要求：组织可以要求进入的会话过了多因素认证，或者只能经本组织的单点登录进入。
//!
//! - `GET`、`PUT /settings/signin-policy`：读、改当前组织的要求，记审计。平台组织的要求只有平台
//!   设置的权限才能动。
//! - 登录、切换组织、刷新会话时用 [`admission`] 判断，规则在
//!   [`crate::app::iam::signin_policy`]。

use axum::{Extension, Json, Router, extract::State, routing::get};

use crate::{
    api::{AppState, http::routes::sso::has_passing_test},
    app::iam::{
        IamContext, SignInMethod, SignInSession,
        signin_policy::{self, Admission},
    },
    domain::iam::{IamScope, SsoProvider, SsoProviderKind},
    infra::{mfa::SigninPolicy, persistence::repositories::audit_events::AuditEvent},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/settings/signin-policy", get(read).put(write))
}

fn ensure_allowed(ctx: &IamContext, writing: bool) -> Result<()> {
    let permission = match (ctx.scope, writing) {
        (IamScope::System, false) => "sys.settings.signin.read",
        (IamScope::System, true) => "sys.settings.signin.manage",
        (_, false) => "org.signin.read",
        (_, true) => "org.signin.manage",
    };

    if ctx.has_permission(permission) {
        Ok(())
    } else {
        Err(Error::forbidden(format!(
            "{permission} permission is required"
        )))
    }
}

async fn read(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<SigninPolicy>> {
    ensure_allowed(&ctx, false)?;

    state.iam.signin_policies.get(&ctx.org_id).await.map(Json)
}

async fn write(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(policy): Json<SigninPolicy>,
) -> Result<Json<SigninPolicy>> {
    ensure_allowed(&ctx, true)?;

    if policy.sso_only && !has_enabled_sso_provider(&state, &ctx.org_id, None).await? {
        return Err(Error::invalid(
            "enable an OIDC, OAuth 2.0 or SAML provider of this organization first",
        ));
    }

    let previous = state.iam.signin_policies.get(&ctx.org_id).await?;
    let now = TimestampMicros::now();

    // 服务商没通过测试就强制单点登录，是管理员确认过的风险，审计里留一笔。
    let sso_tested = if policy.sso_only && !previous.sso_only {
        Some(has_passing_test(&state, &ctx.org_id).await?)
    } else {
        None
    };

    state
        .iam
        .signin_policies
        .put(&ctx.org_id, policy, &ctx.user_id, now)
        .await?;

    let recorded = state
        .iam
        .audit_events
        .record(AuditEvent {
            id: Id::new(),
            org_id: ctx.org_id.clone(),
            actor_kind: "user".into(),
            actor_id: ctx.user_id.0.clone(),
            action: "org.signin_policy.update".into(),
            target_kind: Some("organization".into()),
            target_id: Some(ctx.org_id.0.clone()),
            ip: None,
            user_agent: None,
            payload: serde_json::json!({
                "previous": previous,
                "policy": policy,
                "ssoTested": sso_tested,
            }),
            ts: now,
        })
        .await;

    if let Err(error) = recorded {
        tracing::warn!(error = %error, "sign-in policy audit event could not be recorded");
    }

    Ok(Json(policy))
}

/// 经 `method` 登录的 `user_id` 能不能进入 `org_id`。
pub(crate) async fn admission(
    state: &AppState,
    org_id: &Id,
    user_id: &Id,
    method: &SignInMethod,
) -> Result<Admission> {
    let policy = state.iam.signin_policies.get(org_id).await?;
    let root = policy.sso_only && state.iam.platform_administrators.is_active(user_id).await?;

    Ok(signin_policy::admission(&policy, method, org_id, root))
}

/// 已签发的会话还能不能留在它的组织：要求改了之后，刷新时结束，要重新登录。
pub(crate) async fn ensure_still_admitted(state: &AppState, session: &SignInSession) -> Result<()> {
    match admission(state, &session.org_id, &session.user_id, &session.method).await? {
        Admission::Allowed => Ok(()),
        Admission::NeedsMfa | Admission::NeedsSso => Err(Error::unauthorized(
            "this organization's sign-in requirements changed; sign in again",
        )),
    }
}

/// 只能经单点登录进入的组织，拒绝别的登录方式时的回答。
pub(crate) fn sso_only(organization_name: &str) -> Error {
    Error::forbidden(format!(
        "{organization_name} only allows single sign-on: sign in through its identity provider"
    ))
}

/// 账号所在的组织里，要求多因素认证的那些的名称。
pub(super) async fn required_by(state: &AppState, user_id: &Id) -> Result<Vec<String>> {
    let mut org_ids = state
        .iam
        .service
        .iam_memberships
        .list_for_user(user_id)
        .await?
        .into_iter()
        .map(|membership| membership.org_id)
        .collect::<Vec<_>>();

    if state.iam.platform_administrators.is_active(user_id).await? {
        org_ids.push(state.iam.system_org_id.clone());
    }

    let requiring = state
        .iam
        .signin_policies
        .requiring_mfa_among(&org_ids)
        .await?;

    let mut names = Vec::new();

    for org_id in org_ids.iter().filter(|org_id| requiring.contains(*org_id)) {
        let organization = state.iam.service.orgs.get(org_id).await?;

        if !organization.disabled {
            names.push(organization.name);
        }
    }

    Ok(names)
}

/// 所在组织要求多因素认证时，账号不能去掉最后一种第二因素。管理员的重置不走这里：之后本人在登录
/// 时现场开启。
pub(super) async fn ensure_may_remove_last_factor(state: &AppState, user_id: &Id) -> Result<()> {
    let names = required_by(state, user_id).await?;

    if names.is_empty() {
        return Ok(());
    }

    Err(Error::invalid(format!(
        "{} requires multi-factor authentication: add another second factor first",
        names.join(", ")
    )))
}

/// 组织有没有启用的 OIDC、OAuth 2.0、SAML 服务商，不算 `excluding`。LDAP 只核对目录里的密码，不算
/// 单点登录。
async fn has_enabled_sso_provider(
    state: &AppState,
    org_id: &Id,
    excluding: Option<&Id>,
) -> Result<bool> {
    Ok(state
        .iam
        .sso_providers
        .list_enabled(org_id)
        .await?
        .iter()
        .any(|provider| provider.kind != SsoProviderKind::Ldap && Some(&provider.id) != excluding))
}

/// 组织只允许单点登录时，不能停用或删除它最后一个能登录的服务商：否则除了 root 谁都进不来。
pub(crate) async fn ensure_keeps_sso_provider(
    state: &AppState,
    provider: &SsoProvider,
) -> Result<()> {
    if provider.kind == SsoProviderKind::Ldap || !provider.enabled {
        return Ok(());
    }

    if !state
        .iam
        .signin_policies
        .get(&provider.org_id)
        .await?
        .sso_only
        || has_enabled_sso_provider(state, &provider.org_id, Some(&provider.id)).await?
    {
        return Ok(());
    }

    Err(Error::conflict(
        "this organization only allows single sign-on and this is its last provider: allow other sign-ins first",
    ))
}
