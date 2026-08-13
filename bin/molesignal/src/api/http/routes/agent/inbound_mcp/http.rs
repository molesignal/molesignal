// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{sync::Arc, time::Duration};

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use serde_json::json;
use url::Url;

use super::{
    admission::AdmissionRejection, handler::InboundMcpHandler, runtime::InboundMcpAdapterRuntime,
};
use crate::{
    agent::{
        FEATURE,
        inbound_mcp::{HARD_BODY_BYTES, InboundMcpSettings},
    },
    api::{
        AppState,
        http::{
            billing::org_blocked_cached,
            middleware::auth::{
                AuthenticatedCredential, authenticate_api_token_identity_with_metadata,
            },
        },
    },
    domain::iam::api_token::ApiTokenKind,
    shared::{Error, time::TimestampMicros, trace_context::update_current_trace_context},
};

pub(super) fn routes(state: AppState, runtime: Arc<InboundMcpAdapterRuntime>) -> Router<AppState> {
    let handler_state = state.clone();
    let handler_runtime = runtime;
    let service: StreamableHttpService<InboundMcpHandler, LocalSessionManager> =
        StreamableHttpService::new(
            move || {
                Ok(InboundMcpHandler::new(
                    handler_state.clone(),
                    handler_runtime.clone(),
                ))
            },
            LocalSessionManager::default().into(),
            StreamableHttpServerConfig::default()
                .with_legacy_session_mode(true)
                .with_json_response(true)
                .with_sse_keep_alive(Some(Duration::from_secs(10)))
                .with_max_request_body_bytes(HARD_BODY_BYTES as usize)
                .disable_allowed_hosts()
                .disable_allowed_origins(),
        );
    Router::new()
        .route_service("/mcp", service)
        .layer(middleware::from_fn_with_state(state, inbound_mcp_layer))
}

async fn inbound_mcp_layer(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let mut response = match prepare_request(&state, &mut request).await {
        Ok(guard) => {
            let response = next.run(request).await;
            drop(guard);
            response
        }
        Err(rejection) => rejection.into_response(&state, request.headers()),
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn prepare_request(
    state: &AppState,
    request: &mut Request,
) -> Result<super::admission::AdmissionGuard, McpHttpRejection> {
    validate_host(state, request.headers())?;
    let resource =
        crate::api::http::routes::oauth::metadata::resource_uri(state, request.headers())
            .map_err(McpHttpRejection::from_error)?;
    let token = bearer_token(request.headers())?;
    let (mut iam, credential) = if token.starts_with("ms_") || token.starts_with("msrum_") {
        let identity = authenticate_api_token_identity_with_metadata(
            token,
            state.iam.service.as_ref(),
            state.iam.api_tokens.clone(),
        )
        .await
        .map_err(McpHttpRejection::from_error)?;
        let credential = AuthenticatedCredential::ApiToken {
            token_id: identity.token_id,
            token_kind: identity.token_kind,
        };
        (identity.context, credential)
    } else if token.starts_with("msoauth_") {
        crate::api::http::middleware::auth::oauth::authenticate_access_token(
            state, token, &resource,
        )
        .await
        .map_err(McpHttpRejection::from_error)?
    } else {
        return Err(McpHttpRejection::unauthorized(
            "Inbound MCP accepts personal, service-account, or OAuth access tokens",
        ));
    };
    if !credential.is_inbound_mcp_credential() {
        return Err(McpHttpRejection::unauthorized(
            "this API token type cannot access Inbound MCP",
        ));
    }
    if matches!(
        &credential,
        AuthenticatedCredential::ApiToken {
            token_kind: ApiTokenKind::DefaultIntake | ApiTokenKind::RumClient,
            ..
        }
    ) {
        return Err(McpHttpRejection::unauthorized(
            "intake and RUM credentials cannot access Inbound MCP",
        ));
    }

    state
        .iam
        .service
        .ensure_organization_access(&iam.org_id)
        .await
        .map_err(McpHttpRejection::from_error)?;
    if org_blocked_cached(state, &iam.org_id, TimestampMicros::now().0)
        .await
        .map_err(McpHttpRejection::from_error)?
    {
        return Err(McpHttpRejection::from_error(Error::payment_required(
            "organization access is paused",
        )));
    }
    if !state.platform.license.has_feature(FEATURE) {
        return Err(McpHttpRejection::forbidden(
            "Inbound MCP requires the agent feature",
        ));
    }
    let capability_snapshot = state
        .iam
        .access
        .enrich_context(&mut iam)
        .await
        .map_err(McpHttpRejection::from_error)?;
    if !iam.has_permission("agent.use") {
        return Err(McpHttpRejection::forbidden(
            "missing permission `agent.use`",
        ));
    }
    let settings = state
        .agent
        .inbound_mcp
        .get_settings(&iam.org_id)
        .await
        .map_err(McpHttpRejection::from_error)?
        .unwrap_or_else(|| InboundMcpSettings::defaults(iam.org_id.clone()));
    if !settings.enabled {
        return Err(McpHttpRejection::forbidden(
            "Inbound MCP is disabled for this organization",
        ));
    }
    validate_origin(state, request.headers(), &settings)?;
    enforce_body_limit(request, settings.max_request_bytes).await?;

    let runtime = request
        .extensions()
        .get::<Arc<InboundMcpAdapterRuntime>>()
        .cloned()
        .ok_or_else(|| McpHttpRejection::internal("Inbound MCP runtime is unavailable"))?;
    let credential_key = credential.stable_key(&iam.org_id);
    let guard = runtime
        .admit(
            credential_key,
            settings.max_concurrent_calls.max(1) as u32,
            settings.calls_per_minute.max(1) as usize,
        )
        .map_err(McpHttpRejection::admission)?;

    if let Some(trace_context) = request
        .extensions_mut()
        .get_mut::<crate::shared::trace_context::TraceContext>()
    {
        trace_context.set_authenticated_org(iam.org_id.as_str());
    }
    update_current_trace_context(|trace_context| {
        trace_context.set_authenticated_org(iam.org_id.as_str());
    });
    tracing::Span::current().record("molesignal.org.id", iam.org_id.as_str());
    tracing::Span::current().record("molesignal.user.id", iam.user_id.as_str());
    request.extensions_mut().insert(capability_snapshot);
    request.extensions_mut().insert(settings);
    request.extensions_mut().insert(credential);
    request.extensions_mut().insert(iam);
    Ok(guard)
}

fn bearer_token(headers: &HeaderMap) -> Result<&str, McpHttpRejection> {
    let authorization_headers = headers.get_all(header::AUTHORIZATION);
    let mut values = authorization_headers.iter();
    let value = values.next();
    if values.next().is_some() {
        return Err(McpHttpRejection::unauthorized(
            "multiple Authorization headers are not allowed",
        ));
    }
    value
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split_once(' '))
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("Bearer"))
        .map(|(_, token)| token)
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 512
                && !value.bytes().any(|byte| byte.is_ascii_whitespace())
        })
        .ok_or_else(|| McpHttpRejection::unauthorized("missing Authorization Bearer token"))
}

