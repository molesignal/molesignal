// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Form, Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use rand::TryRng as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    OAuthError, OAuthResult,
    client::{
        ClientAuthenticationMethod, authenticate_client, normalize_scope, resolve_client,
        scope_allowed,
    },
    metadata,
};
use crate::{
    agent::inbound_mcp::InboundMcpOAuthTokenKind,
    api::AppState,
    shared::{ids::Id, time::TimestampMicros},
};

mod issuance;

use issuance::{TokenRotation, ensure_grant_available, issue_tokens};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TokenRequest {
    grant_type: String,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    redirect_uri: Option<String>,
    #[serde(default)]
    code_verifier: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    client_id: Option<String>,
    #[serde(default)]
    client_secret: Option<String>,
    #[serde(default)]
    resource: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RevocationRequest {
    token: String,
    #[serde(default)]
    token_type_hint: Option<String>,
    #[serde(default)]
    client_id: Option<String>,
    #[serde(default)]
    client_secret: Option<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct TokenResponse {
    access_token: String,
    token_type: &'static str,
    expires_in: i64,
    scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh_token: Option<String>,
}

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/oauth/token", post(exchange))
        .route("/oauth/revoke", post(revoke))
}

pub(super) fn random_secret(bytes: usize) -> String {
    let mut value = vec![0u8; bytes];
    rand::rngs::SysRng
        .try_fill_bytes(&mut value)
        .expect("operating system RNG must be available");
    URL_SAFE_NO_PAD.encode(value)
}

async fn exchange(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(request): Form<TokenRequest>,
) -> OAuthResult<Response> {
    let response = match request.grant_type.as_str() {
        "authorization_code" => exchange_authorization_code(&state, &headers, request).await?,
        "refresh_token" => exchange_refresh_token(&state, &headers, request).await?,
        _ => return Err(OAuthError::unsupported_grant_type()),
    };
    Ok(no_store_json(response))
}

async fn exchange_authorization_code(
    state: &AppState,
    headers: &HeaderMap,
    request: TokenRequest,
) -> OAuthResult<TokenResponse> {
    if request.refresh_token.is_some() || request.scope.is_some() {
        return Err(OAuthError::invalid_request(
            "authorization_code requests must not include refresh_token or scope",
        ));
    }
    let code = required(request.code, "code")?;
    validate_credential(&code, "mscode_", "authorization code")?;
    let redirect_uri = required(request.redirect_uri, "redirect_uri")?;
    let verifier = required(request.code_verifier, "code_verifier")?;
    let resource = required(request.resource, "resource")?;
    let (client_id, client_secret, authentication_method) =
        client_credentials(headers, request.client_id, request.client_secret)?;
    let client = resolve_client(state, &client_id)
        .await
        .map_err(|error| OAuthError::invalid_client(error.to_string()))?;
    authenticate_client(&client, authentication_method, client_secret.as_deref())
        .map_err(|error| OAuthError::invalid_client(error.to_string()))?;
    let expected_resource = metadata::resource_uri(state, headers)
        .map_err(|error| OAuthError::invalid_grant(error.to_string()))?;
    if resource != expected_resource {
        return Err(OAuthError::invalid_grant(
            "authorization code resource does not match this endpoint",
        ));
    }
    let code_challenge = pkce_challenge(&verifier)
        .ok_or_else(|| OAuthError::invalid_grant("PKCE code_verifier is malformed"))?;
    let now = TimestampMicros::now();
    let grant = state
        .agent
        .inbound_mcp
        .consume_authorization_code(
            &crate::api::http::middleware::auth::oauth::token_hash(&code),
            &client.client_id,
            &redirect_uri,
            &resource,
            &code_challenge,
            now,
        )
        .await
        .map_err(OAuthError::server)?
        .ok_or_else(|| OAuthError::invalid_grant("authorization code is invalid or expired"))?;
    if grant.client_id != client.client_id
        || grant.redirect_uri != redirect_uri
        || grant.resource != resource
        || !verify_pkce(&verifier, &grant.code_challenge)
    {
        return Err(OAuthError::invalid_grant(
            "authorization code binding or PKCE verification failed",
        ));
    }
    ensure_grant_available(state, &grant.org_id, &grant.user_id).await?;
    issue_tokens(
        state,
        client.client_id,
        grant.org_id,
        grant.user_id,
        grant.scope,
        grant.resource,
        Id::new(),
        None,
        now,
    )
    .await
}

async fn exchange_refresh_token(
    state: &AppState,
    headers: &HeaderMap,
    request: TokenRequest,
) -> OAuthResult<TokenResponse> {
    if request.code.is_some() || request.redirect_uri.is_some() || request.code_verifier.is_some() {
        return Err(OAuthError::invalid_request(
            "refresh_token requests must not include authorization-code fields",
        ));
    }
    let plaintext = required(request.refresh_token, "refresh_token")?;
    validate_credential(&plaintext, "msrefresh_", "refresh token")?;
    let requested_resource = required(request.resource, "resource")?;
    let (client_id, client_secret, authentication_method) =
        client_credentials(headers, request.client_id, request.client_secret)?;
    let client = resolve_client(state, &client_id)
        .await
        .map_err(|error| OAuthError::invalid_client(error.to_string()))?;
    authenticate_client(&client, authentication_method, client_secret.as_deref())
        .map_err(|error| OAuthError::invalid_client(error.to_string()))?;
    if !client
        .grant_types
        .iter()
        .any(|grant| grant == "refresh_token")
    {
        return Err(OAuthError::invalid_grant(
            "OAuth client is not registered for refresh_token",
        ));
    }
    let token_hash = crate::api::http::middleware::auth::oauth::token_hash(&plaintext);
    let record = state
        .agent
        .inbound_mcp
        .find_oauth_token(&token_hash)
        .await
        .map_err(OAuthError::server)?
        .ok_or_else(|| OAuthError::invalid_grant("refresh token is invalid"))?;
    let now = TimestampMicros::now();
    let expected_resource = metadata::resource_uri(state, headers)
        .map_err(|error| OAuthError::invalid_grant(error.to_string()))?;
    if record.revoked_at.is_some() {
        state
            .agent
            .inbound_mcp
            .revoke_oauth_family(&record.org_id, &record.family_id, now)
            .await
            .map_err(OAuthError::server)?;
        return Err(OAuthError::invalid_grant(
            "refresh token reuse was detected; the token family was revoked",
        ));
    }
    if record.token_kind != InboundMcpOAuthTokenKind::Refresh
        || record.expires_at.0 <= now.0
        || record.client_id != client.client_id
        || record.resource != requested_resource
        || requested_resource != expected_resource
    {
        return Err(OAuthError::invalid_grant(
            "refresh token is expired or does not match this client and resource",
        ));
    }
    let scope = match request.scope {
        Some(scope) => {
            let normalized = normalize_scope(&scope).map_err(OAuthError::invalid_scope)?;
            if !scope_allowed(&normalized, &record.scope) {
                return Err(OAuthError::invalid_grant(
                    "refresh scope exceeds the original grant",
                ));
            }
            normalized
        }
        None => record.scope.clone(),
    };
    ensure_grant_available(state, &record.org_id, &record.user_id).await?;
    issue_tokens(
        state,
        record.client_id,
        record.org_id,
        record.user_id,
        scope,
        record.resource,
        record.family_id,
        Some(TokenRotation {
            token_hash,
            token_id: record.id,
        }),
        now,
    )
    .await
}

async fn revoke(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(request): Form<RevocationRequest>,
) -> OAuthResult<Response> {
    let _ = request.token_type_hint;
    let (client_id, client_secret, authentication_method) =
        client_credentials(&headers, request.client_id, request.client_secret)?;
    let client = resolve_client(&state, &client_id)
        .await
        .map_err(|error| OAuthError::invalid_client(error.to_string()))?;
    authenticate_client(&client, authentication_method, client_secret.as_deref())
        .map_err(|error| OAuthError::invalid_client(error.to_string()))?;
    if request.token.len() > 256 {
        return Ok(no_store_status(StatusCode::OK));
    }
    let token_hash = crate::api::http::middleware::auth::oauth::token_hash(&request.token);
    let Some(record) = state
        .agent
        .inbound_mcp
        .find_oauth_token(&token_hash)
        .await
        .map_err(OAuthError::server)?
    else {
        return Ok(no_store_status(StatusCode::OK));
    };
    if client_id != record.client_id {
        return Ok(no_store_status(StatusCode::OK));
    }
    let now = TimestampMicros::now();
    if record.token_kind == InboundMcpOAuthTokenKind::Refresh {
        state
            .agent
            .inbound_mcp
            .revoke_oauth_family(&record.org_id, &record.family_id, now)
            .await
            .map_err(OAuthError::server)?;
    } else {
        state
            .agent
            .inbound_mcp
            .revoke_oauth_token(&token_hash, now)
            .await
            .map_err(OAuthError::server)?;
    }
    Ok(no_store_status(StatusCode::OK))
}

fn no_store_json(value: TokenResponse) -> Response {
    (
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::PRAGMA, "no-cache"),
        ],
        Json(value),
    )
        .into_response()
}

