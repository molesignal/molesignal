// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! pprof compatibility aliases under `/api/v1/debug/profile/*`.

use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;

use crate::{
    api::{AppState, http::middleware::auth::authenticate_bearer},
    app::profiling::{CaptureError, CapturedProfile},
    infra::profiles,
    shared::{Error, self_telemetry::with_suppression},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/debug/profile/cpu", get(cpu_profile_alias))
        .route("/debug/profile/heap", get(heap_profile_alias))
}

/// 独立 listener 复用相同 handler，仅换成 Go pprof 约定路径。
pub fn pprof_routes() -> Router<AppState> {
    Router::new()
        .route("/debug/pprof/profile", get(cpu_profile))
        .route("/debug/pprof/heap", get(heap_profile))
}

#[derive(Debug, Deserialize, Default)]
pub struct CpuParams {
    #[serde(default = "default_seconds")]
    pub seconds: u32,
}

fn default_seconds() -> u32 {
    30
}

pub(crate) async fn cpu_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<CpuParams>,
) -> Response {
    if let Some(response) = preflight(&state, &headers, false).await {
        return response;
    }
    match state
        .telemetry
        .profiling_service
        .capture_cpu(params.seconds)
        .await
    {
        Ok(captured) => capture_response(&state, "cpu", captured),
        Err(error) => capture_error_response(error),
    }
}

pub(crate) async fn heap_profile(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(response) = preflight(&state, &headers, false).await {
        return response;
    }
    match state.telemetry.profiling_service.capture_heap().await {
        Ok(captured) => capture_response(&state, "heap", captured),
        Err(error) => capture_error_response(error),
    }
}

async fn cpu_profile_alias(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<CpuParams>,
) -> Response {
    if let Some(response) = preflight(&state, &headers, true).await {
        return response;
    }
    match state
        .telemetry
        .profiling_service
        .capture_cpu(params.seconds)
        .await
    {
        Ok(captured) => capture_response(&state, "cpu", captured),
        Err(error) => capture_error_response(error),
    }
}

async fn heap_profile_alias(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(response) = preflight(&state, &headers, true).await {
        return response;
    }
    match state.telemetry.profiling_service.capture_heap().await {
        Ok(captured) => capture_response(&state, "heap", captured),
        Err(error) => capture_error_response(error),
    }
}

async fn preflight(
    state: &AppState,
    headers: &HeaderMap,
    compatibility_alias: bool,
) -> Option<Response> {
    match access_policy(&state.telemetry.profiling_settings, compatibility_alias) {
        ProfileAccess::Hidden => Some(StatusCode::NOT_FOUND.into_response()),
        ProfileAccess::Local => None,
        ProfileAccess::Administrator => match authorize_administrator(state, headers).await {
            Ok(()) => None,
            Err(error) => Some(error.into_response()),
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProfileAccess {
    Hidden,
    Local,
    Administrator,
}

fn access_policy(
    settings: &crate::config::ProfilingSettings,
    compatibility_alias: bool,
) -> ProfileAccess {
    if !settings.enabled {
        return ProfileAccess::Hidden;
    }
    // 独立 listener 由 validated loopback bind 保护；主 API 上的兼容别名可能对外，
    // 因此无论 allow_remote 配置如何都要求 Administrator。
    if compatibility_alias || settings.allow_remote {
        ProfileAccess::Administrator
    } else {
        ProfileAccess::Local
    }
}

async fn authorize_administrator(state: &AppState, headers: &HeaderMap) -> Result<(), Error> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(|| Error::unauthorized("remote profiling requires an Administrator token"))?;
    let mut context = authenticate_bearer(
        token,
        state.iam.service.as_ref(),
        state.iam.api_tokens.clone(),
    )
    .await?;
    state.iam.access.enrich_context(&mut context).await?;
    if !context.has_permission("org.settings.manage") {
        return Err(Error::forbidden(
            "remote profiling requires org.settings.manage",
        ));
    }
    Ok(())
}

fn capture_response(state: &AppState, kind: &'static str, captured: CapturedProfile) -> Response {
    let response = match profile_download_response(kind, &captured.raw_pprof) {
        Ok(response) => response,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "profile encoding failed",
                    "detail": error,
                })),
            )
                .into_response();
        }
    };

    // 下载已完整生成后再异步归档；归档失败绝不改变或截断本次响应。
    if state.telemetry.self_telemetry_profiles_enabled
        && let Some(runtime) = state.telemetry.self_telemetry_runtime.clone()
    {
        tokio::spawn(with_suppression(async move {
            if let Err(error) = runtime.persist_profile(captured).await {
                tracing::warn!(
                    target: "molesignal::app::self_telemetry",
                    error = %error,
                    "on-demand self profile persistence failed"
                );
            }
        }));
    }

    response
}

