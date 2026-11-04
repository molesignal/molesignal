// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! SSO provider 管理端点：增删改查、启停、可分配角色，以及填表用的身份源读取
//! （[`discovery`]）。
//!
//! 权限：`org.sso.read` 读取，`org.sso.manage` 增改删及启停。面向公网账号的
//! 服务启用前要有限定范围（[`super::policy`]）。读取时带上当前配置的测试情况
//! （[`super::signin_test`]）。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{get, post},
};

use super::{policy, signin_test};
use crate::{
    api::{AppState, http::routes::mfa},
    app::iam::IamContext,
    domain::iam::{SsoProvider, SsoProviderKind, permission},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

mod discovery;
mod mapping;
mod request;
mod response;

use request::{UpsertRequest, build_config};
pub(crate) use response::PublicProviderResponse;
use response::{AssignableRoleResponse, ProviderResponse};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .merge(discovery::routes())
        .route("/auth/sso/providers", get(list_public))
        .route("/sso/providers", get(list).post(create))
        .route("/sso/providers/roles", get(list_assignable_roles))
        .route(
            "/sso/providers/{id}",
            get(get_one).put(update).delete(delete),
        )
        .route("/sso/providers/{id}/enable", post(enable))
        .route("/sso/providers/{id}/disable", post(disable))
}

pub(super) fn ensure_same_org(p: &SsoProvider, ctx: &IamContext) -> Result<()> {
    if p.org_id != ctx.org_id {
        return Err(Error::forbidden("cross-org sso_provider"));
    }

    Ok(())
}

async fn list_public(State(state): State<AppState>) -> Result<Json<Vec<PublicProviderResponse>>> {
    let providers = &state.iam.sso_providers;

    let (oidc, oauth2, saml, ldap) = tokio::try_join!(
        providers.list_enabled_by_kind(SsoProviderKind::Oidc),
        providers.list_enabled_by_kind(SsoProviderKind::Oauth2),
        providers.list_enabled_by_kind(SsoProviderKind::Saml),
        providers.list_enabled_by_kind(SsoProviderKind::Ldap),
    )?;

    let mut providers = oidc
        .into_iter()
        .chain(oauth2)
        .chain(saml)
        .chain(ldap)
        .map(PublicProviderResponse::from)
        .collect::<Vec<_>>();

    providers.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(Json(providers))
}

/// 这个组织能跳转登录的服务：切换到只允许单点登录的组织时，告诉浏览器可以去哪儿。
pub(crate) async fn redirect_providers_of(
    state: &AppState,
    org_id: &Id,
) -> Result<Vec<PublicProviderResponse>> {
    Ok(state
        .iam
        .sso_providers
        .list_enabled(org_id)
        .await?
        .into_iter()
        .filter(|provider| provider.kind != SsoProviderKind::Ldap)
        .map(PublicProviderResponse::from)
        .collect())
}

#[permission("org.sso.read")]
async fn list_assignable_roles(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<AssignableRoleResponse>>> {
    state.iam.roles.ensure_builtin_roles(&ctx.org_id).await?;

    let roles = state
        .iam
        .roles
        .list(&ctx.org_id)
        .await?
        .into_iter()
        .filter(|role| role.role_type == "organization" && role.scope == "organization")
        .map(|role| AssignableRoleResponse::new(role.id.0, role.name))
        .collect();

    Ok(Json(roles))
}

#[permission("org.sso.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<ProviderResponse>>> {
    let providers = state.iam.sso_providers.list(&ctx.org_id).await?;
    let mut standings = signin_test::standings(&state, &providers).await?;

    Ok(Json(
        providers
            .into_iter()
            .map(|provider| {
                let standing = standings.remove(&provider.id.0);

                ProviderResponse::from(provider).with_test(standing)
            })
            .collect(),
    ))
}

#[permission("org.sso.read")]
async fn get_one(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<ProviderResponse>> {
    let p = state.iam.sso_providers.get(&Id::from_string(id)).await?;
    ensure_same_org(&p, &ctx)?;

    let standing = signin_test::standings(&state, std::slice::from_ref(&p))
        .await?
        .remove(&p.id.0);

    Ok(Json(ProviderResponse::from(p).with_test(standing)))
}

#[permission("org.sso.manage")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<UpsertRequest>,
) -> Result<Json<ProviderResponse>> {
    let validated = build_config(req, None)?;
    mapping::ensure_roles_exist(&state, &ctx.org_id, &validated.config).await?;
    mapping::ensure_roles_grantable(&state, &ctx, &validated.config, None).await?;
    let now = TimestampMicros::now();

    let p = SsoProvider {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        name: validated.name,
        kind: validated.kind,
        platform: validated.platform,
        enabled: validated.enabled.unwrap_or(true),
        config: validated.config,
        created_at: now,
        updated_at: now,
    };

    if p.enabled {
        policy::ensure_restricted(&state, &p).await?;
    }

    Ok(Json(state.iam.sso_providers.create(p).await?.into()))
}

#[permission("org.sso.manage")]
async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<UpsertRequest>,
) -> Result<Json<ProviderResponse>> {
    let id = Id::from_string(id);
    let existing = state.iam.sso_providers.get(&id).await?;
    ensure_same_org(&existing, &ctx)?;
    let validated = build_config(req, Some(&existing.config))?;
    mapping::ensure_roles_exist(&state, &ctx.org_id, &validated.config).await?;
    mapping::ensure_roles_grantable(&state, &ctx, &validated.config, Some(&existing.config))
        .await?;

    let updated = SsoProvider {
        id,
        org_id: existing.org_id.clone(),
        name: validated.name,
        kind: validated.kind,
        platform: validated.platform,
        enabled: validated.enabled.unwrap_or(existing.enabled),
        config: validated.config,
        created_at: existing.created_at,
        updated_at: TimestampMicros::now(),
    };

    if !updated.enabled || updated.kind == SsoProviderKind::Ldap {
        mfa::ensure_keeps_sso_provider(&state, &existing).await?;
    }

    if updated.enabled {
        policy::ensure_restricted(&state, &updated).await?;
    }

    Ok(Json(state.iam.sso_providers.update(updated).await?.into()))
}

#[permission("org.sso.manage")]
async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode> {
    let id = Id::from_string(id);
    let existing = state.iam.sso_providers.get(&id).await?;
    ensure_same_org(&existing, &ctx)?;
    mfa::ensure_keeps_sso_provider(&state, &existing).await?;
    state.iam.sso_providers.delete(&id).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[permission("org.sso.manage")]
async fn enable(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<ProviderResponse>> {
    set_enabled(state, ctx, id, true).await
}

#[permission("org.sso.manage")]
async fn disable(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<ProviderResponse>> {
    set_enabled(state, ctx, id, false).await
}

async fn set_enabled(
    state: AppState,
    ctx: IamContext,
    id: String,
    enabled: bool,
) -> Result<Json<ProviderResponse>> {
    let id = Id::from_string(id);
    let existing = state.iam.sso_providers.get(&id).await?;
    ensure_same_org(&existing, &ctx)?;

    if enabled {
        policy::ensure_restricted(&state, &existing).await?;
    } else {
        mfa::ensure_keeps_sso_provider(&state, &existing).await?;
    }

    Ok(Json(
        state
            .iam
            .sso_providers
            .set_enabled(&id, enabled)
            .await?
            .into(),
    ))
}
