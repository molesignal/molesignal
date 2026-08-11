// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Private Status Page Magic Link flow when the page is opened on its custom domain.

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, header::SET_COOKIE},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;

use super::super::request_hostname;
use crate::{
    api::AppState,
    app::status_page::{
        StatusPageAccessMetadata, StatusPageAccessRequestOutcome, StatusPageAccessSessionOutcome,
    },
    shared::{Error, Result, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/public/status-pages/by-domain/access",
            get(access_metadata),
        )
        .route(
            "/api/v1/public/status-pages/by-domain/access/request",
            post(request_access),
        )
        .route(
            "/api/v1/public/status-pages/by-domain/access/consume",
            post(consume_access),
        )
}

#[derive(Debug, Deserialize)]
struct AccessRequest {
    email: String,
}

#[derive(Debug, Deserialize)]
struct AccessConsumeRequest {
    token: String,
}

async fn access_metadata(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<StatusPageAccessMetadata>> {
    let page = resolve_page(&state, &headers).await?;
    Ok(Json(state.status_pages.access_metadata(&page.slug).await?))
}

async fn request_access(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<AccessRequest>,
) -> Result<(StatusCode, Json<StatusPageAccessRequestOutcome>)> {
    let page = resolve_page(&state, &headers).await?;
    let origin = request_hostname(&headers)
        .ok_or_else(|| Error::unauthorized("custom-domain host is required"))?;
    let outcome = state
        .status_pages
        .request_private_access(&page.slug, &request.email, &origin)
        .await?;
    Ok((StatusCode::ACCEPTED, Json(outcome)))
}

async fn consume_access(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<AccessConsumeRequest>,
) -> Result<Response> {
    let page = resolve_page(&state, &headers).await?;
    let origin = request_hostname(&headers)
        .ok_or_else(|| Error::unauthorized("custom-domain host is required"))?;
    let outcome = state
        .status_pages
        .consume_private_access(&page.slug, &request.token, &origin)
        .await?;
    session_response(&state, &headers, outcome)
}

async fn resolve_page(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<crate::domain::status_page::StatusPage> {
    let hostname = request_hostname(headers)
        .ok_or_else(|| Error::not_found("custom-domain status page was not found"))?;
    state
        .status_pages
        .resolve_customer_page_by_domain(&hostname)
        .await?
        .ok_or_else(|| Error::not_found("custom-domain status page was not found"))
}

fn session_response(
    state: &AppState,
    headers: &HeaderMap,
    outcome: StatusPageAccessSessionOutcome,
) -> Result<Response> {
    let cookie_name = crate::app::status_page::access_cookie_name(&outcome.page.id);
    let max_age = outcome
        .session
        .expires_at
        .0
        .saturating_sub(TimestampMicros::now().0)
        / 1_000_000;
    let mut cookie = format!(
        "{cookie_name}={}; Path=/; Max-Age={}; HttpOnly; SameSite=Lax",
        outcome.raw_session_token,
        max_age.max(0),
    );
    if state.platform.external_url.trim().starts_with("https://")
        || headers
            .get("x-forwarded-proto")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.eq_ignore_ascii_case("https"))
    {
        cookie.push_str("; Secure");
    }
    let mut response = Json(serde_json::json!({
        "authenticated": true,
        "expires_at": outcome.session.expires_at,
    }))
    .into_response();
    response.headers_mut().insert(
        SET_COOKIE,
        cookie
            .parse()
            .map_err(|_| Error::internal("status-page session cookie is invalid"))?,
    );
    Ok(response)
}