fn no_store_status(status: StatusCode) -> Response {
    (
        status,
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::PRAGMA, "no-cache"),
        ],
    )
        .into_response()
}

fn required(value: Option<String>, field: &str) -> OAuthResult<String> {
    value
        .filter(|value| !value.is_empty())
        .ok_or_else(|| OAuthError::invalid_request(format!("{field} is required")))
}

fn client_credentials(
    headers: &HeaderMap,
    body_id: Option<String>,
    body_secret: Option<String>,
) -> OAuthResult<(String, Option<String>, ClientAuthenticationMethod)> {
    let authorization_values = headers.get_all(http::header::AUTHORIZATION);
    let mut authorization_headers = authorization_values.iter();
    let authorization = authorization_headers
        .next()
        .map(|value| {
            value
                .to_str()
                .map_err(|_| OAuthError::invalid_client("malformed Authorization header"))
        })
        .transpose()?;
    if authorization_headers.next().is_some() {
        return Err(OAuthError::invalid_client(
            "multiple Authorization headers are not allowed",
        ));
    }
    if let Some(authorization) = authorization {
        let (_, encoded) = authorization
            .split_once(' ')
            .filter(|(scheme, encoded)| {
                scheme.eq_ignore_ascii_case("Basic")
                    && !encoded.is_empty()
                    && encoded.len() <= 4_096
            })
            .ok_or_else(|| {
                OAuthError::invalid_client("expected HTTP Basic client authentication")
            })?;
        if body_id.is_some() || body_secret.is_some() {
            return Err(OAuthError::invalid_request(
                "client credentials must use only one authentication method",
            ));
        }
        let decoded = STANDARD
            .decode(encoded)
            .map_err(|_| OAuthError::invalid_client("malformed HTTP Basic credentials"))?;
        let decoded = String::from_utf8(decoded)
            .map_err(|_| OAuthError::invalid_client("malformed HTTP Basic credentials"))?;
        let (client_id, secret) = decoded
            .split_once(':')
            .ok_or_else(|| OAuthError::invalid_client("malformed HTTP Basic credentials"))?;
        let decode = |value: &str| {
            url::form_urlencoded::parse(value.as_bytes())
                .next()
                .map(|(value, _)| value.into_owned())
                .unwrap_or_default()
        };
        let client_id = decode(client_id);
        let secret = decode(secret);
        validate_client_credential_lengths(&client_id, Some(&secret))?;
        return Ok((client_id, Some(secret), ClientAuthenticationMethod::Basic));
    }
    let method = if body_secret.is_some() {
        ClientAuthenticationMethod::Post
    } else {
        ClientAuthenticationMethod::None
    };
    let client_id = required(body_id, "client_id")?;
    validate_client_credential_lengths(&client_id, body_secret.as_deref())?;
    Ok((client_id, body_secret, method))
}

