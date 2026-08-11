// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Resource-scoped sharing.
//!
//! A public link is an opaque bearer credential for one resource, not a way to
//! mint a normal user token. The link is exchanged once for a short-lived,
//! HttpOnly share session; public dashboard reads then go through a saved-query
//! proxy that never accepts an arbitrary statement from the browser.

use std::collections::{BTreeMap, BTreeSet};

use axum::{
    Extension, Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{
        HeaderMap, StatusCode,
        header::{
            CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE, COOKIE, LOCATION, REFERRER_POLICY,
            SET_COOKIE, USER_AGENT,
        },
    },
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::Engine as _;
use object_store::{ObjectStoreExt, PutPayload, path::Path as ObjectPath};
use rand::TryRng as _;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::{
    api::{
        AppState,
        http::middleware::{
            Permission, ProtectedResource, authorize_resource as authorize_protected_resource,
        },
    },
    app::iam::{IamContext, hash_password, verify_password},
    domain::{
        dashboard::Dashboard,
        iam::{
            access::{IamCrossOrgGrant, IamCrossOrgGrantStatus, IamPrincipalType},
            permission, resource_permission,
        },
        query::{QueryLanguage, QueryRequest, StreamHint},
        stream::StreamType,
    },
    infra::persistence::repositories::{
        audit_events::AuditEvent,
        resource_shares::{
            ResourceShare, ResourceShareMode, ResourceSharePolicy, ResourceShareSession,
        },
        scheduled_reports::ScheduledReport,
    },
    shared::{
        Error, ReportFormat, Result, Viewport,
        ids::Id,
        time::{TimeRange, TimestampMicros},
        validate_report_bytes,
    },
};

const SHARE_SESSION_COOKIE: &str = "molesignal_share_session";
const SHARE_SESSION_SECS: i64 = 30 * 60;
const DEFAULT_DASHBOARD_RANGE_SECS: i64 = 60 * 60;
const MAX_DASHBOARD_RANGE_SECS: i64 = 24 * 60 * 60;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/resource_shares", get(list).post(create))
        .route("/resource_shares/{id}", get(get_one).delete(revoke))
        .route("/resource_shares/{id}/rotate", post(rotate))
        .route(
            "/resource_shares/policy",
            get(get_policy).put(update_policy),
        )
        .route("/public/share", get(public_metadata))
        .route("/public/share/unlock", post(public_unlock))
        .route("/public/share/query", post(public_query))
        .route("/public/share/file", get(public_file))
}

enum ShareableResourceId {
    Dashboard(Id),
    Report(Id),
}

impl ShareableResourceId {
    fn new(resource_type: &str, resource_id: &str) -> Result<Self> {
        let resource_id = resource_id.trim();
        if resource_id.is_empty() {
            return Err(Error::invalid("resource_id is required"));
        }
        match resource_type {
            "dashboard" => Ok(Self::Dashboard(Id::from_string(resource_id))),
            "report" => Ok(Self::Report(Id::from_string(resource_id))),
            _ => Err(Error::invalid("resource_type must be dashboard or report")),
        }
    }
}

enum ShareableResource {
    Dashboard(Dashboard),
    Report(ScheduledReport),
}

impl ShareableResource {
    fn into_metadata(self) -> (Option<String>, String, Option<ScheduledReport>) {
        match self {
            Self::Dashboard(dashboard) => {
                (Some(dashboard.version.to_string()), dashboard.title, None)
            }
            Self::Report(report) => (
                Some(report.updated_at.0.to_string()),
                report.name.clone(),
                Some(report),
            ),
        }
    }
}

#[async_trait::async_trait]
impl ProtectedResource for ShareableResource {
    type Id = ShareableResourceId;

    async fn load(state: &AppState, id: Self::Id) -> Result<Self> {
        match id {
            ShareableResourceId::Dashboard(id) => {
                state.dashboard.get(&id).await.map(Self::Dashboard)
            }
            ShareableResourceId::Report(id) => state
                .platform
                .scheduled_reports
                .get_by_id(&id)
                .await
                .map(Self::Report),
        }
    }

    fn organization_id(&self) -> &Id {
        match self {
            Self::Dashboard(dashboard) => &dashboard.org_id,
            Self::Report(report) => &report.org_id,
        }
    }

    fn resource_type(&self) -> &str {
        match self {
            Self::Dashboard(_) => "dashboard",
            Self::Report(_) => "report",
        }
    }

    fn resource_id(&self) -> &str {
        match self {
            Self::Dashboard(dashboard) => dashboard.id.as_str(),
            Self::Report(report) => report.id.as_str(),
        }
    }
}

#[async_trait::async_trait]
impl ProtectedResource for ResourceShare {
    type Id = Id;

    async fn load(state: &AppState, id: Self::Id) -> Result<Self> {
        state.storage.resource_shares.get_by_id(&id).await
    }

    fn organization_id(&self) -> &Id {
        &self.organization_id
    }

    fn resource_type(&self) -> &str {
        normalized_share_resource_type(&self.resource_type)
    }

    fn resource_id(&self) -> &str {
        self.resource_id.as_str()
    }
}

pub fn redirect_routes() -> Router<AppState> {
    Router::new().route("/s/{token}", get(resolve_share_link))
}

#[derive(Debug, Deserialize)]
struct CreateRequest {
    resource_type: String,
    resource_id: String,
    share_mode: ResourceShareMode,
    #[serde(default)]
    resource_version_id: Option<String>,
    #[serde(default)]
    expires_in_secs: Option<i64>,
    #[serde(default)]
    password: Option<String>,
    #[serde(default)]
    max_views: Option<i64>,
    #[serde(default)]
    allow_download: bool,
    #[serde(default = "empty_object")]
    constraints: Value,
    #[serde(default)]
    target_organization_id: Option<String>,
    #[serde(default)]
    grantee_type: Option<IamPrincipalType>,
    #[serde(default)]
    grantee_id: Option<String>,
}

#[derive(Serialize)]
struct CreateResponse {
    share: ResourceShareView,
    url: String,
}

#[derive(Serialize)]
struct ResourceShareView {
    #[serde(flatten)]
    share: ResourceShare,
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    #[serde(default)]
    resource_type: Option<String>,
    #[serde(default)]
    resource_id: Option<String>,
}

#[derive(Serialize)]
struct RotateResponse {
    share: ResourceShareView,
    url: String,
}

#[derive(Debug, Deserialize)]
struct PolicyUpdateRequest {
    allow_public_links: bool,
    allow_public_dashboards: bool,
    max_public_expiry_secs: i64,
    require_public_report_password: bool,
    deny_production_public_shares: bool,
    allow_public_csv_download: bool,
}

