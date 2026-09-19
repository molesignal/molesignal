// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};

use futures::StreamExt;
use reqwest::redirect::Policy;
use serde::Deserialize;
use url::Url;

use crate::{
    agent::inbound_mcp::InboundMcpOAuthClient,
    api::AppState,
    app::iam::verify_password,
    shared::{Error, Result},
};

const MAX_CLIENT_METADATA_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub(super) struct ResolvedOAuthClient {
    pub client_id: String,
    pub client_name: String,
    pub redirect_uris: Vec<String>,
    pub grant_types: Vec<String>,
    pub token_endpoint_auth_method: String,
    pub scope: String,
    pub client_secret_hash: Option<String>,
    pub client_secret_expires_at: Option<i64>,
    pub client_uri: Option<String>,
    pub is_metadata_document: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ClientAuthenticationMethod {
    None,
    Basic,
    Post,
}

impl From<InboundMcpOAuthClient> for ResolvedOAuthClient {
    fn from(client: InboundMcpOAuthClient) -> Self {
        Self {
            client_id: client.client_id,
            client_name: client.client_name,
            redirect_uris: client.redirect_uris,
            grant_types: client.grant_types,
            token_endpoint_auth_method: client.token_endpoint_auth_method,
            scope: client.scope,
            client_secret_hash: client.client_secret_hash,
            client_secret_expires_at: client.client_secret_expires_at,
            client_uri: client.client_uri,
            is_metadata_document: false,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ClientMetadataDocument {
    client_id: String,
    client_name: String,
    redirect_uris: Vec<String>,
    #[serde(default = "authorization_code_grants")]
    grant_types: Vec<String>,
    #[serde(default = "code_response_types")]
    response_types: Vec<String>,
    #[serde(default = "none_auth_method")]
    token_endpoint_auth_method: String,
    #[serde(default = "mcp_scope")]
    scope: String,
    #[serde(default)]
    client_uri: Option<String>,
}

fn authorization_code_grants() -> Vec<String> {
    vec!["authorization_code".into(), "refresh_token".into()]
}

fn code_response_types() -> Vec<String> {
    vec!["code".into()]
}

fn none_auth_method() -> String {
    "none".into()
}

fn mcp_scope() -> String {
    "mcp".into()
}

pub(super) async fn resolve_client(
    state: &AppState,
    client_id: &str,
) -> Result<ResolvedOAuthClient> {
    if client_id.is_empty() || client_id.len() > 2_048 {
        return Err(Error::invalid(
            "OAuth client_id must contain 1 to 2048 bytes",
        ));
    }
    if client_id.starts_with("https://") {
        return fetch_client_metadata(client_id).await;
    }
    state
        .agent
        .inbound_mcp
        .get_oauth_client(client_id)
        .await?
        .map(Into::into)
        .ok_or_else(|| Error::unauthorized("OAuth client is not registered"))
}

pub(super) fn authenticate_client(
    client: &ResolvedOAuthClient,
    method: ClientAuthenticationMethod,
    presented_secret: Option<&str>,
) -> Result<()> {
    if client
        .client_secret_expires_at
        .is_some_and(|expires_at| expires_at > 0 && expires_at <= chrono::Utc::now().timestamp())
    {
        return Err(Error::unauthorized("OAuth client secret has expired"));
    }
    match client.token_endpoint_auth_method.as_str() {
        "none" if method == ClientAuthenticationMethod::None && presented_secret.is_none() => {
            Ok(())
        }
        "none" => Err(Error::unauthorized(
            "public OAuth clients must not send a client secret",
        )),
        "client_secret_basic" if method == ClientAuthenticationMethod::Basic => {
            verify_client_secret(client, presented_secret)
        }
        "client_secret_post" if method == ClientAuthenticationMethod::Post => {
            verify_client_secret(client, presented_secret)
        }
        "client_secret_basic" | "client_secret_post" => Err(Error::unauthorized(
            "OAuth client used a token endpoint authentication method different from its registration",
        )),
        _ => Err(Error::unauthorized(
            "unsupported OAuth client authentication method",
        )),
    }
}

fn verify_client_secret(
    client: &ResolvedOAuthClient,
    presented_secret: Option<&str>,
) -> Result<()> {
    let secret =
        presented_secret.ok_or_else(|| Error::unauthorized("OAuth client secret is required"))?;
    let hash = client
        .client_secret_hash
        .as_deref()
        .ok_or_else(|| Error::unauthorized("OAuth client has no secret"))?;
    verify_password(secret, hash)
        .map_err(|_| Error::unauthorized("OAuth client authentication failed"))
}

pub(super) fn normalize_scope(scope: &str) -> std::result::Result<String, String> {
    if scope.len() > 256 {
        return Err("OAuth scope exceeds 256 bytes".into());
    }
    let values = scope.split_ascii_whitespace().collect::<Vec<_>>();
    if !values.contains(&"mcp")
        || values
            .iter()
            .any(|value| !matches!(*value, "mcp" | "offline_access"))
    {
        return Err("scope must include mcp and may additionally include offline_access".into());
    }
    Ok(if values.contains(&"offline_access") {
        "mcp offline_access".into()
    } else {
        "mcp".into()
    })
}

pub(super) fn scope_allowed(requested: &str, registered: &str) -> bool {
    requested.split_ascii_whitespace().all(|scope| {
        registered
            .split_ascii_whitespace()
            .any(|item| item == scope)
    })
}

pub(super) fn validate_redirect_uris(redirect_uris: &[String]) -> std::result::Result<(), String> {
    if redirect_uris.is_empty() || redirect_uris.len() > 20 {
        return Err("redirect_uris must contain between 1 and 20 entries".into());
    }
    for redirect_uri in redirect_uris {
        if redirect_uri.len() > 2_048 {
            return Err("redirect URI exceeds 2048 characters".into());
        }
        let url = Url::parse(redirect_uri)
            .map_err(|error| format!("invalid redirect URI `{redirect_uri}`: {error}"))?;
        if url.fragment().is_some() || !url.username().is_empty() || url.password().is_some() {
            return Err("redirect URIs must not include fragments or credentials".into());
        }
        match url.scheme() {
            "https" if url.host_str().is_some() => {}
            "http" if is_loopback_host(url.host_str().unwrap_or_default()) => {}
            "http" | "https" => {
                return Err("HTTP redirect URIs are allowed only for loopback clients".into());
            }
            "file" | "javascript" | "data" => {
                return Err("unsafe redirect URI scheme".into());
            }
            _ => {}
        }
    }
    Ok(())
}

pub(super) fn validate_client_uri(client_uri: Option<&str>) -> std::result::Result<(), String> {
    let Some(client_uri) = client_uri else {
        return Ok(());
    };
    if client_uri.len() > 2_048 {
        return Err("client_uri exceeds 2048 characters".into());
    }
    let url = Url::parse(client_uri).map_err(|error| format!("invalid client_uri: {error}"))?;
    if url.fragment().is_some() || !url.username().is_empty() || url.password().is_some() {
        return Err("client_uri must not include a fragment or credentials".into());
    }
    match url.scheme() {
        "https" if url.host_str().is_some() => Ok(()),
        "http" if is_loopback_host(url.host_str().unwrap_or_default()) => Ok(()),
        _ => Err("client_uri must use HTTPS, except for loopback clients".into()),
    }
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

async fn fetch_client_metadata(client_id: &str) -> Result<ResolvedOAuthClient> {
    if client_id.len() > 2_048 {
        return Err(Error::invalid("OAuth client metadata URL is too long"));
    }
    let url = Url::parse(client_id)
        .map_err(|error| Error::invalid(format!("invalid client metadata URL: {error}")))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::invalid(
            "client metadata documents require a credential-free HTTPS URL",
        ));
    }
    let host = url.host_str().expect("checked host").to_ascii_lowercase();
    let port = url.port_or_known_default().unwrap_or(443);
    let addresses = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map_err(|error| Error::unavailable(format!("client metadata DNS lookup: {error}")))?
        .collect::<Vec<SocketAddr>>();
    if addresses.is_empty() || addresses.iter().any(|address| !is_public_ip(address.ip())) {
        return Err(Error::forbidden(
            "client metadata documents may resolve only to public addresses",
        ));
    }
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(Policy::none())
        .timeout(Duration::from_secs(5))
        .resolve_to_addrs(&host, &addresses)
        .build()
        .map_err(|error| Error::internal(format!("client metadata HTTP client: {error}")))?;
    let response = crate::shared::http_trace::send(
        &client,
        client
            .get(url)
            .header(http::header::ACCEPT, "application/json"),
        crate::shared::http_trace::HttpTarget::ThirdParty,
    )
    .await
    .map_err(|error| Error::unavailable(format!("client metadata request: {error}")))?;
    if !response.status().is_success() {
        return Err(Error::unauthorized(format!(
            "client metadata endpoint returned HTTP {}",
            response.status().as_u16()
        )));
    }
    let content_type_is_json = response
        .headers()
        .get(http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(';')
                .next()
                .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/json"))
        });
    if !content_type_is_json {
        return Err(Error::invalid(
            "client metadata endpoint must return application/json",
        ));
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|error| Error::unavailable(format!("client metadata body: {error}")))?;
        if body.len().saturating_add(chunk.len()) > MAX_CLIENT_METADATA_BYTES {
            return Err(Error::payload_too_large(
                "client metadata document exceeds 64 KiB",
            ));
        }
        body.extend_from_slice(&chunk);
    }
    let document: ClientMetadataDocument = serde_json::from_slice(&body)
        .map_err(|error| Error::invalid(format!("invalid client metadata document: {error}")))?;
    if document.client_id != client_id || document.token_endpoint_auth_method != "none" {
        return Err(Error::unauthorized(
            "client metadata must bind the exact client_id and use token auth method none",
        ));
    }
    if document.client_name.trim().is_empty() || document.client_name.chars().count() > 255 {
        return Err(Error::invalid(
            "client metadata client_name must contain 1 to 255 characters",
        ));
    }
    validate_redirect_uris(&document.redirect_uris).map_err(Error::invalid)?;
    validate_client_uri(document.client_uri.as_deref()).map_err(Error::invalid)?;
    if !document
        .grant_types
        .iter()
        .any(|value| value == "authorization_code")
        || document
            .grant_types
            .iter()
            .any(|value| !matches!(value.as_str(), "authorization_code" | "refresh_token"))
    {
        return Err(Error::invalid(
            "client metadata may support only authorization_code and refresh_token",
        ));
    }
    if document.response_types.len() != 1
        || document
            .response_types
            .first()
            .is_none_or(|value| value != "code")
    {
        return Err(Error::invalid(
            "client metadata must use response_type code",
        ));
    }
    let scope = normalize_scope(&document.scope).map_err(Error::invalid)?;
    Ok(ResolvedOAuthClient {
        client_id: document.client_id,
        client_name: document.client_name,
        redirect_uris: document.redirect_uris,
        grant_types: document.grant_types,
        token_endpoint_auth_method: document.token_endpoint_auth_method,
        scope,
        client_secret_hash: None,
        client_secret_expires_at: None,
        client_uri: document.client_uri,
        is_metadata_document: true,
    })
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let octets = ip.octets();
            !ip.is_private()
                && !ip.is_loopback()
                && !ip.is_link_local()
                && !ip.is_unspecified()
                && !ip.is_broadcast()
                && !ip.is_multicast()
                && octets[0] != 0
                && !(octets[0] == 100 && (64..=127).contains(&octets[1]))
                && !(octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
                && !(octets[0] == 192 && octets[1] == 0 && octets[2] == 2)
                && !(octets[0] == 198 && (octets[1] == 18 || octets[1] == 19))
                && !(octets[0] == 198 && octets[1] == 51 && octets[2] == 100)
                && !(octets[0] == 203 && octets[1] == 0 && octets[2] == 113)
                && !(octets[0] == 192 && octets[1] == 88 && octets[2] == 99)
                && octets[0] < 240
        }
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            let mapped = ip.to_ipv4_mapped();
            !ip.is_loopback()
                && !ip.is_unique_local()
                && !ip.is_unicast_link_local()
                && !ip.is_unspecified()
                && !ip.is_multicast()
                && (segments[0] & 0xffc0 != 0xfec0)
                && !(segments[0] == 0x0100 && segments[1..4].iter().all(|segment| *segment == 0))
                && !(segments[0] == 0x2001 && segments[1] == 0x0db8)
                && mapped.is_none_or(|address| is_public_ip(IpAddr::V4(address)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_validation_allows_https_and_loopback() {
        assert!(validate_redirect_uris(&["https://client.example/callback".into()]).is_ok());
        assert!(validate_redirect_uris(&["http://127.0.0.1:4321/callback".into()]).is_ok());
        assert!(validate_redirect_uris(&["http://client.example/callback".into()]).is_err());
    }

    #[test]
    fn private_metadata_targets_are_rejected() {
        assert!(!is_public_ip("127.0.0.1".parse().unwrap()));
        assert!(!is_public_ip("10.0.0.8".parse().unwrap()));
        assert!(!is_public_ip("0.1.2.3".parse().unwrap()));
        assert!(!is_public_ip("192.88.99.1".parse().unwrap()));
        assert!(!is_public_ip("2001:db8::1".parse().unwrap()));
        assert!(is_public_ip("8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn client_authentication_method_must_match_registration() {
        let public = ResolvedOAuthClient {
            client_id: "public-client".into(),
            client_name: "Public client".into(),
            redirect_uris: vec!["https://client.example/callback".into()],
            grant_types: vec!["authorization_code".into()],
            token_endpoint_auth_method: "none".into(),
            scope: "mcp".into(),
            client_secret_hash: None,
            client_secret_expires_at: None,
            client_uri: None,
            is_metadata_document: false,
        };
        assert!(authenticate_client(&public, ClientAuthenticationMethod::None, None).is_ok());
        assert!(
            authenticate_client(&public, ClientAuthenticationMethod::Basic, Some("secret"))
                .is_err()
        );

        let mut confidential = public;
        confidential.token_endpoint_auth_method = "client_secret_post".into();
        assert!(
            authenticate_client(
                &confidential,
                ClientAuthenticationMethod::Basic,
                Some("secret"),
            )
            .is_err()
        );
    }

    #[test]
    fn oauth_scope_is_canonical_and_bounded() {
        assert_eq!(
            normalize_scope("offline_access mcp mcp").unwrap(),
            "mcp offline_access"
        );
        assert!(normalize_scope("openid mcp").is_err());
        assert!(normalize_scope(&"m".repeat(257)).is_err());
    }
}
