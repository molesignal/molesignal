// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! SSO 登录端点。
//!
//! `GET /auth/sso/login`：构造 `state` / `nonce`，存 [`SsoStateStore`] 后 302 到 IdP。
//! `GET/POST /auth/sso/callback`：取 IdP 回调 `code` + `state` → 校验 state → 取
//! 对应 provider 配置 → exchange_code → verify_id_token (JWKS + nonce) →
//! provision_or_get_user → 发本地 JWT。
//!
//! Login 阶段没有 session 拿 org_id，按 `?provider_id=` / `?provider=` 解析；
//! 都不带且只有一个 enabled OIDC provider 时自动选它。
//!
//! SAML 登录由 [`saml`] 子模块处理。
//!
//! License gate：所有路径走前先调 `state.platform.license.has_feature("sso")`，OSS / 未授
//! 权 license 直接 403。

use axum::{
    Json, Router,
    extract::{Query, State},
    response::{IntoResponse, Redirect, Response},
    routing::get,
};
use serde::Deserialize;

use crate::{
    api::AppState,
    domain::iam::{SsoOidcConfig, SsoProvider, SsoProviderConfig, SsoProviderKind},
    infra::sso::{OidcConfig, OidcLoginFlow},
    shared::{Error, Result, ids::Id},
};

mod ldap;
mod providers;
pub(super) mod provision;
mod saml;

use provision::{FederatedIdentity, FederatedLoginResponse, complete_login};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/sso/login", get(login))
        .route("/auth/sso/callback", get(callback_get).post(callback_post))
        .merge(ldap::routes())
        .merge(saml::routes())
        .merge(providers::routes())
}

#[derive(Debug, Deserialize)]
pub struct LoginParams {
    /// 直接指定 provider 行 id；用于多 IdP / 多 org 场景。
    #[serde(default)]
    pub provider_id: Option<String>,
    /// 按 provider name 在全表里找 enabled 行；name 在同 org 内唯一，跨 org 时
    /// 可能命中多个 — 命中多个返 400 要求改用 `provider_id`。
    #[serde(default)]
    pub provider: Option<String>,
}

async fn login(State(state): State<AppState>, Query(p): Query<LoginParams>) -> Result<Response> {
    if !state.platform.license.has_feature("sso") {
        return Err(Error::forbidden("sso feature not licensed"));
    }
    let provider = resolve_provider_for_login(&state, &p).await?;
    if provider.kind != SsoProviderKind::Oidc {
        return Err(Error::invalid(format!(
            "provider {} is kind={:?}; /auth/sso/login currently handles OIDC only",
            provider.id.0, provider.kind
        )));
    }
    let oidc_cfg = match &provider.config {
        SsoProviderConfig::Oidc(c) => c.clone(),
        _ => unreachable!("kind == Oidc enforced above"),
    };

    let flow = OidcLoginFlow::new(to_flow_config(&oidc_cfg))?;
    let state_str = rand_b64(24);
    let nonce_str = rand_b64(24);
    state
        .iam
        .sso_state_store
        .put(state_str.clone(), provider.id.0.clone(), nonce_str.clone());
    let url = flow.build_authz_url(&state_str, &nonce_str)?;
    Ok(Redirect::temporary(url.as_str()).into_response())
}

#[derive(Debug, Deserialize)]
pub struct CallbackParams {
    pub code: String,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub error_description: Option<String>,
}

async fn callback_get(
    State(state): State<AppState>,
    Query(p): Query<CallbackParams>,
) -> Result<Json<FederatedLoginResponse>> {
    do_callback(state, p).await
}

async fn callback_post(
    State(state): State<AppState>,
    Json(p): Json<CallbackParams>,
) -> Result<Json<FederatedLoginResponse>> {
    do_callback(state, p).await
}