fn profile_download_response(
    kind: &'static str,
    raw_pprof: &[u8],
) -> std::result::Result<Response, String> {
    let body = profiles::gzip_pprof(raw_pprof).map_err(|error| error.to_string())?;
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/octet-stream"),
            (
                header::CONTENT_DISPOSITION,
                if kind == "cpu" {
                    "attachment; filename=\"profile.pb.gz\""
                } else {
                    "attachment; filename=\"heap.pb.gz\""
                },
            ),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        body,
    )
        .into_response())
}

fn capture_error_response(error: CaptureError) -> Response {
    match error {
        CaptureError::InvalidDuration => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "invalid profile duration",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
        CaptureError::Busy => (
            StatusCode::CONFLICT,
            [(header::RETRY_AFTER, "1")],
            Json(serde_json::json!({
                "error": "profile capture already running",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
        CaptureError::Unavailable(kind) => (
            StatusCode::NOT_IMPLEMENTED,
            Json(serde_json::json!({
                "error": format!("{kind} profile unavailable"),
                "detail": error.to_string(),
            })),
        )
            .into_response(),
        CaptureError::Failed(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "profile capture failed",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use axum::body::to_bytes;

    use super::*;
    use crate::{
        config::ProfilingSettings,
        infra::profiles::{NormalizedProfile, ProfileType, ValueType},
    };

    #[test]
    fn listener_and_compatibility_aliases_apply_the_expected_access_policy() {
        let disabled = ProfilingSettings::default();
        assert_eq!(access_policy(&disabled, false), ProfileAccess::Hidden);

        let local = ProfilingSettings {
            enabled: true,
            ..ProfilingSettings::default()
        };
        assert_eq!(access_policy(&local, false), ProfileAccess::Local);
        assert_eq!(access_policy(&local, true), ProfileAccess::Administrator);

        let remote = ProfilingSettings {
            allow_remote: true,
            ..local
        };
        assert_eq!(access_policy(&remote, false), ProfileAccess::Administrator);
    }

    #[test]
    fn capture_errors_use_pprof_compatible_http_statuses() {
        let invalid = capture_error_response(CaptureError::InvalidDuration);
        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);

        let busy = capture_error_response(CaptureError::Busy);
        assert_eq!(busy.status(), StatusCode::CONFLICT);
        assert_eq!(busy.headers()[header::RETRY_AFTER], "1");

        let unavailable = capture_error_response(CaptureError::Unavailable("heap"));
        assert_eq!(unavailable.status(), StatusCode::NOT_IMPLEMENTED);
    }

    #[tokio::test]
    async fn profile_download_is_gzipped_canonical_pprof() {
        let normalized = NormalizedProfile {
            service: "molesignal".into(),
            profile_type: ProfileType::Cpu,
            sample_types: vec![ValueType::new("samples", "count")],
            default_value_index: 0,
            samples: Vec::new(),
            period_type: None,
            period: 0,
            start_time_micros: 1,
            duration_nanos: 1,
            labels: BTreeMap::new(),
            trace_id: None,
            span_id: None,
        };
        let raw = profiles::encode_pprof_raw(&normalized).unwrap();
        let response = profile_download_response("cpu", &raw).unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CONTENT_DISPOSITION],
            "attachment; filename=\"profile.pb.gz\""
        );
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let decompressed = profiles::decompress_pprof_input(&body).unwrap();
        assert!(profiles::decode_pprof(&decompressed).is_ok());
    }
}
