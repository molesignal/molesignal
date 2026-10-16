// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The business service a status-page component stands for.

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::put,
};
use serde::Deserialize;

use crate::{
    api::{AppState, http::routes::activity_audit},
    app::{iam::IamContext, status_page::StatusPageServiceLinkInput},
    domain::{
        iam::permission,
        status_page::{ServiceLinkMode, StatusPageComponent},
    },
    shared::{Result, ids::Id},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new().route(
        "/status-pages/{page_id}/components/{component_id}/business-service",
        put(link).delete(unlink),
    )
}

#[derive(Debug, Deserialize)]
struct LinkRequest {
    business_service_id: Id,
    mode: ServiceLinkMode,
    /// Whether a status the component follows tells subscribers.
    #[serde(default = "notify_by_default")]
    notify: bool,
}

#[permission("status_pages.manage")]
async fn link(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((page_id, component_id)): Path<(String, String)>,
    Json(request): Json<LinkRequest>,
) -> Result<Json<StatusPageComponent>> {
    let component = state
        .status_pages
        .set_component_service_link(
            &context.org_id,
            &Id(page_id),
            &Id(component_id),
            Some(StatusPageServiceLinkInput {
                business_service_id: request.business_service_id,
                mode: request.mode,
                notify_subscribers: request.notify,
            }),
        )
        .await?;

    record_link_audit(&state, &context, &component, "link").await;
    Ok(Json(component))
}

#[permission("status_pages.manage")]
async fn unlink(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((page_id, component_id)): Path<(String, String)>,
) -> Result<Json<StatusPageComponent>> {
    let component = state
        .status_pages
        .set_component_service_link(&context.org_id, &Id(page_id), &Id(component_id), None)
        .await?;

    record_link_audit(&state, &context, &component, "unlink").await;
    Ok(Json(component))
}

async fn record_link_audit(
    state: &AppState,
    context: &IamContext,
    component: &StatusPageComponent,
    verb: &str,
) {
    activity_audit::record(
        state,
        context,
        &format!("status_page.component.{verb}_business_service"),
        "status_page_component",
        component.id.as_str(),
        serde_json::json!({
            "status_page_id": component.status_page_id,
            "business_service": component.business_service,
        }),
    )
    .await;
}

fn notify_by_default() -> bool {
    true
}
