// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    routing::post,
};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    api::{AppState, http::federation::emit_cud},
    app::iam::IamContext,
    domain::{
        dashboard::Dashboard,
        federation::{CudAction, ResourceKind},
        iam::resource_permission,
    },
    shared::{Result, ids::Id},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/dashboards/{id}/panels", post(add))
        .route(
            "/dashboards/{id}/panels/{panel_id}",
            axum::routing::put(update).delete(delete),
        )
        .route("/dashboards/{id}/panels/{panel_id}/move", post(move_panel))
}

#[derive(Debug, Deserialize)]
struct PanelReq {
    panel: Value,
    #[serde(default)]
    container_id: Option<String>,
    #[serde(default)]
    position: Option<usize>,
    #[serde(default)]
    expected_version: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct UpdatePanelReq {
    panel: Value,
    #[serde(default)]
    expected_version: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct DeletePanelReq {
    #[serde(default)]
    expected_version: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct MovePanelReq {
    #[serde(default)]
    container_id: Option<String>,
    position: usize,
    #[serde(default)]
    expected_version: Option<u32>,
}

#[resource_permission(
    action = "dashboards.edit",
    resource = Dashboard,
    id = Id::from_string(id),
    bind = dashboard
)]
async fn add(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<PanelReq>,
) -> Result<Json<Dashboard>> {
    let saved = state
        .dashboard
        .add_panel(
            dashboard,
            context.user_id,
            request.panel,
            request.container_id.as_deref(),
            request.position,
            request.expected_version,
        )
        .await?;
    emit_update(&state, &saved).await;
    Ok(Json(saved))
}

#[resource_permission(
    action = "dashboards.edit",
    resource = Dashboard,
    id = Id::from_string(id),
    bind = dashboard
)]
async fn update(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((id, panel_id)): Path<(String, String)>,
    Json(request): Json<UpdatePanelReq>,
) -> Result<Json<Dashboard>> {
    let saved = state
        .dashboard
        .update_panel(
            dashboard,
            context.user_id,
            &panel_id,
            request.panel,
            request.expected_version,
        )
        .await?;
    emit_update(&state, &saved).await;
    Ok(Json(saved))
}

#[resource_permission(
    action = "dashboards.edit",
    resource = Dashboard,
    id = Id::from_string(id),
    bind = dashboard
)]
async fn delete(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((id, panel_id)): Path<(String, String)>,
    Query(payload): Query<DeletePanelReq>,
) -> Result<Json<Dashboard>> {
    let saved = state
        .dashboard
        .delete_panel(
            dashboard,
            context.user_id,
            &panel_id,
            payload.expected_version,
        )
        .await?;
    emit_update(&state, &saved).await;
    Ok(Json(saved))
}

#[resource_permission(
    action = "dashboards.edit",
    resource = Dashboard,
    id = Id::from_string(id),
    bind = dashboard
)]
async fn move_panel(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((id, panel_id)): Path<(String, String)>,
    Json(request): Json<MovePanelReq>,
) -> Result<Json<Dashboard>> {
    let saved = state
        .dashboard
        .move_panel(
            dashboard,
            context.user_id,
            &panel_id,
            request.container_id.as_deref(),
            request.position,
            request.expected_version,
        )
        .await?;
    emit_update(&state, &saved).await;
    Ok(Json(saved))
}

async fn emit_update(state: &AppState, dashboard: &Dashboard) {
    emit_cud(
        state,
        &dashboard.org_id,
        ResourceKind::Dashboard,
        CudAction::Updated,
        &dashboard.id.0,
        dashboard,
    )
    .await;
}
