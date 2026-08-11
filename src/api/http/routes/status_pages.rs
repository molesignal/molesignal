// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Authenticated Status Page management and customer-facing access routes.

mod access;
mod events;
mod feed;
mod logo;
mod management;
mod public;
mod subscriptions;

use axum::{
    Router,
    extract::{Path, State},
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{CACHE_CONTROL, COOKIE, HOST},
        uri::Authority,
    },
    response::{IntoResponse, Response},
    routing::get,
};

use self::public::PublicStatusPageSnapshot;
use crate::{
    api::AppState,
    domain::status_page::{StatusPageSnapshot, StatusPageVisibility},
    shared::Result,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(management::routes())
        .merge(events::routes())
        .merge(subscriptions::management_routes())
        .merge(access::management_routes())
        .route(
            "/status-pages/{page_id}/logo",
            get(logo::get_logo).post(logo::upload_logo),
        )
}

pub fn public_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/public/status-pages/by-domain/current",
            get(get_customer_page_by_domain),
        )
        .route("/api/v1/public/status-pages/{slug}", get(get_customer_page))
        .merge(subscriptions::public_routes())
        .merge(access::public_routes())
        .route(
            "/api/v1/public/status-pages/{slug}/feed.rss",
            get(feed::rss),
        )
        .route(
            "/api/v1/public/status-pages/{slug}/logo/{file}",
            get(logo::serve_public_logo),
        )
}

async fn get_customer_page(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> Result<Response> {
    let cookie_name = state
        .status_pages
        .access_cookie_name_for_slug(&slug)
        .await?;
    let session = cookie_value(&headers, &cookie_name);
    let origin = request_hostname(&headers).unwrap_or_default();
    let snapshot = state
        .status_pages
        .get_customer_snapshot(&slug, session, &origin)
        .await?;
    Ok(customer_snapshot_response(snapshot))
}

async fn get_customer_page_by_domain(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response> {
    let Some(hostname) = request_hostname(&headers) else {
        return Ok(unmatched_domain_response());
    };
    let Some(page) = state
        .status_pages
        .resolve_customer_page_by_domain(&hostname)
        .await?
    else {
        return Ok(unmatched_domain_response());
    };
    let cookie_name = crate::app::status_page::access_cookie_name(&page.id);
    let session = cookie_value(&headers, &cookie_name);
    match state
        .status_pages
        .get_customer_snapshot_by_domain(&hostname, session)
        .await?
    {
        Some(snapshot) => Ok(customer_snapshot_response(snapshot)),
        None => Ok(unmatched_domain_response()),
    }
}

fn customer_snapshot_response(snapshot: StatusPageSnapshot) -> Response {
    let private = snapshot.page.visibility == StatusPageVisibility::Private;
    let mut response = axum::Json(PublicStatusPageSnapshot::from(snapshot)).into_response();
    response.headers_mut().insert(
        CACHE_CONTROL,
        if private {
            HeaderValue::from_static("private, no-store")
        } else {
            HeaderValue::from_static("public, max-age=30, stale-while-revalidate=60")
        },
    );
    response
}

fn unmatched_domain_response() -> Response {
    let mut response = StatusCode::NO_CONTENT.into_response();
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

pub(super) fn request_hostname(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(HOST)?.to_str().ok()?.trim();
    if value.contains('@') {
        return None;
    }
    let authority = value.parse::<Authority>().ok()?;
    Some(authority.host().trim_end_matches('.').to_ascii_lowercase())
}

pub(super) fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|cookie| {
                let (cookie_name, value) = cookie.trim().split_once('=')?;
                (cookie_name == name).then_some(value)
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_hostname_uses_host_header_and_removes_port() {
        let mut headers = HeaderMap::new();
        headers.insert(HOST, HeaderValue::from_static("Status.Acme.Example:8443"));
        assert_eq!(
            request_hostname(&headers).as_deref(),
            Some("status.acme.example")
        );
        headers.insert(HOST, HeaderValue::from_static("user@status.acme.example"));
        assert_eq!(request_hostname(&headers), None);
    }
}