fn validate_client_credential_lengths(
    client_id: &str,
    client_secret: Option<&str>,
) -> OAuthResult<()> {
    if client_id.len() > 2_048 || client_secret.is_some_and(|secret| secret.len() > 256) {
        return Err(OAuthError::invalid_client(
            "OAuth client credentials exceed the allowed length",
        ));
    }
    Ok(())
}

fn validate_credential(value: &str, prefix: &str, label: &str) -> OAuthResult<()> {
    if value.len() > 256
        || !value.strip_prefix(prefix).is_some_and(|secret| {
            secret.len() >= 32
                && secret
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
    {
        return Err(OAuthError::invalid_grant(format!("{label} is malformed")));
    }
    Ok(())
}

fn verify_pkce(verifier: &str, challenge: &str) -> bool {
    pkce_challenge(verifier).as_deref() == Some(challenge)
}

fn pkce_challenge(verifier: &str) -> Option<String> {
    if !(43..=128).contains(&verifier.len())
        || !verifier
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~'))
    {
        return None;
    }
    Some(URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())))
}

#[cfg(test)]
mod tests {
    use http::HeaderValue;

    use super::*;

    #[test]
    fn pkce_s256_matches_rfc_example() {
        assert!(verify_pkce(
            "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk",
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        ));
    }

    #[test]
    fn client_credentials_accept_case_insensitive_basic_and_reject_duplicates() {
        let mut headers = HeaderMap::new();
        let encoded = STANDARD.encode("client-id:client-secret");
        headers.insert(
            http::header::AUTHORIZATION,
            HeaderValue::from_str(&format!("basic {encoded}")).unwrap(),
        );
        let (client_id, secret, method) =
            client_credentials(&headers, None, None).expect("valid Basic credentials");
        assert_eq!(client_id, "client-id");
        assert_eq!(secret.as_deref(), Some("client-secret"));
        assert_eq!(method, ClientAuthenticationMethod::Basic);

        headers.append(
            http::header::AUTHORIZATION,
            HeaderValue::from_static("Basic Y2xpZW50OnNlY3JldA=="),
        );
        assert!(client_credentials(&headers, None, None).is_err());
    }
}
