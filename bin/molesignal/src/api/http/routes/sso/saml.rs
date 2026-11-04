// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! SAML SP 登录端点。
//!
//! - `GET /auth/sso/saml/login`：定位 SAML provider → 构造 `SamlLoginFlow` →
//!   生成 RelayState 存 [`crate::infra::sso::SsoStateStore`] → 跳到 IdP（HTTP-Redirect binding）。
//! - `POST /auth/sso/saml/callback`：接 IdP form post（`SAMLResponse` + `RelayState`）
//!   → 校验 RelayState → 解 Response → 提取 NameID/email/groups → 开户或登录 →
//!   经 [`super::handoff`] 回到前端。[`super::signin_test`] 发起的测试登录也回到这里：验断言的
//!   那段相同，结果只记进测试，不登录。
//!
//! 签名校验：`SamlLoginFlow::parse_response` 调 `xmldsig::verify_assertion_signature` 做
//! 真实 XMLDSig（enveloped signature + RSA-SHA256 + reference digest + 内嵌 cert 与配置
//! cert 公钥比对）。限制：未实现 exclusive C14N（xml-exc-c14n），对主流 IdP
//! （Azure AD / Okta / Keycloak / ADFS）的 deterministic 输出稳定通过；非规范化 XML 会被拒。

use axum::{
    Form, Router,
    extract::{Query, State},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::Deserialize;

use super::{
    LoginParams, handoff,
    provision::{self, FederatedIdentity, SignIn, sign_in},
    random_token, resolve_provider_for_login, safe_next, signin_test,
};
use crate::{
    api::AppState,
    domain::iam::{SsoProvider, SsoProviderConfig, SsoProviderKind, SsoSamlConfig},
    infra::sso::{SamlConfig, SamlLoginFlow, SsoStateEntry},
    shared::{Error, Result},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/sso/saml/login", get(login))
        .route("/auth/sso/saml/callback", post(callback))
}

async fn login(State(state): State<AppState>, Query(params): Query<LoginParams>) -> Response {
    let next = safe_next(params.next.as_deref());

    match authorization_url(&state, &params, next.clone()).await {
        Ok(url) => Redirect::to(&url).into_response(),
        Err(error) => handoff::finish(&state, Err(error), next).await,
    }
}

async fn authorization_url(
    state: &AppState,
    params: &LoginParams,
    next: Option<String>,
) -> Result<String> {
    let provider = resolve_provider_for_login(state, params, &[SsoProviderKind::Saml]).await?;
    let relay = random_token();
    let url = redirect_url(&provider, &relay)?;

    // SAML 没有 OIDC 的 nonce，nonce 槽位存空字符串。
    state
        .iam
        .sso_state_store
        .put(
            &relay,
            &SsoStateEntry {
                provider_id: provider.id.0,
                nonce: String::new(),
                next,
                test_id: None,
            },
        )
        .await?;

    Ok(url)
}

/// 浏览器要去的 IdP 地址。登录和测试登录用的是同一个。
pub(super) fn redirect_url(provider: &SsoProvider, relay: &str) -> Result<String> {
    let SsoProviderConfig::Saml(config) = &provider.config else {
        return Err(Error::invalid(format!(
            "provider {} is not SAML",
            provider.name
        )));
    };

    SamlLoginFlow::new(to_flow_config(config)).build_authz_url(relay)
}

#[derive(Debug, Deserialize)]
struct CallbackForm {
    #[serde(default, rename = "SAMLResponse")]
    saml_response: Option<String>,
    #[serde(default, rename = "RelayState")]
    relay_state: Option<String>,
}

async fn callback(State(state): State<AppState>, Form(form): Form<CallbackForm>) -> Response {
    let pending = match form.relay_state.as_deref() {
        Some(relay) => state.iam.sso_state_store.take(relay).await,
        None => Ok(None),
    };

    // 测试登录共用这个回调：只记结果，不登录。
    if let Ok(Some(SsoStateEntry {
        test_id: Some(test_id),
        ..
    })) = &pending
    {
        return test_callback(&state, test_id, form.saml_response).await;
    }

    let next = pending
        .as_ref()
        .ok()
        .and_then(Option::as_ref)
        .and_then(|entry| entry.next.clone());

    let outcome = finish_login(&state, pending, form.saml_response).await;

    handoff::finish(&state, outcome, next).await
}

async fn finish_login(
    state: &AppState,
    pending: Result<Option<SsoStateEntry>>,
    saml_response: Option<String>,
) -> Result<SignIn> {
    let pending = pending?.ok_or_else(|| {
        Error::unauthorized("the sign-in request has expired or was already used; sign in again")
    })?;

    let saml_response =
        saml_response.ok_or_else(|| Error::unauthorized("callback missing SAMLResponse"))?;

    let provider = provision::enabled_provider(state, &pending.provider_id).await?;
    let identity = identity_from_response(&provider, &saml_response)?;

    sign_in(state, &provider, identity).await
}

async fn test_callback(state: &AppState, test_id: &str, saml_response: Option<String>) -> Response {
    let Some(run) = signin_test::Run::open(state, test_id).await else {
        return signin_test::Run::lost();
    };

    let outcome = test_identity(&run, saml_response);

    run.conclude(state, outcome, &[]).await
}

fn test_identity(
    run: &signin_test::Run,
    saml_response: Option<String>,
) -> Result<FederatedIdentity> {
    run.check_settings()?;

    let saml_response =
        saml_response.ok_or_else(|| Error::unauthorized("callback missing SAMLResponse"))?;

    // 身份源回答失败时响应里没有断言，验签只能说找不到。把它的原话告诉管理员。
    if let Some(status) = SamlLoginFlow::failed_status(&saml_response) {
        return Err(Error::unauthorized(format!(
            "the identity provider answered {status}"
        )));
    }

    identity_from_response(run.provider(), &saml_response)
}

/// 验过断言，取出身份。登录和测试登录走的是同一段。
fn identity_from_response(
    provider: &SsoProvider,
    saml_response: &str,
) -> Result<FederatedIdentity> {
    let SsoProviderConfig::Saml(config) = &provider.config else {
        return Err(Error::invalid(format!(
            "provider {} is not SAML; wrong callback endpoint",
            provider.name
        )));
    };

    let assertion = SamlLoginFlow::new(to_flow_config(config)).parse_response(saml_response)?;

    Ok(FederatedIdentity {
        display_name: assertion.name.unwrap_or_else(|| assertion.email.clone()),
        email: assertion.email,
        subject: assertion.subject,
        groups: assertion.groups,
        email_verified: None,
    })
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
