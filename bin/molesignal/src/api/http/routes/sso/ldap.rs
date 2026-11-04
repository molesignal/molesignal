// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! LDAP credential login endpoint.
//!
//! The answer has the shape of the redirect sign-ins' exchange: `{"login": …}` with the session
//! cookies set, or `{"link": …}` when the email already belongs to an account the directory
//! identity is not bound to yet. A directory sign-in only checks a password, so an account with
//! multi-factor authentication answers `{"mfa": …}` and waits for the second step, and too many
//! wrong passwords are refused with 429 before they reach the directory
//! ([`crate::app::iam::signin_limit`]).

use axum::{
    Json, Router,
    extract::State,
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::post,
};
use serde::Deserialize;

use super::provision::{FederatedIdentity, SignIn, sign_in};
use crate::{
    api::{AppState, http::signin_limit::SigninSource},
    app::iam::signin_limit::{SigninAccount, SigninError},
    domain::iam::{SsoLdapConfig, SsoProvider, SsoProviderConfig, SsoProviderKind},
    infra::sso::{LdapConfig, LdapLoginFlow, LdapUser},
    shared::{Error, Result, ids::Id},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/auth/sso/ldap/login", post(login))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoginRequest {
    provider_id: String,
    username: String,
    password: String,
    #[serde(default)]
    remember: bool,
}

async fn login(
    State(state): State<AppState>,
    SigninSource(source): SigninSource,
    headers: HeaderMap,
    Json(request): Json<LoginRequest>,
) -> Result<Response, SigninError> {
    let account = SigninAccount::Directory {
        provider_id: &request.provider_id,
        username: &request.username,
    };

    let attempt = state.iam.signin_limiter.begin(&account, source).await?;
    let outcome = bind(&state, &request).await;

    attempt.settle(&outcome).await;
    let (provider, identity) = outcome?;

    let response = sign_in(
        &state,
        &provider,
        FederatedIdentity {
            email: identity.email,
            display_name: identity.display_name,
            subject: identity.subject,
            groups: identity.groups,
            email_verified: None,
        },
    )
    .await?;

    // LDAP 只是核对目录里的密码：和密码登录一样，账号开了多因素认证就要第二步。
    match response {
        SignIn::Login(session) => Ok(crate::api::http::routes::mfa::finish_first_step(
            &state,
            &headers,
            session,
            request.remember,
        )
        .await?),
        link @ SignIn::Link(_) => Ok(Json(link).into_response()),
    }
}

/// 找到服务商，用这个用户名和密码绑定目录。
async fn bind(state: &AppState, request: &LoginRequest) -> Result<(SsoProvider, LdapUser)> {
    let provider = state
        .iam
        .sso_providers
        .get(&Id::from_string(request.provider_id.clone()))
        .await
        .map_err(|_| Error::unauthorized("invalid LDAP credentials"))?;

    if !provider.enabled || provider.kind != SsoProviderKind::Ldap {
        return Err(Error::unauthorized("invalid LDAP credentials"));
    }

    let config = match &provider.config {
        SsoProviderConfig::Ldap(config) => config.clone(),
        _ => return Err(Error::unauthorized("invalid LDAP credentials")),
    };

    let identity = LdapLoginFlow::new(to_flow_config(&config))?
        .authenticate(&request.username, &request.password)
        .await?;

    Ok((provider, identity))
}

pub(super) fn to_flow_config(config: &SsoLdapConfig) -> LdapConfig {
    LdapConfig {
        url: config.url.clone(),
        start_tls: config.start_tls,
        bind_dn: config.bind_dn.clone(),
        bind_password: config.bind_password.clone(),
        base_dn: config.base_dn.clone(),
        user_filter: config.user_filter.clone(),
        field_mapping: config.field_mapping.clone(),
    }
}
