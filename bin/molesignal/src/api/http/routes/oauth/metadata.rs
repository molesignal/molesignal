// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, header},
    routing::get,
};
use serde_json::{Value, json};
use url::Url;

use crate::{
    api::AppState,
    shared::{Error, Result},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/.well-known/oauth-protected-resource",
            get(protected_resource_metadata),
        )
        .route(
            "/.well-known/oauth-protected-resource/api/v1/mcp",
            get(protected_resource_metadata),
        )
        .route(
            "/.well-known/oauth-authorization-server",
            get(authorization_server_metadata),
        )
        .route("/docs/inbound-mcp", get(documentation))
}

async fn documentation() -> ([(&'static str, &'static str); 2], &'static str) {
    (
        [
            (
                header::CONTENT_TYPE.as_str(),
                "text/markdown; charset=utf-8",
            ),
            (header::CACHE_CONTROL.as_str(), "public, max-age=300"),
        ],
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/inbound-mcp.md"
        )),
    )
}

pub(crate) fn external_base(state: &AppState, headers: &HeaderMap) -> Result<String> {
    let configured = state.platform.external_url.trim().trim_end_matches('/');
    if !configured.is_empty() {
        let parsed = Url::parse(configured)
            .map_err(|error| Error::internal(format!("invalid external URL: {error}")))?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.path() != "/"
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(Error::internal(
                "configured external URL is not a safe HTTP origin",
            ));
        }
        if parsed.scheme() != "https" && !parsed.host_str().is_some_and(is_loopback_host) {
            return Err(Error::internal(
                "OAuth requires an HTTPS external URL except on loopback",
            ));
        }
        return Ok(configured.to_string());
    }
    let host_headers = headers.get_all(http::header::HOST);
    let mut hosts = host_headers.iter();
    let host = hosts
        .next()
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::forbidden("missing Host header"))?;
    if hosts.next().is_some() {
        return Err(Error::forbidden("multiple Host headers are not allowed"));
    }
    let parsed = Url::parse(&format!("http://{host}"))
        .map_err(|_| Error::forbidden("invalid Host header"))?;
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.path() != "/"
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(Error::forbidden("invalid Host header"));
    }
    let loopback = parsed.host_str().is_some_and(is_loopback_host);
    if !loopback {
        return Err(Error::forbidden(
            "configure http.external_url before exposing OAuth on a non-loopback host",
        ));
    }
    Ok(format!("http://{host}"))
}

fn is_loopback_host(value: &str) -> bool {
    value.eq_ignore_ascii_case("localhost")
        || value
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

pub(crate) fn resource_uri(state: &AppState, headers: &HeaderMap) -> Result<String> {
    Ok(format!("{}/api/v1/mcp", external_base(state, headers)?))
}

async fn protected_resource_metadata(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let base = external_base(&state, &headers)?;
    Ok(Json(json!({
        "resource": format!("{base}/api/v1/mcp"),
        "authorization_servers": [base],
        "bearer_methods_supported": ["header"],
        "scopes_supported": ["mcp", "offline_access"],
        "resource_documentation": format!("{base}/docs/inbound-mcp")
    })))
}

async fn authorization_server_metadata(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let base = external_base(&state, &headers)?;

    // Some MCP clients still discard the optional RFC 9207 `iss` parameter
    // before handing the callback to their OAuth library. Advertising issuer
    // response validation as mandatory makes those clients reject an otherwise
    // valid response. MoleSignal still returns `iss` from the authorization
    // endpoint so clients that preserve it can validate the issuer.
    Ok(Json(json!({
        "issuer": base,
        "authorization_endpoint": format!("{base}/oauth/authorize"),
        "token_endpoint": format!("{base}/api/v1/oauth/token"),
        "registration_endpoint": format!("{base}/api/v1/oauth/register"),
        "revocation_endpoint": format!("{base}/api/v1/oauth/revoke"),
        "service_documentation": format!("{base}/docs/inbound-mcp"),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "code_challenge_methods_supported": ["S256"],
        "resource_indicators_supported": true,
        "token_endpoint_auth_methods_supported": [
            "none", "client_secret_basic", "client_secret_post"
        ],
        "revocation_endpoint_auth_methods_supported": [
            "none", "client_secret_basic", "client_secret_post"
        ],
        "scopes_supported": ["mcp", "offline_access"],
        "client_id_metadata_document_supported": true
    })))
}
