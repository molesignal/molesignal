// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! SAML SP 登录端点。
//!
//! - `GET /auth/sso/saml/login`：定位 SAML provider → 构造 `SamlLoginFlow` →
//!   生成 RelayState 存 [`SsoStateStore`] → 302 到 IdP（HTTP-Redirect binding）。
//! - `POST /auth/sso/saml/callback`：接 IdP form post（`SAMLResponse` + `RelayState`）
//!   → 校验 RelayState → 解 Response → 提取 NameID/email/groups → provision_or_get_user
//!   → 发本地 JWT。
//!
//! 签名校验：`SamlLoginFlow::parse_response` 调 `xmldsig::verify_assertion_signature` 做
//! 真实 XMLDSig（enveloped signature + RSA-SHA256 + reference digest + 内嵌 cert 与配置
//! cert 公钥比对）。限制：未实现 exclusive C14N（xml-exc-c14n），对主流 IdP
//! （Azure AD / Okta / Keycloak / ADFS）的 deterministic 输出稳定通过；非规范化 XML 会被拒。

use axum::{
    Form, Json, Router,
    extract::{Query, State},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::Deserialize;

use super::provision::{FederatedIdentity, FederatedLoginResponse, complete_login};
use crate::{
    api::AppState,
    domain::iam::{SsoProvider, SsoProviderConfig, SsoProviderKind, SsoSamlConfig},
    infra::sso::{SamlConfig, SamlLoginFlow},
    shared::{Error, Result, ids::Id},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/sso/saml/login", get(login))
        .route("/auth/sso/saml/callback", post(callback))
}

#[derive(Debug, Deserialize)]
pub struct LoginParams {
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
}

async fn login(State(state): State<AppState>, Query(p): Query<LoginParams>) -> Result<Response> {
    if !state.platform.license.has_feature("sso") {
        return Err(Error::forbidden("sso feature not licensed"));
    }
    let provider = resolve_provider_for_login(&state, &p).await?;
    if provider.kind != SsoProviderKind::Saml {
        return Err(Error::invalid(format!(
            "provider {} is kind={:?}; /auth/sso/saml/login expects SAML",
            provider.id.0, provider.kind
        )));
    }
    let saml_cfg = match &provider.config {
        SsoProviderConfig::Saml(c) => c.clone(),
        _ => unreachable!("kind == Saml enforced above"),
    };
    let flow = SamlLoginFlow::new(to_flow_config(&saml_cfg));
    let relay = rand_b64(24);
    // nonce 槽位塞空字符串 — SAML 没 OIDC 的 nonce 概念，但复用 store。
    state
        .iam
        .sso_state_store
        .put(relay.clone(), provider.id.0.clone(), String::new());
    let url = flow.build_authz_url(&relay)?;
    Ok(Redirect::temporary(&url).into_response())
}

#[derive(Debug, Deserialize)]
pub struct CallbackForm {
    #[serde(default, rename = "SAMLResponse")]
    pub saml_response: Option<String>,
    #[serde(default, rename = "RelayState")]
    pub relay_state: Option<String>,
}

async fn callback(
    State(state): State<AppState>,
    Form(form): Form<CallbackForm>,
) -> Result<Json<FederatedLoginResponse>> {
    if !state.platform.license.has_feature("sso") {
        return Err(Error::forbidden("sso feature not licensed"));
    }
    let saml_response = form
        .saml_response
        .ok_or_else(|| Error::Unauthorized("callback missing SAMLResponse".into()))?;
    let relay = form
        .relay_state
        .ok_or_else(|| Error::Unauthorized("callback missing RelayState".into()))?;
    let entry = state
        .iam
        .sso_state_store
        .take(&relay)
        .ok_or_else(|| Error::Unauthorized("invalid or expired RelayState".into()))?;
    let provider = state
        .iam
        .sso_providers
        .get(&Id::from_string(entry.provider_id))
        .await
        .map_err(|_| Error::Unauthorized("sso provider not found".into()))?;
    if !provider.enabled {
        return Err(Error::forbidden(format!(
            "provider {} is disabled",
            provider.id.0
        )));
    }
    let saml_cfg = match &provider.config {
        SsoProviderConfig::Saml(c) => c.clone(),
        _ => {
            return Err(Error::invalid(format!(
                "provider {} is not SAML; wrong callback endpoint",
                provider.id.0
            )));
        }
    };

    let flow = SamlLoginFlow::new(to_flow_config(&saml_cfg));
    let assertion = flow.parse_response(&saml_response)?;

    let response = complete_login(
        &state,
        &provider,
        FederatedIdentity {
            email: assertion.email.clone(),
            display_name: assertion.name.unwrap_or_else(|| assertion.email.clone()),
            subject: assertion.subject,
            groups: assertion.groups,
        },
        &saml_cfg.group_role_mapping,
        saml_cfg.default_role_id.as_deref(),
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
        .list_enabled_by_kind(SsoProviderKind::Saml)
        .await?;
    if let Some(name) = p.provider.as_deref() {
        candidates.retain(|c| c.name == name);
    }
    if candidates.is_empty() {
        return Err(Error::invalid(
            "no enabled SAML provider configured; create one via POST /api/v1/sso/providers",
        ));
    }
    if candidates.len() > 1 {
        let names: Vec<String> = candidates.iter().map(|c| c.name.clone()).collect();
        return Err(Error::invalid(format!(
            "multiple enabled SAML providers ({}); add ?provider_id=<id> to choose",
            names.join(", ")
        )));
    }
    Ok(candidates.remove(0))
}

fn to_flow_config(cfg: &SsoSamlConfig) -> SamlConfig {
    SamlConfig {
        sp_entity_id: cfg.sp_entity_id.clone(),
        idp_entity_id: cfg.idp_entity_id.clone(),
        idp_sso_url: cfg.idp_sso_url.clone(),
        idp_x509_cert: cfg.idp_x509_cert.clone(),
        assertion_consumer_url: cfg.assertion_consumer_url.clone(),
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
