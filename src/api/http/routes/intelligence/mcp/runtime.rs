// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};

use futures::StreamExt;
use ipnet::IpNet;
use reqwest::{Client, header::HeaderValue, redirect::Policy};
use serde_json::{Value, json};
use url::Url;

use super::{DEFAULT_MAX_RESPONSE_BYTES, DEFAULT_TIMEOUT_MS, MCP_PROTOCOL_VERSION};
use crate::{
    intelligence::tool_control::{McpServer, McpServerRuntime},
    shared::{Error, Result, ids::Id},
};

pub(super) async fn rpc_request(
    runtime: &McpServerRuntime,
    method: &str,
    params: Value,
) -> Result<Value> {
    if runtime.server.transport != "streamable_http" {
        return Err(Error::invalid(format!(
            "MCP transport `{}` is configured but this runtime only permits Streamable HTTP",
            runtime.server.transport
        )));
    }
    if !runtime.server.enabled {
        return Err(Error::forbidden("MCP Server is disabled"));
    }
    let (resolved_host, resolved_addresses) = validate_network_target(&runtime.server).await?;
    let client = Client::builder()
        .redirect(Policy::none())
        .danger_accept_invalid_certs(!runtime.server.tls_verify)
        .resolve_to_addrs(&resolved_host, &resolved_addresses)
        .timeout(Duration::from_millis(
            u64::try_from(runtime.server.timeout_ms).unwrap_or(DEFAULT_TIMEOUT_MS as u64),
        ))
        .build()
        .map_err(|error| Error::internal(format!("MCP HTTP client: {error}")))?;
    let initialize = json!({
        "jsonrpc": "2.0",
        "id": Id::new().0,
        "method": "initialize",
        "params": {
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {
                "name": "Mole Intelligence",
                "version": env!("CARGO_PKG_VERSION")
            }
        }
    });
    let (_, session_id) = send_rpc(&client, runtime, initialize, None, true).await?;
    let initialized = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized",
        "params": {}
    });
    let _ = send_rpc(&client, runtime, initialized, session_id.as_deref(), false).await?;
    let request = json!({
        "jsonrpc": "2.0",
        "id": Id::new().0,
        "method": method,
        "params": params
    });
    let (response, _) = send_rpc(&client, runtime, request, session_id.as_deref(), true).await?;
    if let Some(error) = response.get("error") {
        return Err(Error::invalid(format!(
            "MCP `{method}` failed: {}",
            error["message"].as_str().unwrap_or("remote error")
        )));
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| Error::invalid(format!("MCP `{method}` response has no result")))
}

async fn send_rpc(
    client: &Client,
    runtime: &McpServerRuntime,
    body: Value,
    session_id: Option<&str>,
    expect_body: bool,
) -> Result<(Value, Option<String>)> {
    let endpoint = runtime
        .server
        .endpoint_url
        .as_deref()
        .ok_or_else(|| Error::invalid("MCP Endpoint URL is missing"))?;
    let mut request = client
        .post(endpoint)
        .header("accept", "application/json, text/event-stream")
        .header("content-type", "application/json")
        .header("mcp-protocol-version", MCP_PROTOCOL_VERSION)
        .json(&body);
    if let Some(session_id) = session_id {
        request = request.header("mcp-session-id", session_id);
    }
    request = match runtime.server.auth_type.as_str() {
        "none" => request,
        "bearer_token" | "oauth" => {
            let credential = runtime
                .credential
                .as_deref()
                .ok_or_else(|| Error::forbidden("MCP credential is not configured"))?;
            request.bearer_auth(credential)
        }
        "api_key" | "internal_service_identity" => {
            let credential = runtime
                .credential
                .as_deref()
                .ok_or_else(|| Error::forbidden("MCP credential is not configured"))?;
            let header = runtime.server.auth_header.as_deref().unwrap_or(
                if runtime.server.auth_type == "api_key" {
                    "x-api-key"
                } else {
                    "x-internal-service-identity"
                },
            );
            let value = HeaderValue::from_str(credential)
                .map_err(|_| Error::invalid("MCP credential contains invalid header bytes"))?;
            request.header(header, value)
        }
        "mtls" => {
            return Err(Error::invalid(
                "mTLS MCP requires a server-managed identity and is not available in this runtime",
            ));
        }
        other => {
            return Err(Error::invalid(format!(
                "unsupported MCP authentication type `{other}`"
            )));
        }
    };
    let response = crate::shared::http_trace::send(
        client,
        request,
        crate::shared::http_trace::HttpTarget::ThirdParty,
    )
    .await
    .map_err(|error| Error::unavailable(format!("MCP request failed: {error}")))?;
    let status = response.status();
    let session_id = response
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let bytes = bounded_response_bytes(response, runtime.server.max_response_bytes).await?;
    if !status.is_success() {
        return Err(Error::unavailable(format!(
            "MCP Server returned HTTP {}",
            status.as_u16()
        )));
    }
    if bytes.is_empty() {
        if expect_body {
            return Err(Error::invalid("MCP Server returned an empty response"));
        }
        return Ok((Value::Null, session_id));
    }
    let value = if content_type.contains("text/event-stream") {
        parse_sse_json(&bytes)?
    } else {
        serde_json::from_slice(&bytes)
            .map_err(|error| Error::invalid(format!("invalid MCP JSON response: {error}")))?
    };
    Ok((value, session_id))
}

