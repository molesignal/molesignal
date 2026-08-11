// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header::SET_COOKIE},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};

mod domain_management;
mod public_domain;

use super::request_hostname;
use crate::{
    api::{AppState, http::routes::activity_audit},
    app::{iam::IamContext, status_page::StatusPageAccessRuleInput},
    domain::{
        iam::permission,
        status_page::{StatusPageAccessRule, StatusPageAccessRuleKind, StatusPageAccessSession},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) fn management_routes() -> Router<AppState> {
    Router::new()
        .merge(domain_management::routes())
        .route(
            "/status-pages/{page_id}/access-rules",
            get(list_rules).post(create_rule),
        )
        .route(
            "/status-pages/{page_id}/access-rules/{rule_id}",
            delete(delete_rule),
        )
        .route(
            "/status-pages/{page_id}/access-sessions",
            get(list_sessions).delete(revoke_all_sessions),
        )
        .route(
            "/status-pages/{page_id}/access-sessions/{session_id}",
            delete(revoke_session),
        )
}

pub(super) fn public_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/public/status-pages/{slug}/access",
            get(access_metadata),
        )
        .route(
            "/api/v1/public/status-pages/{slug}/access/request",
            post(request_access),
        )
        .route(
            "/api/v1/public/status-pages/{slug}/access/consume",
            post(consume_access),
        )
        .merge(public_domain::routes())
}

#[derive(Debug, Deserialize)]
struct RuleRequest {
    kind: StatusPageAccessRuleKind,
    value: String,
}

#[derive(Debug, Deserialize)]
struct AccessRequest {
    email: String,
}

#[derive(Debug, Deserialize)]
struct AccessConsumeRequest {
    token: String,
}

#[derive(Debug, Serialize)]
struct AccessRuleResponse {
    id: Id,
    kind: StatusPageAccessRuleKind,
    masked_value: String,
    created_at: TimestampMicros,
    updated_at: TimestampMicros,
}