#[resource_permission(
    action = dynamic(share_permission(&request.resource_type)?),
    resource = ShareableResource,
    id = ShareableResourceId::new(&request.resource_type, &request.resource_id)?,
    bind = authorized_resource
)]
async fn create(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Json(request): Json<CreateRequest>,
) -> Result<Json<CreateResponse>> {
    if request.max_views.is_some_and(|value| value <= 0) {
        return Err(Error::invalid("max_views must be greater than zero"));
    }

    let resource_id = Id::from_string(authorized_resource.resource_id());
    let now = TimestampMicros::now();
    let share_id = Id::new();
    let (resource_version_id, resource_title, report) = authorized_resource.into_metadata();

    let policy = state
        .storage
        .resource_shares
        .get_policy(&context.org_id)
        .await?;
    let expires_at = expiration_for(&request, &policy, now)?;
    let mut normalized_constraints = request.constraints.clone();
    let mut allow_download = request.allow_download;
    let password_hash = if request.share_mode == ResourceShareMode::PublicLink {
        validate_public_policy(
            &policy,
            &request,
            report.as_ref(),
            &resource_title,
            &state,
            &resource_id,
        )
        .await?;
        normalized_constraints = normalize_public_constraints(
            &request.resource_type,
            &request.constraints,
            &state,
            &resource_id,
        )
        .await?;
        if request.resource_type == "dashboard" {
            allow_download = false;
        }
        let password = request
            .password
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty());
        validate_public_share_password(
            &request.resource_type,
            policy.require_public_report_password,
            password,
        )?;
        password.map(hash_password).transpose()?
    } else {
        None
    };

    let (cross_org_grant_id, permissions) = match request.share_mode {
        ResourceShareMode::Authenticated => {
            (None, json!([read_permission(&request.resource_type)?]))
        }
        ResourceShareMode::CrossOrg => {
            Permission::require_key(&context, "iam.policies.manage")?;
            let grant =
                create_cross_org_grant(&state, &context, &request, &resource_id, expires_at)
                    .await?;
            (
                Some(grant.id),
                json!([read_permission(&request.resource_type)?]),
            )
        }
        ResourceShareMode::PublicLink => {
            if request.resource_type == "dashboard" {
                (None, json!(["dashboards.view", "dashboard_panels.execute"]))
            } else {
                (None, json!(["reports.view"]))
            }
        }
    };

    let raw_token = generate_share_token();
    let mut share = ResourceShare {
        id: share_id,
        organization_id: context.org_id.clone(),
        resource_type: request.resource_type.clone(),
        resource_id,
        resource_version_id: request.resource_version_id.or(resource_version_id),
        share_mode: request.share_mode,
        token_hash: token_hash(&raw_token),
        raw_token: Some(raw_token.clone()),
        permissions,
        constraints: normalized_constraints,
        expires_at,
        password_hash,
        max_views: request.max_views,
        view_count: 0,
        allow_download,
        enabled: true,
        cross_org_grant_id,
        snapshot_object_key: None,
        snapshot_content_type: None,
        snapshot_filename: None,
        created_by: context.user_id.clone(),
        created_at: now,
        last_accessed_at: None,
        revoked_at: None,
    };

    if request.share_mode == ResourceShareMode::PublicLink
        && let Some(report) = report
    {
        let snapshot = render_report_snapshot(&state, &context, &share.id, &report).await?;
        share.resource_version_id = Some(report.updated_at.0.to_string());
        share.snapshot_object_key = Some(snapshot.object_key);
        share.snapshot_content_type = Some(snapshot.content_type);
        share.snapshot_filename = Some(snapshot.filename);
    }

    let share = state.storage.resource_shares.create(share).await?;
    record_audit(
        &state,
        &share,
        "resource_share.create",
        "user",
        &context.user_id.0,
        None,
        json!({
            "share_mode": share.share_mode,
            "resource_type": share.resource_type,
            "expires_at_micros": share.expires_at.map(|value| value.0),
            "allow_download": share.allow_download,
            "has_password": share.password_hash.is_some(),
            "max_views": share.max_views,
            "cross_org_grant_id": share.cross_org_grant_id.as_ref().map(|value| value.0.clone()),
        }),
    )
    .await?;

    Ok(Json(CreateResponse {
        share: resource_share_view(share),
        url: format!("/s/{raw_token}"),
    }))
}

#[permission(any("dashboards.share", "reports.share", "org.settings.manage"))]
async fn list(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<ResourceShareView>>> {
    let resource_id = query.resource_id.map(Id::from_string);
    match (query.resource_type.as_deref(), resource_id.as_ref()) {
        (Some(resource_type), Some(resource_id)) => {
            authorize_shareable_filter(&state, &context, resource_type, resource_id).await?;
        }
        (None, None) => Permission::require_key(&context, "org.settings.manage")?,
        _ => {
            return Err(Error::invalid(
                "resource_type and resource_id must be provided together",
            ));
        }
    }
    let shares = state
        .storage
        .resource_shares
        .list(
            &context.org_id,
            query.resource_type.as_deref(),
            resource_id.as_ref(),
        )
        .await?;
    Ok(Json(
        shares
            .into_iter()
            .filter(|share| can_manage_share(&context, &share.resource_type))
            .map(resource_share_view)
            .collect(),
    ))
}

#[resource_permission(
    action = resolve(share_permission_for_record),
    resource = ResourceShare,
    id = Id::from_string(id),
    bind = share
)]
async fn get_one(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<ResourceShareView>> {
    Ok(Json(resource_share_view(share)))
}

#[resource_permission(
    action = resolve(share_permission_for_record),
    resource = ResourceShare,
    id = Id::from_string(id),
    bind = existing
)]
async fn revoke(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<ResourceShareView>> {
    let id = existing.id.clone();
    if let Some(grant_id) = &existing.cross_org_grant_id {
        let _ = state
            .iam
            .access
            .repository()
            .set_cross_org_grant_status(
                &context.org_id,
                grant_id,
                IamCrossOrgGrantStatus::Revoked,
                &context.user_id,
                TimestampMicros::now(),
            )
            .await?;
    }
    let share = state
        .storage
        .resource_shares
        .revoke(&context.org_id, &id, TimestampMicros::now())
        .await?;
    record_audit(
        &state,
        &share,
        "resource_share.revoke",
        "user",
        &context.user_id.0,
        None,
        json!({}),
    )
    .await?;
    Ok(Json(resource_share_view(share)))
}

#[resource_permission(
    action = resolve(share_permission_for_record),
    resource = ResourceShare,
    id = Id::from_string(id),
    bind = existing
)]
async fn rotate(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<RotateResponse>> {
    let id = existing.id.clone();
    let raw_token = generate_share_token();
    let share = state
        .storage
        .resource_shares
        .rotate_token(&context.org_id, &id, &token_hash(&raw_token), &raw_token)
        .await?;
    record_audit(
        &state,
        &share,
        "resource_share.rotate",
        "user",
        &context.user_id.0,
        None,
        json!({}),
    )
    .await?;
    Ok(Json(RotateResponse {
        share: resource_share_view(share),
        url: format!("/s/{raw_token}"),
    }))
}

#[permission(any(
    "org.settings.read",
    "org.settings.manage",
    "dashboards.share",
    "reports.share"
))]
async fn get_policy(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
) -> Result<Json<ResourceSharePolicy>> {
    Ok(Json(
        state
            .storage
            .resource_shares
            .get_policy(&context.org_id)
            .await?,
    ))
}

#[permission("org.settings.manage")]
async fn update_policy(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Json(request): Json<PolicyUpdateRequest>,
) -> Result<Json<ResourceSharePolicy>> {
    if !(3600..=2_592_000).contains(&request.max_public_expiry_secs) {
        return Err(Error::invalid(
            "max_public_expiry_secs must be between 3600 and 2592000",
        ));
    }
    let policy = state
        .storage
        .resource_shares
        .upsert_policy(ResourceSharePolicy {
            organization_id: context.org_id.clone(),
            allow_public_links: request.allow_public_links,
            allow_public_dashboards: request.allow_public_dashboards,
            max_public_expiry_secs: request.max_public_expiry_secs,
            require_public_report_password: request.require_public_report_password,
            deny_production_public_shares: request.deny_production_public_shares,
            allow_public_csv_download: request.allow_public_csv_download,
            updated_by: context.user_id.clone(),
            updated_at: TimestampMicros::now(),
        })
        .await?;
    state
        .iam
        .audit_events
        .record(AuditEvent {
            id: Id::new(),
            org_id: context.org_id.clone(),
            actor_kind: "user".into(),
            actor_id: context.user_id.0,
            action: "resource_share.policy.update".into(),
            target_kind: Some("resource_share_policy".into()),
            target_id: Some(context.org_id.0),
            ip: None,
            user_agent: None,
            payload: json!({
                "allow_public_links": policy.allow_public_links,
                "allow_public_dashboards": policy.allow_public_dashboards,
                "max_public_expiry_secs": policy.max_public_expiry_secs,
                "require_public_report_password": policy.require_public_report_password,
                "deny_production_public_shares": policy.deny_production_public_shares,
                "allow_public_csv_download": policy.allow_public_csv_download,
            }),
            ts: TimestampMicros::now(),
        })
        .await?;
    Ok(Json(policy))
}

