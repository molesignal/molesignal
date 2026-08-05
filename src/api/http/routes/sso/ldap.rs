// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! LDAP credential login endpoint.

use axum::{Json, Router, extract::State, routing::post};
use serde::Deserialize;

use super::provision::{FederatedIdentity, FederatedLoginResponse, complete_login};
use crate::{
    api::AppState,
    domain::iam::{SsoLdapConfig, SsoProviderConfig, SsoProviderKind},
    infra::sso::{LdapConfig, LdapLoginFlow},
    shared::{Error, Result, ids::Id},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/auth/sso/ldap/login", post(login))
}

#[derive(Debug, Deserialize)]
struct LoginRequest {
    provider_id: String,
    username: String,
    password: String,
}

async fn login(
    State(state): State<AppState>,
    Json(request): Json<LoginRequest>,
) -> Result<Json<FederatedLoginResponse>> {
    if !state.platform.license.has_feature("sso") {
        return Err(Error::forbidden("sso feature not licensed"));
    }
    let provider = state
        .iam
        .sso_providers
        .get(&Id::from_string(request.provider_id))
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
    let response = complete_login(
        &state,
        &provider,
        FederatedIdentity {
            email: identity.email,
            display_name: identity.display_name,
            subject: identity.subject,
            groups: identity.groups,
        },
        &config.group_role_mapping,
        config.default_role_id.as_deref(),
    )
    .await?;
    Ok(Json(response))
}

fn to_flow_config(config: &SsoLdapConfig) -> LdapConfig {
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
