// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::Deserialize;

use super::logo;
use crate::{
    api::{AppState, http::routes::activity_audit},
    app::{
        iam::IamContext,
        status_page::{StatusPageComponentInput, StatusPageInput},
    },
    domain::{
        iam::permission,
        status_page::{
            ComponentLifecycle, ComponentStatus, ComponentVisibility, StatusPage,
            StatusPageComponent, StatusPageLifecycle, StatusPageSnapshot, StatusPageVisibility,
        },
    },
    shared::{Result, ids::Id},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/status-pages", get(list_pages).post(create_page))
        .route(
            "/status-pages/{page_id}",
            get(get_page).put(update_page).delete(delete_page),
        )
        .route("/status-pages/{page_id}/archive", post(archive_page))
        .route("/status-pages/{page_id}/restore", post(restore_page))
        .route("/status-pages/{page_id}/components", post(create_component))
        .route(
            "/status-pages/{page_id}/components/{component_id}",
            axum::routing::put(update_component).delete(delete_component),
        )
}

#[derive(Debug, Deserialize)]
struct PageListQuery {
    lifecycle: Option<StatusPageLifecycle>,
}

#[derive(Debug, Deserialize)]
struct PageWriteRequest {
    name: String,
    slug: String,
    #[serde(default)]
    logo_url: Option<String>,
    #[serde(default = "default_brand_color")]
    brand_color: String,
    #[serde(default = "default_timezone")]
    timezone: String,
    #[serde(default = "default_language")]
    language: String,
    #[serde(default)]
    languages: Vec<String>,
    #[serde(default = "default_history_days")]
    history_days: i32,
    #[serde(default = "default_delivery_retention_days")]
    delivery_retention_days: i32,
    #[serde(default = "default_private_session_days")]
    private_session_days: i32,
    #[serde(default = "default_visibility")]
    visibility: StatusPageVisibility,
}