async fn resolve_share_link(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
) -> Result<Response> {
    let now = TimestampMicros::now();
    let share = state
        .storage
        .resource_shares
        .find_by_token_hash(&token_hash(&token))
        .await?
        .ok_or_else(|| Error::not_found("resource share not found"))?;
    state
        .iam
        .service
        .ensure_organization_access(&share.organization_id)
        .await?;
    if !share_is_active(&share, now) {
        return Ok((
            StatusCode::GONE,
            [(CACHE_CONTROL, "no-store")],
            "resource share is expired or revoked",
        )
            .into_response());
    }

    if share.share_mode != ResourceShareMode::PublicLink {
        let location = authenticated_target(&share)?;
        record_audit(
            &state,
            &share,
            "resource_share.open",
            "share_link",
            &share.id.0,
            Some(&headers),
            json!({"result": "redirect_to_authenticated_resource"}),
        )
        .await?;
        return Ok((
            StatusCode::FOUND,
            [
                (LOCATION, location.as_str()),
                (CACHE_CONTROL, "no-store"),
                (REFERRER_POLICY, "no-referrer"),
            ],
        )
            .into_response());
    }

    let raw_session = generate_opaque_token();
    let session = ResourceShareSession {
        id: Id::new(),
        share_id: share.id.clone(),
        session_token_hash: token_hash(&raw_session),
        password_verified: share.password_hash.is_none(),
        created_at: now,
        expires_at: TimestampMicros(
            now.0
                .saturating_add(SHARE_SESSION_SECS.saturating_mul(1_000_000)),
        ),
        last_seen_at: now,
        ip: request_ip(&headers),
        user_agent: header_text(&headers, USER_AGENT),
    };
    let share = state
        .storage
        .resource_shares
        .create_session(session, now)
        .await?;
    let mut cookie = format!(
        "{SHARE_SESSION_COOKIE}={raw_session}; Path=/; Max-Age={SHARE_SESSION_SECS}; HttpOnly; SameSite=Lax"
    );
    if state.platform.external_url.trim().starts_with("https://") {
        cookie.push_str("; Secure");
    }
    record_audit(
        &state,
        &share,
        "resource_share.open",
        "share_session",
        &share.id.0,
        Some(&headers),
        json!({"result": "session_created", "view_count": share.view_count}),
    )
    .await?;
    Response::builder()
        .status(StatusCode::FOUND)
        .header(LOCATION, "/shared")
        .header(SET_COOKIE, cookie)
        .header(CACHE_CONTROL, "no-store")
        .header(REFERRER_POLICY, "no-referrer")
        .body(Body::empty())
        .map_err(|error| Error::internal(format!("share redirect response: {error}")))
}

async fn public_metadata(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Value>> {
    let (session, share) = public_share_context(&state, &headers, false).await?;
    state
        .storage
        .resource_shares
        .touch_session(&session.id, TimestampMicros::now())
        .await?;
    let requires_password = share.password_hash.is_some() && !session.password_verified;
    if requires_password {
        return Ok(Json(json!({
            "kind": share.resource_type,
            "requires_password": true,
            "expires_at_micros": share.expires_at.map(|value| value.0),
        })));
    }

    let value = match share.resource_type.as_str() {
        "dashboard" => {
            let dashboard = state.dashboard.get(&share.resource_id).await?;
            let definition =
                sanitize_dashboard_model(dashboard.model, &share.id, &share.constraints);
            json!({
                "kind": "dashboard",
                "title": dashboard.title,
                "requires_password": false,
                "expires_at_micros": share.expires_at.map(|value| value.0),
                "constraints": share.constraints,
                "definition": definition,
                "watermark": {
                    "share_id": share.id.0,
                    "accessed_at_micros": TimestampMicros::now().0,
                },
            })
        }
        "report" | "report_file" => {
            let report = state
                .platform
                .scheduled_reports
                .get_by_id(&share.resource_id)
                .await?;
            json!({
                "kind": "report",
                "title": report.name,
                "format": report.format,
                "requires_password": false,
                "allow_download": share.allow_download,
                "expires_at_micros": share.expires_at.map(|value| value.0),
                "generated_at_micros": share.created_at.0,
                "content_type": share.snapshot_content_type,
                "watermark": {
                    "share_id": share.id.0,
                    "accessed_at_micros": TimestampMicros::now().0,
                },
            })
        }
        _ => return Err(Error::invalid("unsupported shared resource type")),
    };
    Ok(Json(value))
}

#[derive(Debug, Deserialize)]
struct UnlockRequest {
    password: String,
}

async fn public_unlock(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<UnlockRequest>,
) -> Result<Json<Value>> {
    let (session, share) = public_share_context(&state, &headers, false).await?;
    let Some(password_hash) = share.password_hash.as_deref() else {
        return Ok(Json(json!({"unlocked": true})));
    };
    if verify_password(&request.password, password_hash).is_err() {
        record_audit(
            &state,
            &share,
            "resource_share.password_failure",
            "share_session",
            &session.id.0,
            Some(&headers),
            json!({"result": "denied"}),
        )
        .await?;
        return Err(Error::unauthorized("invalid share password"));
    }
    state
        .storage
        .resource_shares
        .mark_password_verified(&session.id)
        .await?;
    record_audit(
        &state,
        &share,
        "resource_share.unlock",
        "share_session",
        &session.id.0,
        Some(&headers),
        json!({"result": "allowed"}),
    )
    .await?;
    Ok(Json(json!({"unlocked": true})))
}

#[derive(Debug, Deserialize)]
struct PublicQueryRequest {
    panel_id: String,
    ref_id: String,
    from_micros: i64,
    to_micros: i64,
    #[serde(default)]
    variables: BTreeMap<String, Value>,
}

async fn public_query(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<PublicQueryRequest>,
) -> Result<Json<crate::domain::query::QueryResult>> {
    let (_session, share) = public_share_context(&state, &headers, true).await?;
    if share.resource_type != "dashboard" {
        return Err(Error::invalid(
            "public query is only available for dashboard shares",
        ));
    }
    let dashboard = state.dashboard.get(&share.resource_id).await?;
    let query = find_saved_panel_query(&dashboard.model, &request.panel_id, &request.ref_id)
        .ok_or_else(|| Error::not_found("shared dashboard query not found"))?;
    let variables = validated_variables(&dashboard.model, &share.constraints, &request.variables)?;
    let config = query
        .get("query")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::invalid("saved dashboard query is invalid"))?;
    let source_type = query
        .get("dataSourceType")
        .and_then(Value::as_str)
        .unwrap_or("sql");
    if source_type == "profiles" {
        return Err(Error::forbidden(
            "profiles are not available through public dashboard shares",
        ));
    }
    let expression = ["expression", "statement", "sql", "query"]
        .into_iter()
        .find_map(|key| config.get(key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::invalid("saved dashboard query has no expression"))?;
    let statement = interpolate_saved_expression(expression, &variables)?;
    let language = if config.get("language").and_then(Value::as_str) == Some("sql")
        || source_type != "metrics"
    {
        QueryLanguage::Sql
    } else {
        QueryLanguage::Promql
    };
    let max_range_secs = constraint_i64(
        &share.constraints,
        "max_time_range_secs",
        DEFAULT_DASHBOARD_RANGE_SECS,
    )
    .clamp(60, MAX_DASHBOARD_RANGE_SECS);
    let now = TimestampMicros::now().0;
    let end = request.to_micros.min(now);
    let start = request
        .from_micros
        .max(end.saturating_sub(max_range_secs.saturating_mul(1_000_000)));
    if start >= end {
        return Err(Error::invalid("invalid public dashboard time range"));
    }
    let stream_name = ["streamName", "stream"]
        .into_iter()
        .find_map(|key| config.get(key).and_then(Value::as_str))
        .map(|value| interpolate_saved_expression(value, &variables))
        .transpose()?
        .filter(|value| !value.trim().is_empty());
    let stream_type = config
        .get("streamType")
        .and_then(Value::as_str)
        .and_then(parse_stream_type)
        .or_else(|| parse_stream_type(source_type));
    let stream = match (stream_name, stream_type) {
        (Some(name), Some(stream_type)) => Some(StreamHint { name, stream_type }),
        _ => None,
    };
    let limit = config
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(1000)
        .clamp(1, 1000) as usize;
    let result = state
        .query
        .run_tracked(
            QueryRequest {
                org_id: share.organization_id.clone(),
                language,
                statement,
                time_range: TimeRange::new(TimestampMicros(start), TimestampMicros(end)),
                stream,
                limit: Some(limit),
                federation_clusters: Vec::new(),
            },
            Id::from_string(format!("share:{}", share.id.0)),
            "viewer",
        )
        .await?;
    record_audit(
        &state,
        &share,
        "resource_share.query",
        "share_session",
        &share.id.0,
        Some(&headers),
        json!({
            "panel_id": request.panel_id,
            "ref_id": request.ref_id,
            "from_micros": start,
            "to_micros": end,
            "variables": variables.keys().collect::<Vec<_>>(),
            "returned_rows": result.rows.len(),
            "scanned_rows": result.scanned_rows,
        }),
    )
    .await?;
    Ok(Json(result))
}

#[derive(Debug, Deserialize)]
struct PublicFileQuery {
    #[serde(default)]
    download: bool,
}

async fn public_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<PublicFileQuery>,
) -> Result<Response> {
    let (_session, share) = public_share_context(&state, &headers, true).await?;
    if !matches!(share.resource_type.as_str(), "report" | "report_file") {
        return Err(Error::invalid("shared resource is not a report"));
    }
    if query.download && !share.allow_download {
        return Err(Error::forbidden("download is disabled for this share"));
    }
    let object_key = share
        .snapshot_object_key
        .as_deref()
        .ok_or_else(|| Error::unavailable("report snapshot is not available"))?;
    let object = state
        .storage
        .object_store
        .get(&ObjectPath::from(object_key))
        .await
        .map_err(|error| Error::internal(format!("read report snapshot: {error}")))?;
    let bytes = object
        .bytes()
        .await
        .map_err(|error| Error::internal(format!("read report snapshot bytes: {error}")))?;
    let content_type = share
        .snapshot_content_type
        .as_deref()
        .unwrap_or("application/octet-stream");
    let filename = share.snapshot_filename.as_deref().unwrap_or("report.bin");
    let disposition = if query.download {
        format!("attachment; filename=\"{}\"", header_filename(filename))
    } else {
        format!("inline; filename=\"{}\"", header_filename(filename))
    };
    record_audit(
        &state,
        &share,
        if query.download {
            "resource_share.download"
        } else {
            "resource_share.preview"
        },
        "share_session",
        &share.id.0,
        Some(&headers),
        json!({"download": query.download, "bytes": bytes.len()}),
    )
    .await?;
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, content_type)
        .header(CONTENT_DISPOSITION, disposition)
        .header(CACHE_CONTROL, "private, no-store")
        .header(REFERRER_POLICY, "no-referrer")
        .body(Body::from(bytes))
        .map_err(|error| Error::internal(format!("public report response: {error}")))
}