async fn do_callback(state: AppState, p: CallbackParams) -> Result<Json<FederatedLoginResponse>> {
    if !state.platform.license.has_feature("sso") {
        return Err(Error::forbidden("sso feature not licensed"));
    }
    if let Some(e) = p.error {
        return Err(Error::Unauthorized(format!(
            "idp returned error: {e} ({})",
            p.error_description.unwrap_or_default()
        )));
    }
    let state_param = p
        .state
        .ok_or_else(|| Error::Unauthorized("callback missing state".into()))?;
    let entry = state
        .iam
        .sso_state_store
        .take(&state_param)
        .ok_or_else(|| Error::Unauthorized("invalid or expired state".into()))?;
    let provider = state
        .iam
        .sso_providers
        .get(&Id::from_string(entry.provider_id.clone()))
        .await
        .map_err(|_| Error::Unauthorized("sso provider not found".into()))?;
    if !provider.enabled {
        return Err(Error::forbidden(format!(
            "provider {} is disabled",
            provider.id.0
        )));
    }
    let oidc_cfg = match &provider.config {
        SsoProviderConfig::Oidc(c) => c.clone(),
        _ => {
            return Err(Error::invalid(format!(
                "provider {} is not OIDC; cannot handle in /auth/sso/callback",
                provider.id.0
            )));
        }
    };

    let flow = OidcLoginFlow::new(to_flow_config(&oidc_cfg))?;
    let tokens = flow.exchange_code(&p.code).await?;
    let id_token = tokens
        .id_token
        .as_deref()
        .ok_or_else(|| Error::Unauthorized("idp returned no id_token".into()))?;
    let oidc_user = flow
        .verify_id_token(id_token, state.iam.sso_jwks_cache.as_ref(), &entry.nonce)
        .await?;
    if oidc_user.email.is_empty() {
        return Err(Error::Unauthorized("idp id_token lacks email claim".into()));
    }

    let response = complete_login(
        &state,
        &provider,
        FederatedIdentity {
            email: oidc_user.email.clone(),
            display_name: oidc_user
                .name
                .clone()
                .unwrap_or_else(|| oidc_user.email.clone()),
            subject: oidc_user.subject,
            groups: oidc_user.groups,
        },
        &oidc_cfg.group_role_mapping,
        oidc_cfg.default_role_id.as_deref(),
    )
    .await?;
    Ok(Json(response))
}

async fn resolve_provider_for_login(state: &AppState, p: &LoginParams) -> Result<SsoProvider> {
    if let Some(id) = p.provider_id.as_deref() {
        let p = state
            .iam
            .sso_providers
            .get(&Id::from_string(id.to_string()))
            .await?;
        if !p.enabled {
            return Err(Error::invalid(format!("provider {} is disabled", p.id.0)));
        }
        return Ok(p);
    }
    let mut candidates = state
        .iam
        .sso_providers
        .list_enabled_by_kind(SsoProviderKind::Oidc)
        .await?;
    if let Some(name) = p.provider.as_deref() {
        candidates.retain(|c| c.name == name);
    }
    if candidates.is_empty() {
        return Err(Error::invalid(
            "no enabled OIDC provider configured; create one via POST /api/v1/sso/providers",
        ));
    }
    if candidates.len() > 1 {
        let names: Vec<String> = candidates.iter().map(|c| c.name.clone()).collect();
        return Err(Error::invalid(format!(
            "multiple enabled OIDC providers ({}); add ?provider_id=<id> to choose",
            names.join(", ")
        )));
    }
    Ok(candidates.remove(0))
}

fn to_flow_config(cfg: &SsoOidcConfig) -> OidcConfig {
    OidcConfig {
        issuer: cfg.issuer.clone(),
        authorize_url: cfg.authorize_url.clone(),
        token_url: cfg.token_url.clone(),
        userinfo_url: cfg.userinfo_url.clone(),
        jwks_uri: cfg.jwks_uri.clone().unwrap_or_default(),
        client_id: cfg.client_id.clone(),
        client_secret: cfg.client_secret.clone(),
        redirect_uri: cfg.redirect_uri.clone(),
        scopes: cfg.scopes.clone(),
        field_mapping: cfg.field_mapping.clone(),
    }
}

fn rand_b64(n: usize) -> String {
    use base64::Engine as _;
    let mut buf = vec![0u8; n];
    use rand::TryRng as _;
    rand::rngs::SysRng.try_fill_bytes(&mut buf).expect("os rng");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&buf)
}