impl From<PageWriteRequest> for StatusPageInput {
    fn from(value: PageWriteRequest) -> Self {
        Self {
            name: value.name,
            slug: value.slug,
            logo_url: value.logo_url,
            brand_color: value.brand_color,
            custom_domain: None,
            timezone: value.timezone,
            language: value.language,
            languages: value.languages,
            history_days: value.history_days,
            delivery_retention_days: value.delivery_retention_days,
            private_session_days: value.private_session_days,
            visibility: value.visibility,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ComponentWriteRequest {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default = "default_component_status")]
    status: ComponentStatus,
    #[serde(default = "default_component_visibility")]
    visibility: ComponentVisibility,
    #[serde(default = "default_component_lifecycle")]
    lifecycle: ComponentLifecycle,
    #[serde(default)]
    position: Option<i32>,
}

impl From<ComponentWriteRequest> for StatusPageComponentInput {
    fn from(value: ComponentWriteRequest) -> Self {
        Self {
            name: value.name,
            description: value.description,
            status: value.status,
            visibility: value.visibility,
            lifecycle: value.lifecycle,
            position: value.position,
        }
    }
}

#[permission("status_pages.read")]
async fn list_pages(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Query(query): Query<PageListQuery>,
) -> Result<Json<Vec<StatusPage>>> {
    Ok(Json(
        state
            .status_pages
            .list_pages(&context.org_id, query.lifecycle)
            .await?,
    ))
}

#[permission("status_pages.manage")]
async fn create_page(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Json(request): Json<PageWriteRequest>,
) -> Result<(StatusCode, Json<StatusPage>)> {
    let page = state
        .status_pages
        .create_page(&context.org_id, request.into())
        .await?;
    activity_audit::record(
        &state,
        &context,
        "status_page.create",
        "status_page",
        page.id.as_str(),
        serde_json::json!({"visibility": page.visibility}),
    )
    .await;
    Ok((StatusCode::CREATED, Json(page)))
}

#[permission("status_pages.read")]
async fn get_page(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(page_id): Path<String>,
) -> Result<Json<StatusPageSnapshot>> {
    Ok(Json(
        state
            .status_pages
            .get_snapshot(&context.org_id, &Id(page_id))
            .await?,
    ))
}

#[permission("status_pages.manage")]
async fn update_page(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(page_id): Path<String>,
    Json(request): Json<PageWriteRequest>,
) -> Result<Json<StatusPage>> {
    let page_id = Id(page_id);
    let existing = state
        .status_pages
        .get_page(&context.org_id, &page_id)
        .await?;
    let mut input: StatusPageInput = request.into();
    input.logo_url = logo::rewrite_managed_logo_url(&existing, &input.slug, input.logo_url);
    input.custom_domain = existing.custom_domain.clone();
    let updated = state
        .status_pages
        .update_page(&context.org_id, &page_id, input)
        .await?;
    logo::cleanup_replaced_logo(&state, &existing, Some(&updated)).await;
    activity_audit::record(
        &state,
        &context,
        "status_page.update",
        "status_page",
        page_id.as_str(),
        serde_json::json!({
            "visibility": updated.visibility,
            "history_days": updated.history_days,
            "delivery_retention_days": updated.delivery_retention_days,
            "private_session_days": updated.private_session_days,
        }),
    )
    .await;
    Ok(Json(updated))
}

#[permission("status_pages.manage")]
async fn archive_page(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(page_id): Path<String>,
) -> Result<Json<StatusPage>> {
    let page_id = Id(page_id);
    let page = state
        .status_pages
        .archive_page(&context.org_id, &page_id)
        .await?;
    activity_audit::record(
        &state,
        &context,
        "status_page.archive",
        "status_page",
        page_id.as_str(),
        serde_json::json!({"purge_after": page.purge_after}),
    )
    .await;
    Ok(Json(page))
}

#[permission("status_pages.manage")]
async fn restore_page(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(page_id): Path<String>,
) -> Result<Json<StatusPage>> {
    let page_id = Id(page_id);
    let page = state
        .status_pages
        .restore_page(&context.org_id, &page_id)
        .await?;
    activity_audit::record(
        &state,
        &context,
        "status_page.restore",
        "status_page",
        page_id.as_str(),
        serde_json::json!({}),
    )
    .await;
    Ok(Json(page))
}

#[permission("status_pages.manage")]
async fn delete_page(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(page_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let page_id = Id(page_id);
    let existing = state
        .status_pages
        .get_page(&context.org_id, &page_id)
        .await?;
    state
        .status_pages
        .delete_page(&context.org_id, &page_id)
        .await?;
    logo::cleanup_replaced_logo(&state, &existing, None).await;
    activity_audit::record(
        &state,
        &context,
        "status_page.delete",
        "status_page",
        page_id.as_str(),
        serde_json::json!({"lifecycle": "archived"}),
    )
    .await;
    Ok(Json(serde_json::json!({"deleted": true})))
}

#[permission("status_pages.manage")]
async fn create_component(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(page_id): Path<String>,
    Json(request): Json<ComponentWriteRequest>,
) -> Result<(StatusCode, Json<StatusPageComponent>)> {
    let page_id = Id(page_id);
    let component = state
        .status_pages
        .create_component(&context.org_id, &page_id, request.into())
        .await?;
    record_component_audit(&state, &context, &component, "create").await;
    Ok((StatusCode::CREATED, Json(component)))
}

#[permission("status_pages.manage")]
async fn update_component(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((page_id, component_id)): Path<(String, String)>,
    Json(request): Json<ComponentWriteRequest>,
) -> Result<Json<StatusPageComponent>> {
    let component = state
        .status_pages
        .update_component(
            &context.org_id,
            &Id(page_id),
            &Id(component_id),
            request.into(),
        )
        .await?;
    record_component_audit(&state, &context, &component, "update").await;
    Ok(Json(component))
}

#[permission("status_pages.manage")]
async fn delete_component(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((page_id, component_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    state
        .status_pages
        .delete_component(&context.org_id, &Id(page_id), &Id(component_id.clone()))
        .await?;
    activity_audit::record(
        &state,
        &context,
        "status_page.component.delete",
        "status_page_component",
        &component_id,
        serde_json::json!({}),
    )
    .await;
    Ok(Json(serde_json::json!({"deleted": true})))
}

async fn record_component_audit(
    state: &AppState,
    context: &IamContext,
    component: &StatusPageComponent,
    verb: &str,
) {
    activity_audit::record(
        state,
        context,
        &format!("status_page.component.{verb}"),
        "status_page_component",
        component.id.as_str(),
        serde_json::json!({
            "status_page_id": component.status_page_id,
            "status": component.status,
            "visibility": component.visibility,
            "lifecycle": component.lifecycle,
            "position": component.position,
        }),
    )
    .await;
}

fn default_brand_color() -> String {
    "#4F46E5".into()
}
fn default_timezone() -> String {
    "UTC".into()
}
fn default_language() -> String {
    "en-us".into()
}
fn default_history_days() -> i32 {
    crate::app::status_page::DEFAULT_STATUS_PAGE_HISTORY_DAYS
}
fn default_delivery_retention_days() -> i32 {
    crate::app::status_page::DEFAULT_STATUS_PAGE_DELIVERY_RETENTION_DAYS
}
fn default_private_session_days() -> i32 {
    crate::app::status_page::DEFAULT_STATUS_PAGE_PRIVATE_SESSION_DAYS
}
fn default_visibility() -> StatusPageVisibility {
    StatusPageVisibility::Public
}
fn default_component_status() -> ComponentStatus {
    ComponentStatus::Operational
}
fn default_component_visibility() -> ComponentVisibility {
    ComponentVisibility::Enabled
}
fn default_component_lifecycle() -> ComponentLifecycle {
    ComponentLifecycle::Active
}