fn expiration_for(
    request: &CreateRequest,
    policy: &ResourceSharePolicy,
    now: TimestampMicros,
) -> Result<Option<TimestampMicros>> {
    if request.share_mode == ResourceShareMode::PublicLink {
        let seconds = request
            .expires_in_secs
            .unwrap_or(policy.max_public_expiry_secs);
        if seconds < 3600 || seconds > policy.max_public_expiry_secs {
            return Err(Error::invalid(format!(
                "public link expiry must be between 3600 and {} seconds",
                policy.max_public_expiry_secs
            )));
        }
        return Ok(Some(TimestampMicros(
            now.0.saturating_add(seconds.saturating_mul(1_000_000)),
        )));
    }
    match request.expires_in_secs {
        Some(seconds) if seconds <= 0 => {
            Err(Error::invalid("expires_in_secs must be greater than zero"))
        }
        Some(seconds) => Ok(Some(TimestampMicros(
            now.0.saturating_add(seconds.saturating_mul(1_000_000)),
        ))),
        None => Ok(None),
    }
}

fn validate_public_share_password(
    resource_type: &str,
    require_public_report_password: bool,
    password: Option<&str>,
) -> Result<()> {
    if resource_type == "dashboard" && password.is_none() {
        return Err(Error::invalid("public dashboard shares require a password"));
    }
    if resource_type == "report" && require_public_report_password && password.is_none() {
        return Err(Error::invalid(
            "workspace policy requires a password for public reports",
        ));
    }
    Ok(())
}

async fn validate_public_policy(
    policy: &ResourceSharePolicy,
    request: &CreateRequest,
    report: Option<&ScheduledReport>,
    _resource_title: &str,
    state: &AppState,
    resource_id: &Id,
) -> Result<()> {
    if !policy.allow_public_links {
        return Err(Error::forbidden(
            "public links are disabled by workspace policy",
        ));
    }
    if request.resource_type == "dashboard" {
        if !policy.allow_public_dashboards {
            return Err(Error::forbidden(
                "public dashboard links are disabled by workspace policy",
            ));
        }
        if policy.deny_production_public_shares {
            let dashboard = state.dashboard.get(resource_id).await?;
            if dashboard
                .tags
                .iter()
                .any(|tag| matches!(tag.to_ascii_lowercase().as_str(), "prod" | "production"))
            {
                return Err(Error::forbidden(
                    "production dashboards cannot be shared publicly",
                ));
            }
        }
    }
    if let Some(report) = report
        && request.allow_download
        && report.format == "csv"
        && !policy.allow_public_csv_download
    {
        return Err(Error::forbidden(
            "public CSV download is disabled by workspace policy",
        ));
    }
    Ok(())
}

async fn normalize_public_constraints(
    resource_type: &str,
    requested: &Value,
    state: &AppState,
    resource_id: &Id,
) -> Result<Value> {
    if resource_type != "dashboard" {
        return Ok(json!({
            "immutable_snapshot": true,
            "watermark": requested.get("watermark").and_then(Value::as_bool).unwrap_or(true),
        }));
    }
    let dashboard = state.dashboard.get(resource_id).await?;
    let known_variables = dashboard_variables(&dashboard.model)
        .into_iter()
        .filter_map(|variable| variable.get("name").and_then(Value::as_str))
        .collect::<BTreeSet<_>>();
    let allowed_variables = requested
        .get("allowed_variables")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|name| known_variables.contains(name))
        .take(32)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let max_range = requested
        .get("max_time_range_secs")
        .and_then(Value::as_i64)
        .unwrap_or(DEFAULT_DASHBOARD_RANGE_SECS)
        .clamp(60, MAX_DASHBOARD_RANGE_SECS);
    Ok(json!({
        "read_only": true,
        "max_time_range_secs": max_range,
        "allow_time_range_changes": requested
            .get("allow_time_range_changes")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        "allowed_variables": allowed_variables,
        "allow_variable_changes": requested
            .get("allow_variable_changes")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        "auto_refresh_interval_secs": requested
            .get("auto_refresh_interval_secs")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .clamp(0, 3600),
        "allow_export": false,
        "allow_explore": false,
        "show_queries": false,
        "watermark": requested.get("watermark").and_then(Value::as_bool).unwrap_or(true),
    }))
}

