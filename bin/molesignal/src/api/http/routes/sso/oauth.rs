// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! OIDC 与 OAuth 2.0 的浏览器跳转登录。
//!
//! `GET /auth/sso/login` 存下 `state` / `nonce` / `next` 后跳到身份源；
//! `GET /auth/sso/callback` 校验 `state`，用授权码换身份，开户或登录后经
//! [`handoff`] 回到前端。两种协议共用一个回调地址。[`signin_test`] 发起的测试登录也回到这里：
//! 换身份的那段相同，结果只记进测试，不登录。

use axum::{
    Router,
    extract::{Query, State},
    response::{IntoResponse, Redirect, Response},
    routing::get,
};
use serde::Deserialize;
use url::Url;

use super::{
    LoginParams, handoff,
    provision::{self, FederatedIdentity, SignIn, sign_in},
    random_token, resolve_provider_for_login, safe_next, signin_test,
};
use crate::{
    api::AppState,
    domain::iam::{SsoProvider, SsoProviderConfig, SsoProviderKind},
    infra::sso::{OAuth2LoginFlow, OidcLoginFlow, SsoStateEntry},
    shared::{Error, Result},
};

const KINDS: [SsoProviderKind; 2] = [SsoProviderKind::Oidc, SsoProviderKind::Oauth2];

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/sso/login", get(login))
        .route("/auth/sso/callback", get(callback))
}

async fn login(State(state): State<AppState>, Query(params): Query<LoginParams>) -> Response {
    let next = safe_next(params.next.as_deref());

    match authorization_url(&state, &params, next.clone()).await {
        Ok(url) => Redirect::to(url.as_str()).into_response(),
        Err(error) => handoff::finish(&state, Err(error), next).await,
    }
}

async fn authorization_url(
    state: &AppState,
    params: &LoginParams,
    next: Option<String>,
) -> Result<Url> {
    let provider = resolve_provider_for_login(state, params, &KINDS).await?;
    let sso_state = random_token();
    let nonce = random_token();
    let url = redirect_url(&provider, &sso_state, &nonce)?;

    state
        .iam
        .sso_state_store
        .put(
            &sso_state,
            &SsoStateEntry {
                provider_id: provider.id.0,
                nonce,
                next,
                test_id: None,
            },
        )
        .await?;

    Ok(url)
}

/// 浏览器要去的授权地址。登录和测试登录用的是同一个。
pub(super) fn redirect_url(provider: &SsoProvider, sso_state: &str, nonce: &str) -> Result<Url> {
    match &provider.config {
        SsoProviderConfig::Oidc(config) => {
            OidcLoginFlow::new(config)?.authorization_url(sso_state, nonce)
        }
        SsoProviderConfig::Oauth2(config) => {
            OAuth2LoginFlow::new(config, provider.platform)?.authorization_url(sso_state)
        }
        SsoProviderConfig::Saml(_) | SsoProviderConfig::Ldap(_) => Err(Error::invalid(format!(
            "provider {} is not OIDC or OAuth 2.0",
            provider.name
        ))),
    }
}

#[derive(Debug, Deserialize)]
struct CallbackParams {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

async fn callback(State(state): State<AppState>, Query(params): Query<CallbackParams>) -> Response {
    let pending = match params.state.as_deref() {
        Some(value) => state.iam.sso_state_store.take(value).await,
        None => Ok(None),
    };

    // 测试登录共用这个回调：只记结果，不登录。
    if let Ok(Some(SsoStateEntry {
        test_id: Some(test_id),
        nonce,
        ..
    })) = &pending
    {
        return test_callback(&state, test_id, nonce, params).await;
    }

    let next = pending
        .as_ref()
        .ok()
        .and_then(Option::as_ref)
        .and_then(|entry| entry.next.clone());

    let outcome = finish_login(&state, pending, params).await;

    handoff::finish(&state, outcome, next).await
}

async fn finish_login(
    state: &AppState,
    pending: Result<Option<SsoStateEntry>>,
    params: CallbackParams,
) -> Result<SignIn> {
    if let Some(error) = idp_error(&params) {
        return Err(error);
    }

    let pending = pending?.ok_or_else(|| {
        Error::unauthorized("the sign-in request has expired or was already used; sign in again")
    })?;

    let code = returned_code(params)?;
    let provider = provision::enabled_provider(state, &pending.provider_id).await?;
    let identity = identity_from_code(state, &provider, &code, &pending.nonce).await?;

    sign_in(state, &provider, identity).await
}

async fn test_callback(
    state: &AppState,
    test_id: &str,
    nonce: &str,
    params: CallbackParams,
) -> Response {
    let Some(run) = signin_test::Run::open(state, test_id).await else {
        return signin_test::Run::lost();
    };

    let seen = [
        params.code.clone().unwrap_or_default(),
        params.state.clone().unwrap_or_default(),
        nonce.to_owned(),
    ];

    let outcome = test_identity(state, &run, nonce, params).await;

    run.conclude(state, outcome, &seen).await
}

async fn test_identity(
    state: &AppState,
    run: &signin_test::Run,
    nonce: &str,
    params: CallbackParams,
) -> Result<FederatedIdentity> {
    if let Some(error) = idp_error(&params) {
        return Err(error);
    }

    run.check_settings()?;

    let code = returned_code(params)?;

    identity_from_code(state, run.provider(), &code, nonce).await
}

/// 身份源回来时说的错误。
fn idp_error(params: &CallbackParams) -> Option<Error> {
    let error = params.error.as_deref()?;

    let detail = params
        .error_description
        .as_deref()
        .map(|description| format!(": {description}"))
        .unwrap_or_default();

    Some(Error::unauthorized(format!(
        "the identity provider returned {error}{detail}"
    )))
}

fn returned_code(params: CallbackParams) -> Result<String> {
    params
        .code
        .filter(|code| !code.is_empty())
        .ok_or_else(|| Error::unauthorized("the identity provider returned no authorization code"))
}

/// 用授权码换身份。登录和测试登录走的是同一段。
async fn identity_from_code(
    state: &AppState,
    provider: &SsoProvider,
    code: &str,
    nonce: &str,
) -> Result<FederatedIdentity> {
    let identity = match &provider.config {
        SsoProviderConfig::Oidc(config) => {
            OidcLoginFlow::new(config)?
                .identity(code, &state.iam.sso_jwks_cache, nonce)
                .await?
        }
        SsoProviderConfig::Oauth2(config) => {
            OAuth2LoginFlow::new(config, provider.platform)?
                .identity(code)
                .await?
        }
        SsoProviderConfig::Saml(_) | SsoProviderConfig::Ldap(_) => {
            return Err(Error::invalid(format!(
                "provider {} is not OIDC or OAuth 2.0",
                provider.name
            )));
        }
    };

    Ok(identity.into())
}
