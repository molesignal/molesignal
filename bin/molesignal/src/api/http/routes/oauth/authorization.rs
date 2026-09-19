// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use url::Url;

use super::{
    client::{normalize_scope, resolve_client, scope_allowed},
    metadata,
};
use crate::{
    agent::{
        FEATURE,
        inbound_mcp::{InboundMcpAuthorizationCode, InboundMcpSettings},
    },
    api::{
        AppState,
        http::middleware::{Permission, auth::AuthenticatedCredential},
    },
    app::iam::IamContext,
    shared::{Error, Result, time::TimestampMicros},
};

const AUTHORIZATION_CODE_TTL_MICROS: i64 = 5 * 60 * 1_000_000;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(super) struct AuthorizationRequest {
    response_type: String,
    client_id: String,
    redirect_uri: String,
    scope: String,
    state: Option<String>,
    code_challenge: String,
    code_challenge_method: String,
    resource: String,
}

#[derive(Debug, Deserialize)]
struct AuthorizationDecision {
    #[serde(flatten)]
    request: AuthorizationRequest,
    approve: bool,
}

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/oauth/authorization-request", get(inspect_request))
        .route("/oauth/authorize", post(authorize))
}

async fn require_available(state: &AppState, ctx: &IamContext) -> Result<InboundMcpSettings> {
    if !state.platform.license.has_feature(FEATURE) {
        return Err(Error::forbidden("Inbound MCP requires the agent feature"));
    }
    Permission::require_key(ctx, "agent.use")?;
    let settings = state
        .agent
        .inbound_mcp
        .get_settings(&ctx.org_id)
        .await?
        .unwrap_or_else(|| InboundMcpSettings::defaults(ctx.org_id.clone()));
    if !settings.enabled {
        return Err(Error::forbidden(
            "Inbound MCP is disabled for this organization",
        ));
    }
    Ok(settings)
}

async fn validate_request(
    state: &AppState,
    headers: &HeaderMap,
    request: &AuthorizationRequest,
) -> Result<super::client::ResolvedOAuthClient> {
    if request.response_type != "code" || request.code_challenge_method != "S256" {
        return Err(Error::invalid(
            "OAuth authorization requires response_type=code and code_challenge_method=S256",
        ));
    }
    if request.code_challenge.len() != 43
        || !request
            .code_challenge
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(Error::invalid("invalid PKCE code_challenge"));
    }
    if request
        .state
        .as_ref()
        .is_some_and(|state| state.len() > 2_048)
    {
        return Err(Error::invalid("OAuth state exceeds 2048 characters"));
    }
    let expected_resource = metadata::resource_uri(state, headers)?;
    if request.resource != expected_resource {
        return Err(Error::invalid(
            "OAuth resource must exactly match the Inbound MCP endpoint",
        ));
    }
    let client = resolve_client(state, &request.client_id).await?;
    if !client
        .grant_types
        .iter()
        .any(|grant| grant == "authorization_code")
    {
        return Err(Error::invalid(
            "OAuth client is not registered for authorization_code",
        ));
    }
    if !client.redirect_uris.contains(&request.redirect_uri) {
        return Err(Error::invalid(
            "redirect_uri is not registered for this OAuth client",
        ));
    }
    let scope = normalize_scope(&request.scope).map_err(Error::invalid)?;
    if scope
        .split_ascii_whitespace()
        .any(|value| value == "offline_access")
        && !client
            .grant_types
            .iter()
            .any(|grant| grant == "refresh_token")
    {
        return Err(Error::invalid(
            "offline_access requires the client refresh_token grant",
        ));
    }
    if !scope_allowed(&scope, &client.scope) {
        return Err(Error::forbidden(
            "requested OAuth scope exceeds the client registration",
        ));
    }
    Ok(client)
}

async fn inspect_request(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Extension(credential): Extension<AuthenticatedCredential>,
    headers: HeaderMap,
    Query(request): Query<AuthorizationRequest>,
) -> Result<Response> {
    require_interactive_user(&credential)?;
    let _ = require_available(&state, &ctx).await?;
    let client = validate_request(&state, &headers, &request).await?;
    let organization = state.iam.service.orgs.get(&ctx.org_id).await?;
    Ok(no_store_json(json!({
        "client": {
            "client_id": client.client_id,
            "client_name": client.client_name,
            "client_uri": client.client_uri,
            "metadata_document": client.is_metadata_document,
        },
        "scope": normalize_scope(&request.scope).map_err(Error::invalid)?,
        "resource": request.resource,
        "redirect_uri": request.redirect_uri,
        "state": request.state,
        "organization": {
            "id": organization.id,
            "name": organization.name,
        },
    })))
}

async fn authorize(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Extension(credential): Extension<AuthenticatedCredential>,
    headers: HeaderMap,
    Json(decision): Json<AuthorizationDecision>,
) -> Result<Response> {
    require_interactive_user(&credential)?;
    let _ = require_available(&state, &ctx).await?;
    let client = validate_request(&state, &headers, &decision.request).await?;
    let issuer = metadata::external_base(&state, &headers)?;
    if !decision.approve {
        return Ok(no_store_json(json!({
            "redirect_to": redirect_response(
                &decision.request.redirect_uri,
                &issuer,
                None,
                Some("access_denied"),
                decision.request.state.as_deref(),
            )?,
        })));
    }
    let now = TimestampMicros::now();
    let plaintext = format!("mscode_{}", super::token::random_secret(32));
    let scope = normalize_scope(&decision.request.scope).map_err(Error::invalid)?;
    state
        .agent
        .inbound_mcp
        .create_authorization_code(InboundMcpAuthorizationCode {
            code_hash: crate::api::http::middleware::auth::oauth::token_hash(&plaintext),
            client_id: client.client_id,
            org_id: ctx.org_id,
            user_id: ctx.user_id,
            redirect_uri: decision.request.redirect_uri.clone(),
            scope,
            resource: decision.request.resource,
            code_challenge: decision.request.code_challenge,
            code_challenge_method: decision.request.code_challenge_method,
            expires_at: TimestampMicros(now.0.saturating_add(AUTHORIZATION_CODE_TTL_MICROS)),
            created_at: now,
        })
        .await?;
    Ok(no_store_json(json!({
        "redirect_to": redirect_response(
            &decision.request.redirect_uri,
            &issuer,
            Some(&plaintext),
            None,
            decision.request.state.as_deref(),
        )?,
    })))
}

fn no_store_json(value: Value) -> Response {
    (
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::PRAGMA, "no-cache"),
        ],
        Json(value),
    )
        .into_response()
}

fn require_interactive_user(credential: &AuthenticatedCredential) -> Result<()> {
    if matches!(credential, AuthenticatedCredential::UserJwt) {
        Ok(())
    } else {
        Err(Error::forbidden(
            "OAuth authorization requires an interactive user session",
        ))
    }
}

fn redirect_response(
    redirect_uri: &str,
    issuer: &str,
    code: Option<&str>,
    error: Option<&str>,
    state: Option<&str>,
) -> Result<String> {
    let mut url = Url::parse(redirect_uri)
        .map_err(|parse_error| Error::invalid(format!("invalid redirect URI: {parse_error}")))?;
    {
        let mut query = url.query_pairs_mut();
        if let Some(code) = code {
            query.append_pair("code", code);
        }
        if let Some(error) = error {
            query.append_pair("error", error);
        }
        if let Some(state) = state {
            query.append_pair("state", state);
        }
        query.append_pair("iss", issuer);
    }
    Ok(url.into())
}