async fn enforce_body_limit(
    request: &mut Request,
    configured_limit: i64,
) -> Result<(), McpHttpRejection> {
    let limit = configured_limit.clamp(1, HARD_BODY_BYTES) as usize;
    let body = std::mem::replace(request.body_mut(), Body::empty());
    let bytes = to_bytes(body, limit)
        .await
        .map_err(|_| McpHttpRejection::payload_too_large(limit))?;
    *request.body_mut() = Body::from(bytes);
    Ok(())
}

fn validate_host(state: &AppState, headers: &HeaderMap) -> Result<(), McpHttpRejection> {
    let host_headers = headers.get_all(header::HOST);
    let mut hosts = host_headers.iter();
    let host = hosts
        .next()
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| McpHttpRejection::forbidden("missing or invalid Host header"))?;
    if hosts.next().is_some() {
        return Err(McpHttpRejection::forbidden(
            "multiple Host headers are not allowed",
        ));
    }
    let configured = state.platform.external_url.trim();
    if !configured.is_empty() {
        let expected = Url::parse(configured)
            .map_err(|_| McpHttpRejection::internal("configured external URL is invalid"))?;
        if !host_matches_origin(host, &expected) {
            return Err(McpHttpRejection::forbidden(
                "Host does not match the configured external URL",
            ));
        }
        return Ok(());
    }
    let parsed = Url::parse(&format!("http://{host}"))
        .map_err(|_| McpHttpRejection::forbidden("invalid Host header"))?;
    let loopback = parsed.host_str().is_some_and(|value| {
        value.eq_ignore_ascii_case("localhost")
            || value
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if loopback {
        Ok(())
    } else {
        Err(McpHttpRejection::forbidden(
            "configure http.external_url before exposing Inbound MCP on a non-loopback host",
        ))
    }
}

fn validate_origin(
    state: &AppState,
    headers: &HeaderMap,
    settings: &InboundMcpSettings,
) -> Result<(), McpHttpRejection> {
    let origin_headers = headers.get_all(header::ORIGIN);
    let mut origins = origin_headers.iter();
    let Some(origin) = origins.next() else {
        return Ok(());
    };
    if origins.next().is_some() {
        return Err(McpHttpRejection::forbidden(
            "multiple Origin headers are not allowed",
        ));
    }
    let origin = origin
        .to_str()
        .map_err(|_| McpHttpRejection::forbidden("invalid Origin header"))?;
    let parsed =
        Url::parse(origin).map_err(|_| McpHttpRejection::forbidden("invalid Origin header"))?;
    let normalized = origin_value(&parsed)
        .ok_or_else(|| McpHttpRejection::forbidden("invalid Origin header"))?;
    let same_origin = crate::api::http::routes::oauth::metadata::external_base(state, headers)
        .ok()
        .and_then(|base| Url::parse(&base).ok())
        .and_then(|url| origin_value(&url))
        .is_some_and(|expected| expected.eq_ignore_ascii_case(&normalized));
    if same_origin
        || settings
            .allowed_origins
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(&normalized))
    {
        Ok(())
    } else {
        Err(McpHttpRejection::forbidden(
            "Origin is not allowed for this organization",
        ))
    }
}