impl From<StatusPageAccessRule> for AccessRuleResponse {
    fn from(rule: StatusPageAccessRule) -> Self {
        Self {
            id: rule.id,
            kind: rule.kind,
            masked_value: mask_sensitive_target(rule.kind, &rule.value),
            created_at: rule.created_at,
            updated_at: rule.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct AccessSessionResponse {
    id: Id,
    masked_email: String,
    origin_host: String,
    expires_at: TimestampMicros,
    last_seen_at: TimestampMicros,
    created_at: TimestampMicros,
}

impl From<StatusPageAccessSession> for AccessSessionResponse {
    fn from(session: StatusPageAccessSession) -> Self {
        Self {
            id: session.id,
            masked_email: mask_email(&session.email),
            origin_host: session.origin_host,
            expires_at: session.expires_at,
            last_seen_at: session.last_seen_at,
            created_at: session.created_at,
        }
    }
}

#[permission("status_pages.manage")]
async fn list_rules(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(page_id): Path<String>,
) -> Result<Json<Vec<AccessRuleResponse>>> {
    Ok(Json(
        state
            .status_pages
            .list_access_rules(&context.org_id, &Id(page_id))
            .await?
            .into_iter()
            .map(AccessRuleResponse::from)
            .collect(),
    ))
}

#[permission("status_pages.manage")]
async fn create_rule(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(page_id): Path<String>,
    Json(request): Json<RuleRequest>,
) -> Result<(StatusCode, Json<AccessRuleResponse>)> {
    let page_id = Id(page_id);
    let rule = state
        .status_pages
        .create_access_rule(
            &context.org_id,
            &page_id,
            StatusPageAccessRuleInput {
                kind: request.kind,
                value: request.value,
            },
        )
        .await?;
    let rule_id = rule.id.clone();
    record_access_audit(
        &state,
        &context,
        "access_rule.create",
        &page_id,
        serde_json::json!({
            "rule_id": rule_id,
            "kind": rule.kind,
        }),
    )
    .await;
    Ok((StatusCode::CREATED, Json(rule.into())))
}

#[permission("status_pages.manage")]
async fn delete_rule(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((page_id, rule_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    let page_id = Id(page_id);
    state
        .status_pages
        .delete_access_rule(&context.org_id, &page_id, &Id(rule_id.clone()))
        .await?;
    record_access_audit(
        &state,
        &context,
        "access_rule.delete",
        &page_id,
        serde_json::json!({
            "rule_id": rule_id,
        }),
    )
    .await;
    Ok(Json(serde_json::json!({"deleted": true})))
}

#[permission("status_pages.manage")]
async fn list_sessions(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(page_id): Path<String>,
) -> Result<Json<Vec<AccessSessionResponse>>> {
    Ok(Json(
        state
            .status_pages
            .list_access_sessions(&context.org_id, &Id(page_id))
            .await?
            .into_iter()
            .map(AccessSessionResponse::from)
            .collect(),
    ))
}

#[permission("status_pages.manage")]
async fn revoke_session(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((page_id, session_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    let page_id = Id(page_id);
    state
        .status_pages
        .revoke_access_session(&context.org_id, &page_id, &Id(session_id.clone()))
        .await?;
    record_access_audit(
        &state,
        &context,
        "access_session.revoke",
        &page_id,
        serde_json::json!({
            "session_id": session_id,
        }),
    )
    .await;
    Ok(Json(serde_json::json!({"revoked": true})))
}

#[permission("status_pages.manage")]
async fn revoke_all_sessions(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(page_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let page_id = Id(page_id);
    let count = state
        .status_pages
        .revoke_all_access_sessions(&context.org_id, &page_id)
        .await?;
    record_access_audit(
        &state,
        &context,
        "access_session.revoke_all",
        &page_id,
        serde_json::json!({
            "count": count,
        }),
    )
    .await;
    Ok(Json(serde_json::json!({"revoked": count})))
}

async fn access_metadata(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<crate::app::status_page::StatusPageAccessMetadata>> {
    Ok(Json(state.status_pages.access_metadata(&slug).await?))
}

async fn request_access(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
    Json(request): Json<AccessRequest>,
) -> Result<(
    StatusCode,
    Json<crate::app::status_page::StatusPageAccessRequestOutcome>,
)> {
    let origin = request_hostname(&headers).unwrap_or_default();
    let outcome = state
        .status_pages
        .request_private_access(&slug, &request.email, &origin)
        .await?;
    Ok((StatusCode::ACCEPTED, Json(outcome)))
}

async fn consume_access(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
    Json(request): Json<AccessConsumeRequest>,
) -> Result<Response> {
    let origin = request_hostname(&headers).unwrap_or_default();
    let outcome = state
        .status_pages
        .consume_private_access(&slug, &request.token, &origin)
        .await?;
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

async fn record_access_audit(
    state: &AppState,
    context: &IamContext,
    verb: &str,
    page_id: &Id,
    payload: serde_json::Value,
) {
    activity_audit::record(
        state,
        context,
        &format!("status_page.{verb}"),
        "status_page",
        page_id.as_str(),
        payload,
    )
    .await;
}

fn mask_sensitive_target(kind: StatusPageAccessRuleKind, value: &str) -> String {
    match kind {
        StatusPageAccessRuleKind::Email => mask_email(value),
        StatusPageAccessRuleKind::Domain => {
            let value = value.trim_start_matches('@');
            let (first, rest) = value.split_once('.').unwrap_or((value, ""));
            let first = first.chars().next().unwrap_or('*');
            if rest.is_empty() {
                format!("@{first}***")
            } else {
                format!("@{first}***.{rest}")
            }
        }
    }
}

pub(super) fn mask_email(value: &str) -> String {
    let Some((local, domain)) = value.rsplit_once('@') else {
        return "***".into();
    };
    format!("{}***@{domain}", local.chars().next().unwrap_or('*'))
}
