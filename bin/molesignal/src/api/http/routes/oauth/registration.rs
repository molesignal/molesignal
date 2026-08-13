// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use serde::{Deserialize, Serialize};

use super::{
    OAuthError, OAuthResult,
    client::{validate_client_uri, validate_redirect_uris},
};
use crate::{
    agent::{FEATURE, inbound_mcp::NewInboundMcpOAuthClient},
    api::AppState,
    app::iam::hash_password,
};

#[derive(Debug, Deserialize)]
struct RegistrationRequest {
    client_name: String,
    redirect_uris: Vec<String>,
    #[serde(default = "default_grant_types")]
    grant_types: Vec<String>,
    #[serde(default = "default_response_types")]
    response_types: Vec<String>,
    #[serde(default = "default_auth_method")]
    token_endpoint_auth_method: String,
    #[serde(default = "default_scope")]
    scope: String,
    #[serde(default)]
    client_uri: Option<String>,
    #[serde(default)]
    software_id: Option<String>,
    #[serde(default)]
    software_version: Option<String>,
}

#[derive(Debug, Serialize)]
struct RegistrationResponse {
    client_id: String,
    client_id_issued_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_secret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_secret_expires_at: Option<i64>,
    client_name: String,
    redirect_uris: Vec<String>,
    grant_types: Vec<String>,
    response_types: Vec<String>,
    token_endpoint_auth_method: String,
    scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    software_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    software_version: Option<String>,
}

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/oauth/register", post(register))
}

fn default_grant_types() -> Vec<String> {
    vec!["authorization_code".into(), "refresh_token".into()]
}

fn default_response_types() -> Vec<String> {
    vec!["code".into()]
}

fn default_auth_method() -> String {
    "none".into()
}

fn default_scope() -> String {
    "mcp offline_access".into()
}

async fn register(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<RegistrationRequest>,
) -> OAuthResult<Response> {
    if !state.platform.license.has_feature(FEATURE) {
        return Err(OAuthError::invalid_request(
            "Inbound MCP requires the agent feature",
        ));
    }
    super::metadata::external_base(&state, &headers)
        .map_err(|error| OAuthError::invalid_request(error.to_string()))?;
    let client_name = request.client_name.trim();
    if client_name.is_empty() || client_name.chars().count() > 255 {
        return Err(OAuthError::invalid_request(
            "client_name must contain 1 to 255 characters",
        ));
    }
    validate_redirect_uris(&request.redirect_uris).map_err(OAuthError::invalid_request)?;
    if request.grant_types.is_empty()
        || !request
            .grant_types
            .iter()
            .any(|value| value == "authorization_code")
        || request
            .grant_types
            .iter()
            .any(|value| !matches!(value.as_str(), "authorization_code" | "refresh_token"))
        || request.response_types.len() != 1
        || request
            .response_types
            .first()
            .is_none_or(|value| value != "code")
    {
        return Err(OAuthError::invalid_request(
            "only authorization_code, refresh_token, and response_type code are supported",
        ));
    }
    validate_client_uri(request.client_uri.as_deref()).map_err(OAuthError::invalid_request)?;
    if request
        .software_id
        .as_ref()
        .is_some_and(|value| value.is_empty() || value.len() > 255)
        || request
            .software_version
            .as_ref()
            .is_some_and(|value| value.is_empty() || value.len() > 255)
    {
        return Err(OAuthError::invalid_request(
            "software_id and software_version must contain 1 to 255 bytes when present",
        ));
    }
    if !matches!(
        request.token_endpoint_auth_method.as_str(),
        "none" | "client_secret_basic" | "client_secret_post"
    ) {
        return Err(OAuthError::invalid_request(
            "unsupported token_endpoint_auth_method",
        ));
    }
    let scopes =
        super::client::normalize_scope(&request.scope).map_err(OAuthError::invalid_request)?;
    let client_id = format!("msmcp_client_{}", super::token::random_secret(18));
    let client_secret = (request.token_endpoint_auth_method != "none")
        .then(|| format!("msmcp_secret_{}", super::token::random_secret(32)));
    let secret_hash = client_secret
        .as_deref()
        .map(hash_password)
        .transpose()
        .map_err(OAuthError::server)?;
    let issued_at = chrono::Utc::now().timestamp();
    let saved = state
        .agent
        .inbound_mcp
        .create_oauth_client(NewInboundMcpOAuthClient {
            client_id,
            client_name: client_name.into(),
            redirect_uris: request.redirect_uris,
            grant_types: request.grant_types,
            response_types: request.response_types,
            token_endpoint_auth_method: request.token_endpoint_auth_method,
            scope: scopes,
            client_secret_hash: secret_hash,
            client_id_issued_at: issued_at,
            client_secret_expires_at: None,
            client_uri: request.client_uri,
            software_id: request.software_id,
            software_version: request.software_version,
        })
        .await
        .map_err(OAuthError::server)?;
    let client_secret_expires_at = client_secret
        .as_ref()
        .map(|_| saved.client_secret_expires_at.unwrap_or(0));
    Ok((
        StatusCode::CREATED,
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::PRAGMA, "no-cache"),
        ],
        Json(RegistrationResponse {
            client_id: saved.client_id,
            client_id_issued_at: saved.client_id_issued_at,
            client_secret,
            client_secret_expires_at,
            client_name: saved.client_name,
            redirect_uris: saved.redirect_uris,
            grant_types: saved.grant_types,
            response_types: saved.response_types,
            token_endpoint_auth_method: saved.token_endpoint_auth_method,
            scope: saved.scope,
            client_uri: saved.client_uri,
            software_id: saved.software_id,
            software_version: saved.software_version,
        }),
    )
        .into_response())
}