fn host_matches_origin(host: &str, expected: &Url) -> bool {
    let Ok(presented) = Url::parse(&format!("{}://{host}", expected.scheme())) else {
        return false;
    };
    presented.username().is_empty()
        && presented.password().is_none()
        && presented.path() == "/"
        && presented.query().is_none()
        && presented.fragment().is_none()
        && presented
            .host_str()
            .zip(expected.host_str())
            .is_some_and(|(left, right)| left.eq_ignore_ascii_case(right))
        && presented.port_or_known_default() == expected.port_or_known_default()
}

fn origin_value(url: &Url) -> Option<String> {
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    Some(url.origin().ascii_serialization())
}

struct McpHttpRejection {
    status: StatusCode,
    code: &'static str,
    message: String,
    retry_after: Option<u64>,
    authenticate: bool,
}

impl McpHttpRejection {
    fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized", message, true)
    }

    fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, "forbidden", message, false)
    }

    fn internal(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            message,
            false,
        )
    }

    fn payload_too_large(limit: usize) -> Self {
        Self::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            format!("request exceeds the configured {limit} byte limit"),
            false,
        )
    }

    fn new(
        status: StatusCode,
        code: &'static str,
        message: impl Into<String>,
        authenticate: bool,
    ) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            retry_after: None,
            authenticate,
        }
    }

    fn from_error(error: Error) -> Self {
        let status = StatusCode::from_u16(error.http_status_code())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let authenticate = status == StatusCode::UNAUTHORIZED;
        let message = if status.is_server_error() {
            tracing::warn!(error = %error, "Inbound MCP request failed");
            "internal error".into()
        } else {
            error.to_string()
        };
        Self::new(status, error.http_error_code(), message, authenticate)
    }

    fn admission(rejection: AdmissionRejection) -> Self {
        let (message, retry_after) = match rejection {
            AdmissionRejection::Concurrent => ("maximum concurrent calls reached".into(), Some(1)),
            AdmissionRejection::RateLimited {
                retry_after_seconds,
            } => (
                "per-token call rate exceeded".into(),
                Some(retry_after_seconds),
            ),
        };
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            code: "resource_exhausted",
            message,
            retry_after,
            authenticate: false,
        }
    }

    fn into_response(self, state: &AppState, headers: &HeaderMap) -> Response {
        let mut response = (
            self.status,
            axum::Json(json!({ "error": self.code, "message": self.message })),
        )
            .into_response();
        if self.authenticate
            && let Ok(base) =
                crate::api::http::routes::oauth::metadata::external_base(state, headers)
        {
            let metadata = format!("{base}/.well-known/oauth-protected-resource/api/v1/mcp");
            if let Ok(value) =
                HeaderValue::from_str(&format!("Bearer resource_metadata=\"{metadata}\""))
            {
                response
                    .headers_mut()
                    .insert(header::WWW_AUTHENTICATE, value);
            }
        }
        if let Some(retry_after) = self.retry_after
            && let Ok(value) = HeaderValue::from_str(&retry_after.to_string())
        {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_default_and_explicit_origin_ports() {
        assert_eq!(
            origin_value(&Url::parse("https://example.com").unwrap()).as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            origin_value(&Url::parse("http://localhost:3000").unwrap()).as_deref(),
            Some("http://localhost:3000")
        );
        assert_eq!(
            origin_value(&Url::parse("https://[::1]").unwrap()).as_deref(),
            Some("https://[::1]")
        );
        assert!(origin_value(&Url::parse("https://example.com/path").unwrap()).is_none());
    }

    #[test]
    fn host_comparison_normalizes_default_ports_and_rejects_credentials() {
        let expected = Url::parse("https://example.com").unwrap();
        assert!(host_matches_origin("example.com", &expected));
        assert!(host_matches_origin("EXAMPLE.COM:443", &expected));
        assert!(!host_matches_origin("example.com:8443", &expected));
        assert!(!host_matches_origin("attacker@example.com", &expected));
    }
}