async fn bounded_response_bytes(response: reqwest::Response, maximum: i64) -> Result<Vec<u8>> {
    let maximum = usize::try_from(maximum).unwrap_or(DEFAULT_MAX_RESPONSE_BYTES as usize);
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|error| Error::unavailable(format!("MCP response read: {error}")))?;
        if bytes.len().saturating_add(chunk.len()) > maximum {
            return Err(Error::payload_too_large(format!(
                "MCP response exceeds {maximum} bytes"
            )));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn parse_sse_json(bytes: &[u8]) -> Result<Value> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| Error::invalid("MCP SSE response is not UTF-8"))?;
    let data = text
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(str::trim)
        .find(|value| !value.is_empty() && *value != "[DONE]")
        .ok_or_else(|| Error::invalid("MCP SSE response contains no data event"))?;
    serde_json::from_str(data)
        .map_err(|error| Error::invalid(format!("invalid MCP SSE JSON: {error}")))
}

pub(super) fn validate_endpoint_shape(endpoint: &str) -> Result<Url> {
    let url = Url::parse(endpoint)
        .map_err(|error| Error::invalid(format!("invalid MCP Endpoint URL: {error}")))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(Error::invalid("MCP Endpoint URL must use http or https"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Error::invalid(
            "MCP Endpoint URL must not contain inline credentials",
        ));
    }
    if url.host_str().is_none() {
        return Err(Error::invalid("MCP Endpoint URL must include a host"));
    }
    Ok(url)
}

async fn validate_network_target(server: &McpServer) -> Result<(String, Vec<SocketAddr>)> {
    let url = validate_endpoint_shape(
        server
            .endpoint_url
            .as_deref()
            .ok_or_else(|| Error::invalid("MCP Endpoint URL is missing"))?,
    )?;
    let host = url
        .host_str()
        .ok_or_else(|| Error::invalid("MCP Endpoint URL has no host"))?
        .to_ascii_lowercase();
    if !server.allowed_domains.is_empty()
        && !server
            .allowed_domains
            .iter()
            .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
    {
        return Err(Error::forbidden(format!(
            "MCP host `{host}` is not in the allowed domain list"
        )));
    }
    let port = url
        .port_or_known_default()
        .ok_or_else(|| Error::invalid("MCP Endpoint URL has no port"))?;
    let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map_err(|error| Error::unavailable(format!("MCP DNS resolution failed: {error}")))?
        .collect();
    if addresses.is_empty() {
        return Err(Error::unavailable(
            "MCP DNS resolution returned no addresses",
        ));
    }
    let allowed_networks = server
        .allowed_cidrs
        .iter()
        .map(|value| {
            value
                .parse::<IpNet>()
                .map_err(|_| Error::invalid(format!("invalid stored MCP CIDR `{value}`")))
        })
        .collect::<Result<Vec<_>>>()?;
    for address in &addresses {
        let ip = address.ip();
        if server.private_only && !is_private_ip(ip) {
            return Err(Error::forbidden(format!(
                "MCP target `{ip}` is outside the private network"
            )));
        }
        if !allowed_networks.is_empty()
            && !allowed_networks.iter().any(|network| network.contains(&ip))
        {
            return Err(Error::forbidden(format!(
                "MCP target `{ip}` is outside the allowed CIDR list"
            )));
        }
    }
    Ok((host, addresses))
}

fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_private() || ip.is_loopback() || ip.is_link_local(),
        IpAddr::V6(ip) => ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unspecified_addresses_are_not_private_targets() {
        assert!(!is_private_ip("0.0.0.0".parse().unwrap()));
        assert!(!is_private_ip("::".parse().unwrap()));
        assert!(is_private_ip("10.0.0.5".parse().unwrap()));
        assert!(is_private_ip("fd00::5".parse().unwrap()));
    }

    #[test]
    fn endpoint_shape_rejects_credentials_and_non_http_schemes() {
        assert!(validate_endpoint_shape("https://mcp.example/tools").is_ok());
        assert!(validate_endpoint_shape("file:///tmp/mcp.sock").is_err());
        assert!(validate_endpoint_shape("https://token@mcp.example/tools").is_err());
    }
}
