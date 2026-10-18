// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 连接一台 MCP 服务器：Streamable HTTP，或旧版 HTTP + SSE（[`super::sse`]）。
//!
//! 每次连接前按 [`TargetPolicy`] 解析并校验地址，把连接钉在校验过的地址上；不跟随重定向。
//! 凭据与请求头只进请求，不进日志与错误信息；远端以 401 / 403 拒绝时报
//! [`Error::Unauthorized`]，调用方据此请成员更新自己的凭据，不要把它原样交给 HTTP 层。
//! stdio 服务器由服务端按实例允许的命令起，见 [`super::stdio`]。

use std::{net::SocketAddr, time::Duration};

use futures::StreamExt;
use reqwest::{
    Client, StatusCode,
    header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue},
    redirect::Policy,
};
use serde_json::{Value, json};
use url::Url;

use super::{sse::SseSession, stdio::StdioSession};
use crate::{
    agent::tool_control::{McpServer, McpServerRuntime},
    app::egress::{TargetPolicy, resolve_target},
    shared::{Error, Result, ids::Id},
};

pub(super) const MCP_PROTOCOL_VERSION: &str = "2025-03-26";
pub(super) const DEFAULT_TIMEOUT_MS: i64 = 10_000;
pub(super) const DEFAULT_MAX_RESPONSE_BYTES: i64 = 1_048_576;

/// 连上之后的一次会话，可以连着发几个请求。
pub(super) enum Session {
    Http(HttpSession),
    Sse(SseSession),
    Stdio(StdioSession),
}

impl Session {
    /// 连接并完成 `initialize`，返回会话与服务器的 `initialize` 结果。`stdio_commands` 是实例
    /// 允许 stdio 服务器起的程序。
    pub(super) async fn open(
        runtime: &McpServerRuntime,
        policy: &TargetPolicy,
        stdio_commands: &[String],
    ) -> Result<(Self, Value)> {
        if !runtime.server.enabled {
            return Err(Error::forbidden("MCP Server is disabled"));
        }

        match runtime.server.transport.as_str() {
            "streamable_http" => {
                let (session, initialized) = HttpSession::open(runtime, policy).await?;
                Ok((Self::Http(session), initialized))
            }
            "sse" => {
                let (session, initialized) = SseSession::open(runtime, policy).await?;
                Ok((Self::Sse(session), initialized))
            }
            "stdio" => {
                let (session, initialized) = StdioSession::open(runtime, stdio_commands).await?;
                Ok((Self::Stdio(session), initialized))
            }
            other => Err(Error::invalid(format!(
                "MCP transport `{other}` has no runtime in Mole Agent"
            ))),
        }
    }

    pub(super) async fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        match self {
            Self::Http(session) => session.request(method, params).await,
            Self::Sse(session) => session.request(method, params).await,
            Self::Stdio(session) => session.request(method, params).await,
        }
    }
}

