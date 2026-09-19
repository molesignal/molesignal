// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! SSO provider 管理端点：增删改查、启停和可分配角色。
//!
//! 权限：`org.settings.read` 读取，`org.settings.manage` 增改删及启停。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{get, post},
};

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::{SsoProvider, SsoProviderKind, permission},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

mod mapping;
mod model;

use model::{
    AssignableRoleResponse, ProviderResponse, PublicProviderResponse, UpsertRequest, build_config,
};

pub fn routes() -> Router<AppState> {
    Router::new()
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

fn ensure_same_org(p: &SsoProvider, ctx: &IamContext) -> Result<()> {
    if p.org_id != ctx.org_id {
        return Err(Error::forbidden("cross-org sso_provider"));
    }
    Ok(())
}

async fn list_public(State(state): State<AppState>) -> Result<Json<Vec<PublicProviderResponse>>> {
    if !state.platform.license.has_feature("sso") {
        return Ok(Json(Vec::new()));
    }
    let (oidc, saml, ldap) = tokio::try_join!(
        state
            .iam
            .sso_providers
            .list_enabled_by_kind(SsoProviderKind::Oidc),
        state
            .iam
            .sso_providers
            .list_enabled_by_kind(SsoProviderKind::Saml),
        state
            .iam
            .sso_providers
            .list_enabled_by_kind(SsoProviderKind::Ldap),
    )?;
    let mut providers = oidc
        .into_iter()
        .chain(saml)
        .chain(ldap)
        .map(PublicProviderResponse::from)
        .collect::<Vec<_>>();
    providers.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(Json(providers))
}

#[permission("org.settings.read")]
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

#[permission("org.settings.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<ProviderResponse>>> {
    let providers = state.iam.sso_providers.list(&ctx.org_id).await?;
    Ok(Json(
        providers.into_iter().map(ProviderResponse::from).collect(),
    ))
}

#[permission("org.settings.read")]
async fn get_one(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<ProviderResponse>> {
    let p = state.iam.sso_providers.get(&Id::from_string(id)).await?;
    ensure_same_org(&p, &ctx)?;
    Ok(Json(p.into()))
}

#[permission("org.settings.manage")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<UpsertRequest>,
) -> Result<Json<ProviderResponse>> {
    let (name, kind, enabled, config) = build_config(req, None)?;
    mapping::ensure_roles_exist(&state, &ctx.org_id, &config).await?;
    let now = TimestampMicros::now();
    let p = SsoProvider {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        name,
        kind,
        enabled: enabled.unwrap_or(true),
        config,
        created_at: now,
        updated_at: now,
    };
    Ok(Json(state.iam.sso_providers.create(p).await?.into()))
}

#[permission("org.settings.manage")]
async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<UpsertRequest>,
) -> Result<Json<ProviderResponse>> {
    let id = Id::from_string(id);
    let existing = state.iam.sso_providers.get(&id).await?;
    ensure_same_org(&existing, &ctx)?;
    let (name, kind, enabled, config) = build_config(req, Some(&existing.config))?;
    mapping::ensure_roles_exist(&state, &ctx.org_id, &config).await?;
    let updated = SsoProvider {
        id,
        org_id: existing.org_id,
        name,
        kind,
        enabled: enabled.unwrap_or(existing.enabled),
        config,
        created_at: existing.created_at,
        updated_at: TimestampMicros::now(),
    };
    Ok(Json(state.iam.sso_providers.update(updated).await?.into()))
}

#[permission("org.settings.manage")]
async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode> {
    let id = Id::from_string(id);
    let existing = state.iam.sso_providers.get(&id).await?;
    ensure_same_org(&existing, &ctx)?;
    state.iam.sso_providers.delete(&id).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[permission("org.settings.manage")]
async fn enable(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<ProviderResponse>> {
    set_enabled(state, ctx, id, true).await
}

#[permission("org.settings.manage")]
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
    Ok(Json(
        state
            .iam
            .sso_providers
            .set_enabled(&id, enabled)
            .await?
            .into(),
    ))
}
