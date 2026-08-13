// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;

use super::runtime::InboundMcpAdapterRuntime;
use crate::{
    agent::{
        FEATURE,
        inbound_mcp::{HARD_BODY_BYTES, InboundMcpSettings},
    },
    api::AppState,
    app::iam::IamContext,
    domain::iam::permission,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/agent/settings/inbound-mcp",
            get(get_settings).put(update_settings),
        )
        .route(
            "/agent/settings/inbound-mcp/oauth-connections",
            get(list_oauth_connections),
        )
        .route(
            "/agent/settings/inbound-mcp/oauth-connections/{family_id}/revoke",
            post(revoke_oauth_connection),
        )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateSettingsRequest {
    enabled: bool,
    #[serde(default)]
    allowed_origins: Vec<String>,
    max_request_bytes: i64,
    max_response_bytes: i64,
    max_concurrent_calls: i32,
    calls_per_minute: i32,
    read_timeout_ms: i64,
}

fn require_management(state: &AppState, ctx: &IamContext) -> Result<()> {
    if !state.platform.license.has_feature(FEATURE) {
        return Err(Error::forbidden("Inbound MCP requires the agent feature"));
    }
    crate::api::http::middleware::Permission::require_key(ctx, "agent.manage")
}

fn endpoint(state: &AppState) -> String {
    let base = state.platform.external_url.trim().trim_end_matches('/');
    if base.is_empty() {
        "/api/v1/mcp".into()
    } else {
        format!("{base}/api/v1/mcp")
    }
}

fn settings_response(state: &AppState, settings: InboundMcpSettings) -> Value {
    let endpoint = endpoint(state);
    let base = endpoint.strip_suffix("/api/v1/mcp").unwrap_or_default();
    json!({
        "settings": settings,
        "endpoint": endpoint,
        "protocol_versions": ["2026-07-28", "2025-11-25", "2025-06-18", "2025-03-26"],
        "transports": ["streamable_http"],
        "oauth": {
            "protected_resource_metadata": format!("{base}/.well-known/oauth-protected-resource/api/v1/mcp"),
            "authorization_server_metadata": format!("{base}/.well-known/oauth-authorization-server"),
            "authorization_endpoint": format!("{base}/oauth/authorize"),
            "token_endpoint": format!("{base}/api/v1/oauth/token"),
            "registration_endpoint": format!("{base}/api/v1/oauth/register"),
            "revocation_endpoint": format!("{base}/api/v1/oauth/revoke"),
        },
        "hard_max_body_bytes": HARD_BODY_BYTES,
    })
}

#[permission("agent.manage")]
async fn get_settings(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    require_management(&state, &ctx)?;
    let settings = state
        .agent
        .inbound_mcp
        .get_settings(&ctx.org_id)
        .await?
        .unwrap_or_else(|| InboundMcpSettings::defaults(ctx.org_id));
    Ok(Json(settings_response(&state, settings)))
}

#[permission("agent.manage")]
async fn update_settings(
    State(state): State<AppState>,
    Extension(runtime): Extension<std::sync::Arc<InboundMcpAdapterRuntime>>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<UpdateSettingsRequest>,
) -> Result<Json<Value>> {
    require_management(&state, &ctx)?;
    validate_limits(&request)?;
    let origins = normalize_origins(request.allowed_origins)?;
    let now = TimestampMicros::now();
    let created_at = state
        .agent
        .inbound_mcp
        .get_settings(&ctx.org_id)
        .await?
        .map_or(now, |settings| settings.created_at);
    let settings = state
        .agent
        .inbound_mcp
        .upsert_settings(InboundMcpSettings {
            org_id: ctx.org_id,
            enabled: request.enabled,
            allowed_origins: origins,
            max_request_bytes: request.max_request_bytes,
            max_response_bytes: request.max_response_bytes,
            max_concurrent_calls: request.max_concurrent_calls,
            calls_per_minute: request.calls_per_minute,
            read_timeout_ms: request.read_timeout_ms,
            updated_by: ctx.user_id,
            created_at,
            updated_at: now,
        })
        .await?;
    runtime.notify_catalog_changed(&settings.org_id);
    Ok(Json(settings_response(&state, settings)))
}

fn validate_limits(request: &UpdateSettingsRequest) -> Result<()> {
    if !(1_024..=HARD_BODY_BYTES).contains(&request.max_request_bytes) {
        return Err(Error::invalid(format!(
            "max_request_bytes must be between 1024 and {HARD_BODY_BYTES}"
        )));
    }
    if !(1_024..=HARD_BODY_BYTES).contains(&request.max_response_bytes) {
        return Err(Error::invalid(format!(
            "max_response_bytes must be between 1024 and {HARD_BODY_BYTES}"
        )));
    }
    if !(1..=128).contains(&request.max_concurrent_calls) {
        return Err(Error::invalid(
            "max_concurrent_calls must be between 1 and 128",
        ));
    }
    if !(1..=10_000).contains(&request.calls_per_minute) {
        return Err(Error::invalid(
            "calls_per_minute must be between 1 and 10000",
        ));
    }
    if !(100..=300_000).contains(&request.read_timeout_ms) {
        return Err(Error::invalid(
            "read_timeout_ms must be between 100 and 300000",
        ));
    }
    Ok(())
}

fn normalize_origins(origins: Vec<String>) -> Result<Vec<String>> {
    let mut normalized = origins
        .into_iter()
        .map(|origin| {
            let origin = origin.trim();
            if origin.len() > 2_048 {
                return Err(Error::invalid("allowed origin exceeds 2048 bytes"));
            }
            let parsed = Url::parse(origin)
                .map_err(|error| Error::invalid(format!("invalid allowed origin: {error}")))?;
            if !matches!(parsed.scheme(), "http" | "https")
                || parsed.host_str().is_none()
                || !parsed.username().is_empty()
                || parsed.password().is_some()
                || parsed.path() != "/"
                || parsed.query().is_some()
                || parsed.fragment().is_some()
            {
                return Err(Error::invalid(
                    "allowed origins must be exact http(s) origins without path, query, or credentials",
                ));
            }
            Ok(parsed.origin().ascii_serialization())
        })
        .collect::<Result<Vec<_>>>()?;
    normalized.sort();
    normalized.dedup();
    if normalized.len() > 64 {
        return Err(Error::invalid(
            "at most 64 allowed origins may be configured",
        ));
    }
    Ok(normalized)
}

#[permission("agent.manage")]
async fn list_oauth_connections(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    require_management(&state, &ctx)?;
    let now = TimestampMicros::now();
    let connections = state
        .agent
        .inbound_mcp
        .list_oauth_connections(&ctx.org_id, now)
        .await?;
    Ok(Json(json!({ "connections": connections })))
}

#[permission("agent.manage")]
async fn revoke_oauth_connection(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(family_id): Path<String>,
) -> Result<Json<Value>> {
    require_management(&state, &ctx)?;
    if family_id.is_empty() || family_id.len() > 128 {
        return Err(Error::invalid("invalid OAuth connection family id"));
    }
    state
        .agent
        .inbound_mcp
        .revoke_oauth_family(
            &ctx.org_id,
            &Id::from_string(family_id),
            TimestampMicros::now(),
        )
        .await?;
    Ok(Json(json!({ "revoked": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_origins_are_canonical_and_deduplicated() {
        assert_eq!(
            normalize_origins(vec![
                "https://example.com".into(),
                "https://example.com/".into(),
            ])
            .expect("valid origins"),
            vec!["https://example.com"]
        );
        assert!(normalize_origins(vec!["https://example.com/path".into()]).is_err());
    }
}