async fn create_cross_org_grant(
    state: &AppState,
    context: &IamContext,
    request: &CreateRequest,
    resource_id: &Id,
    expires_at: Option<TimestampMicros>,
) -> Result<IamCrossOrgGrant> {
    let target_organization_id = Id::from_string(
        request
            .target_organization_id
            .as_deref()
            .ok_or_else(|| Error::invalid("target_organization_id is required"))?,
    );
    if target_organization_id == context.org_id {
        return Err(Error::invalid(
            "cross-org share target must be another organization",
        ));
    }
    state.iam.service.orgs.get(&target_organization_id).await?;
    let grantee_type = request
        .grantee_type
        .unwrap_or(IamPrincipalType::Organization);
    let grantee_id = Id::from_string(
        request
            .grantee_id
            .clone()
            .unwrap_or_else(|| target_organization_id.0.clone()),
    );
    match grantee_type {
        IamPrincipalType::Organization if grantee_id == target_organization_id => {}
        IamPrincipalType::Team => {
            let team = state.iam.teams.get(&grantee_id).await?;
            if team.org_id != target_organization_id {
                return Err(Error::invalid(
                    "grantee team must belong to the target organization",
                ));
            }
        }
        IamPrincipalType::User => {
            let found = state
                .iam
                .service
                .iam_memberships
                .list_for_org(&target_organization_id)
                .await?
                .iter()
                .any(|membership| membership.user_id == grantee_id);
            if !found {
                return Err(Error::invalid(
                    "grantee user must belong to the target organization",
                ));
            }
        }
        _ => {
            return Err(Error::invalid(
                "cross-org sharing supports organization, team, or user grantees",
            ));
        }
    }
    let grant = IamCrossOrgGrant {
        id: Id::new(),
        source_organization_id: context.org_id.clone(),
        target_organization_id,
        grantee_type,
        grantee_id,
        resource_type: request.resource_type.clone(),
        resource_selector: json!({"ids": [resource_id.0]}),
        permissions: vec![read_permission(&request.resource_type)?.to_string()],
        conditions: json!({}),
        starts_at: None,
        expires_at,
        status: IamCrossOrgGrantStatus::Pending,
        approved_by: None,
        approved_at: None,
        revoked_by: None,
        revoked_at: None,
        created_by: context.user_id.clone(),
        created_at: TimestampMicros::now(),
    };
    let (grant, _) = state
        .iam
        .access
        .repository()
        .create_cross_org_grant(grant)
        .await?;
    Ok(grant)
}

struct ReportSnapshot {
    object_key: String,
    content_type: String,
    filename: String,
}

async fn render_report_snapshot(
    state: &AppState,
    context: &IamContext,
    share_id: &Id,
    report: &ScheduledReport,
) -> Result<ReportSnapshot> {
    let body = if let Ok(format) = report.format.parse::<ReportFormat>() {
        let renderer = state.platform.report_renderer.as_ref().ok_or_else(|| {
            Error::unavailable("report PDF/PNG renderer is unavailable; verify the Chrome runtime")
        })?;
        let url = super::reports::scheduled::report_render_url(
            &state.platform.report_renderer_base_url,
            report,
        )?;
        let browser_auth_storage = super::reports::scheduled::browser_auth_storage(state, context)?;
        let bytes = renderer
            .render(
                &url,
                format,
                Viewport::default(),
                Some(&browser_auth_storage),
            )
            .await?;
        validate_report_bytes(format, &bytes)?;
        bytes.to_vec()
    } else {
        crate::infra::reporting::render_payload(report)?
    };
    let extension = report.format.to_ascii_lowercase();
    let object_key = format!(
        "resource-shares/{}/{}/report.{}",
        context.org_id.0, share_id.0, extension
    );
    state
        .storage
        .object_store
        .put(
            &ObjectPath::from(object_key.clone()),
            PutPayload::from(body),
        )
        .await
        .map_err(|error| Error::internal(format!("store report snapshot: {error}")))?;
    Ok(ReportSnapshot {
        object_key,
        content_type: crate::infra::reporting::content_type_for(&report.format).to_string(),
        filename: format!("{}.{}", safe_filename(&report.name), extension),
    })
}

async fn public_share_context(
    state: &AppState,
    headers: &HeaderMap,
    require_password: bool,
) -> Result<(ResourceShareSession, ResourceShare)> {
    let raw_session = cookie_value(headers, SHARE_SESSION_COOKIE)
        .ok_or_else(|| Error::unauthorized("missing share session"))?;
    let now = TimestampMicros::now();
    let session = state
        .storage
        .resource_shares
        .find_session(&token_hash(raw_session), now)
        .await?
        .ok_or_else(|| Error::unauthorized("share session expired"))?;
    let share = state
        .storage
        .resource_shares
        .get_by_id(&session.share_id)
        .await?;
    state
        .iam
        .service
        .ensure_organization_access(&share.organization_id)
        .await?;
    if share.share_mode != ResourceShareMode::PublicLink || !share_is_active(&share, now) {
        return Err(Error::unauthorized("resource share unavailable"));
    }
    if require_password && share.password_hash.is_some() && !session.password_verified {
        return Err(Error::unauthorized("share password required"));
    }
    Ok((session, share))
}

fn sanitize_dashboard_model(mut model: Value, share_id: &Id, constraints: &Value) -> Value {
    let Some(object) = model.as_object_mut() else {
        return model;
    };
    object.insert("id".into(), Value::String(String::new()));
    object.insert(
        "uid".into(),
        Value::String(format!("public-{}", share_id.0)),
    );
    object.insert("editable".into(), Value::Bool(false));
    object.insert("defaultDashboard".into(), Value::Bool(false));
    object.insert("annotations".into(), Value::Array(Vec::new()));
    object.insert("links".into(), Value::Array(Vec::new()));
    object.insert(
        "refreshSettings".into(),
        json!({
            "enabled": false,
            "mode": "off",
            "defaultInterval": "off",
            "allowedIntervals": ["off"],
        }),
    );
    let allowed_variables = constraint_strings(constraints, "allowed_variables");
    let uses_current_schema = object.get("elements").is_some_and(Value::is_array);
    if let Some(variables) = object.get_mut("variables").and_then(Value::as_array_mut) {
        variables.retain(|variable| {
            variable
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| allowed_variables.contains(name))
        });
        for variable in variables {
            if let Some(variable) = variable.as_object_mut() {
                variable.remove("query");
                variable.insert("refresh".into(), Value::String("never".into()));
            }
        }
    }
    if uses_current_schema {
        if let Some(elements) = object.get_mut("elements").and_then(Value::as_array_mut) {
            sanitize_elements(elements, &allowed_variables);
        }
    } else {
        sanitize_legacy_variables(object, &allowed_variables);
        if let Some(panels) = object.get_mut("panels").and_then(Value::as_array_mut) {
            sanitize_legacy_panels(panels);
        }
    }
    object.remove("createdBy");
    object.remove("updatedBy");
    model
}