pub(super) fn initialize_params() -> Value {
    json!({
        "protocolVersion": MCP_PROTOCOL_VERSION,
        "capabilities": {},
        "clientInfo": {
            "name": "Mole Agent",
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}

/// JSON-RPC 应答 → 结果。
pub(super) fn rpc_result(method: &str, response: Value) -> Result<Value> {
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

/// 远端的 HTTP 状态 → 错误；401 / 403 说明凭据不对。
pub(super) fn status_error(status: StatusCode) -> Error {
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        Error::unauthorized(format!(
            "MCP Server rejected the credentials (HTTP {})",
            status.as_u16()
        ))
    } else {
        Error::unavailable(format!("MCP Server returned HTTP {}", status.as_u16()))
    }
}

pub(super) fn timeout_of(server: &McpServer) -> Duration {
    Duration::from_millis(u64::try_from(server.timeout_ms).unwrap_or(DEFAULT_TIMEOUT_MS as u64))
}

pub(super) fn max_bytes_of(server: &McpServer) -> usize {
    usize::try_from(server.max_response_bytes).unwrap_or(DEFAULT_MAX_RESPONSE_BYTES as usize)
}

/// 一台服务器的客户端：不跟随重定向，连接钉在校验过的地址上。长连接的事件流不设总超时。
pub(super) fn client_for(
    server: &McpServer,
    host: &str,
    addresses: &[SocketAddr],
    streaming: bool,
) -> Result<Client> {
    let mut builder = Client::builder()
        .redirect(Policy::none())
        .danger_accept_invalid_certs(!server.tls_verify)
        .resolve_to_addrs(host, addresses)
        .connect_timeout(timeout_of(server));

    if !streaming {
        builder = builder.timeout(timeout_of(server));
    }

    builder
        .build()
        .map_err(|error| Error::internal(format!("MCP HTTP client: {error}")))
}

fn header_value(value: &str, sensitive: bool) -> Result<HeaderValue> {
    let mut value = HeaderValue::from_str(value)
        .map_err(|_| Error::invalid("an MCP header value contains invalid bytes"))?;

    value.set_sensitive(sensitive);
    Ok(value)
}

/// 每个请求都带的请求头：配置的请求头，加上按认证方式放的凭据。
pub(super) fn request_headers(runtime: &McpServerRuntime) -> Result<HeaderMap> {
    let server = &runtime.server;
    let mut headers = HeaderMap::new();

    for (name, value) in &runtime.headers {
        let secret = server
            .headers
            .iter()
            .any(|header| header.name == *name && header.secret);

        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| Error::invalid("an MCP header name is invalid"))?;

        headers.insert(name, header_value(value, secret)?);
    }

    let credential = || {
        runtime
            .credential
            .as_deref()
            .ok_or_else(|| Error::forbidden("MCP credential is not configured"))
    };

    match server.auth_type.as_str() {
        "none" => {}
        "bearer_token" | "oauth" => {
            headers.insert(
                AUTHORIZATION,
                header_value(&format!("Bearer {}", credential()?), true)?,
            );
        }
        "api_key" | "internal_service_identity" => {
            let name = server
                .auth_header
                .as_deref()
                .unwrap_or(if server.auth_type == "api_key" {
                    "x-api-key"
                } else {
                    "x-internal-service-identity"
                });

            let name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| Error::invalid("invalid MCP authentication header"))?;

            headers.insert(name, header_value(credential()?, true)?);
        }
        "member_secret" => {
            let spec = server
                .member_credential
                .as_ref()
                .ok_or_else(|| Error::invalid("MCP member credential is not described"))?;

            let value = runtime
                .member_credential
                .as_deref()
                .ok_or_else(|| Error::unauthorized("your own MCP credential is not saved"))?;

            let value = if spec.bearer {
                format!("Bearer {value}")
            } else {
                value.to_string()
            };

            let name = HeaderName::from_bytes(spec.header.as_bytes())
                .map_err(|_| Error::invalid("invalid MCP member credential header"))?;

            headers.insert(name, header_value(&value, true)?);
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
    }

    Ok(headers)
}

pub(super) async fn bounded_response_bytes(
    response: reqwest::Response,
    maximum: usize,
) -> Result<Vec<u8>> {
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

/// Streamable HTTP 的事件流应答里找出这个请求的应答（前面可能有通知）。
fn sse_response(bytes: &[u8], id: Option<&str>) -> Result<Value> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| Error::invalid("MCP SSE response is not UTF-8"))?;

    let mut first = None;

    for data in text.split("\n\n").flat_map(|event| {
        let data = event
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(str::trim)
            .collect::<Vec<_>>()
            .join("\n");

        (!data.is_empty() && data != "[DONE]").then_some(data)
    }) {
        let value: Value = serde_json::from_str(&data)
            .map_err(|error| Error::invalid(format!("invalid MCP SSE JSON: {error}")))?;

        let answers = value.get("result").is_some() || value.get("error").is_some();

        if answers && id.is_none_or(|id| value["id"].as_str() == Some(id)) {
            return Ok(value);
        }

        first.get_or_insert(value);
    }

    first.ok_or_else(|| Error::invalid("MCP SSE response contains no data event"))
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

/// 地址校验过、解析好的连接目标。
pub(super) async fn target(
    server: &McpServer,
    policy: &TargetPolicy,
) -> Result<(Url, String, Vec<SocketAddr>)> {
    let url = validate_endpoint_shape(
        server
            .endpoint_url
            .as_deref()
            .ok_or_else(|| Error::invalid("MCP Endpoint URL is missing"))?,
    )?;

    let (host, addresses) = resolve_target(&url, policy).await?;
    Ok((url, host, addresses))
}

pub(super) struct HttpSession {
    client: Client,
    endpoint: Url,
    headers: HeaderMap,
    session_id: Option<String>,
    max_bytes: usize,
}

impl HttpSession {
    async fn open(runtime: &McpServerRuntime, policy: &TargetPolicy) -> Result<(Self, Value)> {
        let (endpoint, host, addresses) = target(&runtime.server, policy).await?;

        let mut session = Self {
            client: client_for(&runtime.server, &host, &addresses, false)?,
            endpoint,
            headers: request_headers(runtime)?,
            session_id: None,
            max_bytes: max_bytes_of(&runtime.server),
        };

        let initialized = session.request("initialize", initialize_params()).await?;

        session
            .send(
                json!({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}}),
                None,
            )
            .await?;

        Ok((session, initialized))
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = Id::new().0;

        let response = self
            .send(
                json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
                Some(&id),
            )
            .await?;

        rpc_result(method, response)
    }

    /// 发一条消息；`id` 为空是通知，不等应答。
    async fn send(&mut self, body: Value, id: Option<&str>) -> Result<Value> {
        let mut request = self
            .client
            .post(self.endpoint.clone())
            .headers(self.headers.clone())
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .header("mcp-protocol-version", MCP_PROTOCOL_VERSION)
            .json(&body);

        if let Some(session_id) = &self.session_id {
            request = request.header("mcp-session-id", session_id);
        }

        let response = crate::shared::http_trace::send(
            &self.client,
            request,
            crate::shared::http_trace::HttpTarget::ThirdParty,
        )
        .await
        .map_err(|error| Error::unavailable(format!("MCP request failed: {error}")))?;

        let status = response.status();

        if let Some(session_id) = response
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
        {
            self.session_id.get_or_insert_with(|| session_id.to_owned());
        }

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_ascii_lowercase();

        let bytes = bounded_response_bytes(response, self.max_bytes).await?;

        if !status.is_success() {
            return Err(status_error(status));
        }

        if id.is_none() {
            return Ok(Value::Null);
        }

        if bytes.is_empty() {
            return Err(Error::invalid("MCP Server returned an empty response"));
        }

        if content_type.contains("text/event-stream") {
            sse_response(&bytes, id)
        } else {
            serde_json::from_slice(&bytes)
                .map_err(|error| Error::invalid(format!("invalid MCP JSON response: {error}")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_shape_rejects_credentials_and_non_http_schemes() {
        assert!(validate_endpoint_shape("https://mcp.example/tools").is_ok());
        assert!(validate_endpoint_shape("file:///tmp/mcp.sock").is_err());
        assert!(validate_endpoint_shape("https://token@mcp.example/tools").is_err());
    }

    #[test]
    fn the_answer_is_picked_out_of_an_event_stream() {
        let body = concat!(
            "event: message\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\n\n",
            "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":\"a\",\"result\":{\"tools\":[]}}\n\n"
        );

        let value = sse_response(body.as_bytes(), Some("a")).unwrap();
        assert_eq!(value["result"]["tools"], json!([]));
        assert!(sse_response(b"event: ping\n\n", Some("a")).is_err());
    }

    #[test]
    fn rejected_credentials_are_told_apart() {
        assert!(matches!(
            status_error(StatusCode::UNAUTHORIZED),
            Error::Unauthorized(_)
        ));
        assert!(matches!(
            status_error(StatusCode::BAD_GATEWAY),
            Error::Unavailable(_)
        ));
    }
}