fn sanitize_elements(elements: &mut [Value], allowed_variables: &BTreeSet<String>) {
    for element in elements {
        let Some(object) = element.as_object_mut() else {
            continue;
        };
        match object.get("kind").and_then(Value::as_str) {
            Some("panel") => {
                object.insert("links".into(), Value::Array(Vec::new()));
                if let Some(repeat) = object
                    .get("repeat")
                    .and_then(|value| value.get("variable"))
                    .and_then(Value::as_str)
                    && !allowed_variables.contains(repeat)
                {
                    object.remove("repeat");
                }
                if let Some(queries) = object.get_mut("queries").and_then(Value::as_array_mut) {
                    for query in queries {
                        if let Some(query) = query.as_object_mut() {
                            query.remove("dataSourceId");
                            query.insert("query".into(), json!({}));
                        }
                    }
                }
            }
            Some("group" | "row") => {
                if let Some(children) = object.get_mut("elements").and_then(Value::as_array_mut) {
                    sanitize_elements(children, allowed_variables);
                }
            }
            Some("tab") => {
                if let Some(tabs) = object.get_mut("tabs").and_then(Value::as_array_mut) {
                    for tab in tabs {
                        if let Some(children) =
                            tab.get_mut("elements").and_then(Value::as_array_mut)
                        {
                            sanitize_elements(children, allowed_variables);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn sanitize_legacy_variables(
    dashboard: &mut Map<String, Value>,
    allowed_variables: &BTreeSet<String>,
) {
    let Some(variables) = dashboard
        .get_mut("templating")
        .and_then(Value::as_object_mut)
        .and_then(|templating| templating.get_mut("list"))
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    variables.retain(|variable| {
        variable
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| allowed_variables.contains(name))
    });
    for variable in variables {
        let Some(variable) = variable.as_object_mut() else {
            continue;
        };
        for key in ["query", "datasource", "definition", "regex", "allValue"] {
            variable.remove(key);
        }
        variable.insert("refresh".into(), Value::String("never".into()));
    }
}

fn sanitize_legacy_panels(panels: &mut [Value]) {
    for panel in panels {
        let Some(panel) = panel.as_object_mut() else {
            continue;
        };
        let panel_context = panel.clone();
        panel.insert("links".into(), Value::Array(Vec::new()));
        panel.remove("datasource");
        if let Some(targets) = panel.get_mut("targets").and_then(Value::as_array_mut) {
            *targets = targets
                .iter()
                .enumerate()
                .filter_map(|(index, target)| sanitize_legacy_target(target, &panel_context, index))
                .collect();
        }
        if let Some(children) = panel.get_mut("panels").and_then(Value::as_array_mut) {
            sanitize_legacy_panels(children);
        }
    }
}

fn sanitize_legacy_target(
    target: &Value,
    panel: &Map<String, Value>,
    index: usize,
) -> Option<Value> {
    let target = target.as_object()?;
    let statement = legacy_statement(target)?;
    let language = legacy_language(target, statement);
    let source_type = legacy_source_type(target, panel, language, statement);
    let mut sanitized = Map::from_iter([
        ("refId".into(), Value::String(legacy_ref_id(target, index))),
        (
            "hide".into(),
            Value::Bool(target.get("hide").and_then(Value::as_bool).unwrap_or(false)),
        ),
        ("language".into(), Value::String(language.into())),
        ("stream_type".into(), Value::String(source_type.into())),
    ]);
    if language == "promql" {
        sanitized.insert(
            "expr".into(),
            Value::String("__molesignal_public_query__".into()),
        );
    } else {
        sanitized.insert("rawSql".into(), Value::String("SELECT 1".into()));
    }
    for key in ["legendFormat", "format"] {
        if let Some(value) = target.get(key).and_then(Value::as_str) {
            sanitized.insert(key.into(), Value::String(value.into()));
        }
    }
    Some(Value::Object(sanitized))
}

fn find_saved_panel_query(model: &Value, panel_id: &str, ref_id: &str) -> Option<Value> {
    if let Some(elements) = model.get("elements").and_then(Value::as_array) {
        let query = find_query_in_elements(elements, panel_id, ref_id)?;
        let shared = query.get("sharedQuery").and_then(Value::as_object);
        return match shared {
            Some(shared) => {
                let source_panel = shared.get("sourcePanelId")?.as_str()?;
                let source_ref = shared.get("sourceRefId")?.as_str()?;
                find_query_in_elements(elements, source_panel, source_ref).or(Some(query))
            }
            None => Some(query),
        };
    }
    model
        .get("panels")
        .and_then(Value::as_array)
        .and_then(|panels| find_legacy_query_in_panels(panels, panel_id, ref_id))
}

fn find_legacy_query_in_panels(panels: &[Value], panel_id: &str, ref_id: &str) -> Option<Value> {
    for (panel_index, panel) in panels.iter().enumerate() {
        let panel = panel.as_object()?;
        if legacy_panel_id(panel, panel_index) == panel_id
            && let Some((target_index, target)) = panel
                .get("targets")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
                .find(|(index, target)| {
                    target
                        .as_object()
                        .is_some_and(|target| legacy_ref_id(target, *index) == ref_id)
                })
        {
            return normalize_legacy_query(target, panel, target_index);
        }
        if let Some(found) = panel
            .get("panels")
            .and_then(Value::as_array)
            .and_then(|children| find_legacy_query_in_panels(children, panel_id, ref_id))
        {
            return Some(found);
        }
    }
    None
}

fn normalize_legacy_query(
    target: &Value,
    panel: &Map<String, Value>,
    index: usize,
) -> Option<Value> {
    let target = target.as_object()?;
    let statement = legacy_statement(target)?;
    let language = legacy_language(target, statement);
    let source_type = legacy_source_type(target, panel, language, statement);
    let mut config = Map::from_iter([("language".into(), Value::String(language.to_string()))]);
    config.insert(
        if language == "promql" {
            "expression".into()
        } else {
            "statement".into()
        },
        Value::String(statement.to_string()),
    );
    if let Some(stream_name) = legacy_stream_name(target) {
        config.insert("streamName".into(), Value::String(stream_name.into()));
        let stream_type = legacy_stream_type(target).unwrap_or(source_type);
        config.insert("streamType".into(), Value::String(stream_type.into()));
    }
    if let Some(limit) = target.get("limit").and_then(Value::as_u64) {
        config.insert("limit".into(), Value::from(limit));
    }
    Some(json!({
        "refId": legacy_ref_id(target, index),
        "enabled": !target.get("hide").and_then(Value::as_bool).unwrap_or(false),
        "dataSourceType": source_type,
        "query": Value::Object(config),
    }))
}

fn legacy_statement(target: &Map<String, Value>) -> Option<&str> {
    ["expr", "rawSql", "query"]
        .into_iter()
        .find_map(|key| target.get(key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn legacy_language(target: &Map<String, Value>, statement: &str) -> &'static str {
    let configured = target
        .get("language")
        .and_then(Value::as_str)
        .or_else(|| {
            target
                .get("datasource")
                .and_then(Value::as_object)
                .and_then(|datasource| datasource.get("type"))
                .and_then(Value::as_str)
        })
        .unwrap_or_default()
        .to_ascii_lowercase();
    if configured.contains("prom") || configured == "metrics" {
        return "promql";
    }
    if configured.contains("sql")
        || configured.contains("postgres")
        || configured.contains("mysql")
        || configured.contains("datafusion")
    {
        return "sql";
    }
    if statement.to_ascii_lowercase().contains("select") {
        "sql"
    } else {
        "promql"
    }
}

fn legacy_source_type(
    target: &Map<String, Value>,
    panel: &Map<String, Value>,
    language: &str,
    statement: &str,
) -> &'static str {
    let configured = [
        target
            .get("datasource")
            .and_then(Value::as_object)
            .and_then(|datasource| datasource.get("type"))
            .and_then(Value::as_str),
        target.get("stream_type").and_then(Value::as_str),
        target.get("streamType").and_then(Value::as_str),
        panel
            .get("datasource")
            .and_then(Value::as_object)
            .and_then(|datasource| datasource.get("type"))
            .and_then(Value::as_str),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ")
    .to_ascii_lowercase();
    if configured.contains("trace") || configured.contains("tempo") {
        return "traces";
    }
    if configured.contains("profile") {
        return "profiles";
    }
    if configured.contains("log")
        || configured.contains("loki")
        || configured.contains("elasticsearch")
    {
        return "logs";
    }
    if configured.contains("metric") || configured.contains("prom") {
        return "metrics";
    }
    if language == "promql" {
        return "metrics";
    }
    let hint = format!(
        "{} {statement}",
        panel
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
    )
    .to_ascii_lowercase();
    if hint.contains("trace") {
        "traces"
    } else if hint.contains("profile") {
        "profiles"
    } else if hint.contains("log") {
        "logs"
    } else {
        "sql"
    }
}

fn legacy_stream_name(target: &Map<String, Value>) -> Option<&str> {
    target
        .get("stream_name")
        .and_then(Value::as_str)
        .or_else(|| {
            target
                .get("stream")
                .and_then(Value::as_object)
                .and_then(|stream| stream.get("name"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn legacy_stream_type(target: &Map<String, Value>) -> Option<&str> {
    target
        .get("stream_type")
        .and_then(Value::as_str)
        .or_else(|| target.get("streamType").and_then(Value::as_str))
        .or_else(|| {
            target
                .get("stream")
                .and_then(Value::as_object)
                .and_then(|stream| stream.get("stream_type"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn legacy_ref_id(target: &Map<String, Value>, index: usize) -> String {
    target
        .get("refId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            if index < 26 {
                ((b'A' + index as u8) as char).to_string()
            } else {
                format!("Q{}", index + 1)
            }
        })
}

fn legacy_panel_id(panel: &Map<String, Value>, index: usize) -> String {
    let fallback = (index + 1).to_string();
    let raw = match panel.get("id") {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        Some(Value::Bool(value)) => value.to_string(),
        _ => fallback.clone(),
    };
    let safe = raw
        .trim()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    let safe = if safe.is_empty() { fallback } else { safe };
    format!("legacy-{safe}-{}", index + 1)
}

fn find_query_in_elements(elements: &[Value], panel_id: &str, ref_id: &str) -> Option<Value> {
    for element in elements {
        let object = element.as_object()?;
        match object.get("kind").and_then(Value::as_str) {
            Some("panel") if object.get("id").and_then(Value::as_str) == Some(panel_id) => {
                if let Some(query) = object
                    .get("queries")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .find(|query| query.get("refId").and_then(Value::as_str) == Some(ref_id))
                {
                    return Some(query.clone());
                }
            }
            Some("group" | "row") => {
                if let Some(found) = object
                    .get("elements")
                    .and_then(Value::as_array)
                    .and_then(|children| find_query_in_elements(children, panel_id, ref_id))
                {
                    return Some(found);
                }
            }
            Some("tab") => {
                for tab in object
                    .get("tabs")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(found) = tab
                        .get("elements")
                        .and_then(Value::as_array)
                        .and_then(|children| find_query_in_elements(children, panel_id, ref_id))
                    {
                        return Some(found);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn validated_variables(
    model: &Value,
    constraints: &Value,
    requested: &BTreeMap<String, Value>,
) -> Result<BTreeMap<String, Value>> {
    let allowed_names = constraint_strings(constraints, "allowed_variables");
    let allow_changes = constraints
        .get("allow_variable_changes")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut values = BTreeMap::new();
    for variable in dashboard_variables(model) {
        let Some(name) = variable.get("name").and_then(Value::as_str) else {
            continue;
        };
        if !allowed_names.contains(name) {
            continue;
        }
        let default =
            variable_default_value(variable).unwrap_or_else(|| Value::String(String::new()));
        let candidate = if allow_changes {
            requested.get(name).cloned().unwrap_or(default)
        } else {
            default
        };
        if !variable_value_allowed(variable, &candidate) {
            return Err(Error::forbidden(format!(
                "variable {name} is outside the shared allowlist"
            )));
        }
        values.insert(name.to_string(), candidate);
    }
    if requested.keys().any(|name| !allowed_names.contains(name)) {
        return Err(Error::forbidden(
            "one or more dashboard variables are not shared",
        ));
    }
    Ok(values)
}

fn dashboard_variables(model: &Value) -> Vec<&Value> {
    if let Some(variables) = model.get("variables").and_then(Value::as_array) {
        return variables.iter().collect();
    }
    model
        .get("templating")
        .and_then(Value::as_object)
        .and_then(|templating| templating.get("list"))
        .and_then(Value::as_array)
        .map(|variables| variables.iter().collect())
        .unwrap_or_default()
}

fn variable_default_value(variable: &Value) -> Option<Value> {
    variable
        .get("currentValue")
        .or_else(|| variable.get("defaultValue"))
        .or_else(|| {
            variable
                .get("current")
                .and_then(Value::as_object)
                .and_then(|current| current.get("value").or_else(|| current.get("text")))
        })
        .or_else(|| {
            variable
                .get("options")
                .and_then(Value::as_array)
                .and_then(|options| options.first())
                .and_then(variable_option_value)
        })
        .cloned()
}

fn variable_option_value(option: &Value) -> Option<&Value> {
    option
        .get("value")
        .or_else(|| (!option.is_object()).then_some(option))
}

fn variable_value_allowed(variable: &Value, candidate: &Value) -> bool {
    let options = variable
        .get("options")
        .and_then(Value::as_array)
        .map(|options| {
            options
                .iter()
                .filter_map(variable_option_value)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !options.is_empty() {
        return match candidate {
            Value::Array(values) => values.iter().all(|value| options.contains(&value)),
            value => options.contains(&value),
        };
    }
    variable_default_value(variable)
        .as_ref()
        .is_some_and(|default| default == candidate)
        || (variable_default_value(variable).is_none() && candidate == "")
}

fn interpolate_saved_expression(
    expression: &str,
    variables: &BTreeMap<String, Value>,
) -> Result<String> {
    let regex =
        Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)(?::([A-Za-z]+))?\}|\$([A-Za-z_][A-Za-z0-9_]*)")
            .map_err(|error| Error::internal(format!("compile variable expression: {error}")))?;
    Ok(regex
        .replace_all(expression, |captures: &regex::Captures<'_>| {
            let name = captures
                .get(1)
                .or_else(|| captures.get(3))
                .map(|value| value.as_str())
                .unwrap_or_default();
            let Some(value) = variables.get(name) else {
                return captures
                    .get(0)
                    .map(|value| value.as_str())
                    .unwrap_or_default()
                    .to_string();
            };
            format_variable_value(
                value,
                captures.get(2).map(|value| value.as_str()).unwrap_or("raw"),
            )
        })
        .into_owned())
}

fn format_variable_value(value: &Value, format: &str) -> String {
    let values = value
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_else(|| std::slice::from_ref(value));
    match format {
        "json" => value.to_string(),
        "sqlstring" => values
            .iter()
            .map(|value| format!("'{}'", scalar_string(value).replace('\'', "''")))
            .collect::<Vec<_>>()
            .join(","),
        "regex" => values
            .iter()
            .map(|value| regex::escape(&scalar_string(value)))
            .collect::<Vec<_>>()
            .join("|"),
        "pipe" => values
            .iter()
            .map(scalar_string)
            .collect::<Vec<_>>()
            .join("|"),
        _ => values
            .iter()
            .map(scalar_string)
            .collect::<Vec<_>>()
            .join(","),
    }
}

fn scalar_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        other => other.to_string(),
    }
}

fn authenticated_target(share: &ResourceShare) -> Result<String> {
    match share.resource_type.as_str() {
        "dashboard" => Ok(format!("/dashboards/{}", share.resource_id.0)),
        "report" | "report_file" => Ok(format!("/reports?report={}", share.resource_id.0)),
        _ => Err(Error::invalid("unsupported shared resource type")),
    }
}

fn share_is_active(share: &ResourceShare, now: TimestampMicros) -> bool {
    share.enabled
        && share.revoked_at.is_none()
        && share.expires_at.is_none_or(|expires_at| expires_at > now)
}

fn read_permission(resource_type: &str) -> Result<&'static str> {
    match resource_type {
        "dashboard" => Ok("dashboards.read"),
        "report" => Ok("reports.read"),
        _ => Err(Error::invalid("resource_type must be dashboard or report")),
    }
}

fn normalized_share_resource_type(resource_type: &str) -> &str {
    if resource_type == "report_file" {
        "report"
    } else {
        resource_type
    }
}

fn share_permission(resource_type: &str) -> Result<&'static str> {
    match resource_type {
        "dashboard" => Ok("dashboards.share"),
        "report" | "report_file" => Ok("reports.share"),
        _ => Err(Error::invalid("unsupported shared resource type")),
    }
}

fn share_permission_for_record(share: &ResourceShare) -> Result<&'static str> {
    share_permission(&share.resource_type)
}

async fn authorize_shareable_filter(
    state: &AppState,
    context: &IamContext,
    resource_type: &str,
    resource_id: &Id,
) -> Result<()> {
    let permission = share_permission(resource_type)?;
    let id = ShareableResourceId::new(resource_type, resource_id.as_str())?;
    authorize_protected_resource::<ShareableResource>(state, context, id, permission).await?;
    Ok(())
}

fn can_manage_share(context: &IamContext, resource_type: &str) -> bool {
    context.has_permission("org.settings.manage")
        || match resource_type {
            "dashboard" => context.has_permission("dashboards.share"),
            "report" | "report_file" => context.has_permission("reports.share"),
            _ => false,
        }
}

fn resource_share_view(share: ResourceShare) -> ResourceShareView {
    let url = if share_is_active(&share, TimestampMicros::now()) {
        share.raw_token.as_ref().map(|token| format!("/s/{token}"))
    } else {
        None
    };
    ResourceShareView { share, url }
}

fn generate_share_token() -> String {
    let mut bytes = [0u8; 9];
    rand::rngs::SysRng
        .try_fill_bytes(&mut bytes)
        .expect("operating-system random source");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn generate_opaque_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::SysRng
        .try_fill_bytes(&mut bytes)
        .expect("operating-system random source");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn token_hash(raw: &str) -> String {
    blake3::hash(raw.as_bytes()).to_hex().to_string()
}

fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
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

fn request_ip(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn header_text(headers: &HeaderMap, name: http::header::HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

async fn record_audit(
    state: &AppState,
    share: &ResourceShare,
    action: &str,
    actor_kind: &str,
    actor_id: &str,
    headers: Option<&HeaderMap>,
    payload: Value,
) -> Result<()> {
    state
        .iam
        .audit_events
        .record(AuditEvent {
            id: Id::new(),
            org_id: share.organization_id.clone(),
            actor_kind: actor_kind.into(),
            actor_id: actor_id.into(),
            action: action.into(),
            target_kind: Some("resource_share".into()),
            target_id: Some(share.id.0.clone()),
            ip: headers.and_then(request_ip),
            user_agent: headers.and_then(|headers| header_text(headers, USER_AGENT)),
            payload,
            ts: TimestampMicros::now(),
        })
        .await
}

fn constraint_strings(value: &Value, key: &str) -> BTreeSet<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn constraint_i64(value: &Value, key: &str, fallback: i64) -> i64 {
    value.get(key).and_then(Value::as_i64).unwrap_or(fallback)
}

fn parse_stream_type(value: &str) -> Option<StreamType> {
    match value {
        "logs" => Some(StreamType::Logs),
        "metrics" => Some(StreamType::Metrics),
        "traces" => Some(StreamType::Traces),
        "profiles" => Some(StreamType::Profiles),
        "extend" => Some(StreamType::Extend),
        _ => None,
    }
}

fn safe_filename(value: &str) -> String {
    let value = value
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    let value = value.trim_matches('-');
    if value.is_empty() {
        "report".into()
    } else {
        value.chars().take(120).collect()
    }
}

fn header_filename(value: &str) -> String {
    value
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_')
        })
        .take(160)
        .collect()
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn share_tokens_are_short_and_url_safe() {
        let first = generate_share_token();
        let second = generate_share_token();
        assert_ne!(first, second);
        assert_eq!(first.len(), 12);
        assert!(first.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
        }));
    }

    #[test]
    fn session_tokens_keep_full_entropy() {
        let first = generate_opaque_token();
        let second = generate_opaque_token();
        assert_ne!(first, second);
        assert_eq!(first.len(), 43);
        assert!(first.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
        }));
    }

    #[test]
    fn public_dashboard_password_is_always_required() {
        assert!(validate_public_share_password("dashboard", false, None).is_err());
        assert!(validate_public_share_password("dashboard", false, Some("secret")).is_ok());
    }

    #[test]
    fn public_report_password_still_follows_workspace_policy() {
        assert!(validate_public_share_password("report", false, None).is_ok());
        assert!(validate_public_share_password("report", true, None).is_err());
        assert!(validate_public_share_password("report", true, Some("secret")).is_ok());
    }

    #[test]
    fn interpolation_only_uses_server_validated_values() {
        let variables = BTreeMap::from([
            ("region".into(), Value::String("cn-north-1".into())),
            ("service".into(), Value::String("api'edge".into())),
        ]);
        assert_eq!(
            interpolate_saved_expression(
                "region=~\"${region:regex}\" and service in (${service:sqlstring})",
                &variables,
            )
            .unwrap(),
            "region=~\"cn\\-north\\-1\" and service in ('api''edge')"
        );
    }

    #[test]
    fn expired_or_revoked_share_is_unavailable() {
        let now = TimestampMicros(100);
        let mut share = test_share();
        share.expires_at = Some(TimestampMicros(99));
        assert!(!share_is_active(&share, now));
    }

    #[test]
    fn share_view_exposes_url_without_token_material() {
        let mut share = test_share();
        share.expires_at = None;
        share.raw_token = Some("Ab3kP9xY2mQ7".into());
        let value = serde_json::to_value(resource_share_view(share)).unwrap();
        assert_eq!(value["url"], "/s/Ab3kP9xY2mQ7");
        assert!(value.get("raw_token").is_none());
        assert!(value.get("token_hash").is_none());
    }

    #[test]
    fn disabled_share_view_hides_url() {
        let mut share = test_share();
        share.expires_at = None;
        share.enabled = false;
        share.raw_token = Some("Ab3kP9xY2mQ7".into());
        let value = serde_json::to_value(resource_share_view(share)).unwrap();
        assert!(value["url"].is_null());
    }

    #[test]
    fn legacy_public_dashboard_is_scrubbed_and_keeps_server_query_lookup() {
        let model = json!({
            "title": "Legacy dashboard",
            "templating": {
                "list": [{
                    "name": "service",
                    "query": "label_values(http_requests_total, service)",
                    "current": {"value": "checkout"}
                }]
            },
            "panels": [{
                "id": 4,
                "title": "Request rate",
                "type": "graph",
                "targets": [{
                    "refId": "A",
                    "expr": "rate(http_requests_total{service=\"$service\"}[5m])",
                    "datasource": {"type": "prometheus"}
                }]
            }]
        });

        let query = find_saved_panel_query(&model, "legacy-4-1", "A").unwrap();
        assert_eq!(query["dataSourceType"], "metrics");
        assert_eq!(
            query["query"]["expression"],
            "rate(http_requests_total{service=\"$service\"}[5m])"
        );

        let sanitized = sanitize_dashboard_model(
            model,
            &Id::from_string("share-1"),
            &json!({"allowed_variables": ["service"]}),
        );
        assert_eq!(
            sanitized["panels"][0]["targets"][0]["expr"],
            "__molesignal_public_query__"
        );
        assert_eq!(
            sanitized["templating"]["list"][0]["current"]["value"],
            "checkout"
        );
        assert!(sanitized["templating"]["list"][0].get("query").is_none());
        let serialized = sanitized.to_string();
        assert!(!serialized.contains("rate(http_requests_total"));
        assert!(!serialized.contains("label_values"));
    }

    fn test_share() -> ResourceShare {
        ResourceShare {
            id: Id::from_string("share"),
            organization_id: Id::from_string("org"),
            resource_type: "dashboard".into(),
            resource_id: Id::from_string("dashboard"),
            resource_version_id: None,
            share_mode: ResourceShareMode::PublicLink,
            token_hash: "hash".into(),
            raw_token: None,
            permissions: json!([]),
            constraints: json!({}),
            expires_at: Some(TimestampMicros(99)),
            password_hash: None,
            max_views: None,
            view_count: 0,
            allow_download: false,
            enabled: true,
            cross_org_grant_id: None,
            snapshot_object_key: None,
            snapshot_content_type: None,
            snapshot_filename: None,
            created_by: Id::from_string("user"),
            created_at: TimestampMicros(1),
            last_accessed_at: None,
            revoked_at: None,
        }
    }
}
