// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    api::{
        AppState,
        http::{
            federation::{delete_payload, emit_cud},
            middleware::ProtectedResource,
        },
    },
    app::iam::IamContext,
    domain::{
        dashboard::Dashboard,
        federation::{CudAction, ResourceKind},
        iam::{permission, resource_permission},
    },
    shared::{Result, ids::Id},
};

mod variables;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/dashboards", get(list).post(create))
        .route("/dashboards/{id}", get(get_one).put(update).delete(delete))
        .merge(variables::routes())
}

#[async_trait::async_trait]
impl ProtectedResource for Dashboard {
    type Id = Id;

    async fn load(state: &AppState, id: Self::Id) -> Result<Self> {
        state.dashboard.get(&id).await
    }

    fn organization_id(&self) -> &Id {
        &self.org_id
    }

    fn resource_type(&self) -> &str {
        "dashboard"
    }

    fn resource_id(&self) -> &str {
        self.id.as_str()
    }
}

#[derive(Deserialize)]
struct CreateReq {
    pub model: Value,
    pub folder_id: Option<String>,
}

#[derive(Deserialize)]
struct UpdateReq {
    pub model: Value,
    pub folder_id: Option<String>,
}

#[permission(any("dashboards.read", "sys.dashboards.read"))]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<Dashboard>>> {
    Ok(Json(state.dashboard.list(&ctx.org_id, None).await?))
}

#[permission("dashboards.create")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(body): Json<CreateReq>,
) -> Result<Json<Dashboard>> {
    let folder_id = body.folder_id.map(Id::from_string);
    if let Some(folder_id) = &folder_id {
        state
            .dashboard
            .folders()
            .get(&ctx.org_id, folder_id)
            .await?;
    }
    let dashboard = state
        .dashboard
        .create(
            ctx.org_id.clone(),
            folder_id,
            ctx.user_id.clone(),
            body.model,
        )
        .await?;
    emit_cud(
        &state,
        &ctx.org_id,
        ResourceKind::Dashboard,
        CudAction::Created,
        &dashboard.id.0,
        &dashboard,
    )
    .await;
    Ok(Json(dashboard))
}

#[resource_permission(
    action = any("dashboards.read", "sys.dashboards.read"),
    resource = Dashboard,
    id = Id::from_string(id),
    bind = dashboard
)]
async fn get_one(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Dashboard>> {
    Ok(Json(dashboard))
}

#[resource_permission(
    action = "dashboards.edit",
    resource = Dashboard,
    id = Id::from_string(id),
    bind = dashboard
)]
async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(body): Json<UpdateReq>,
) -> Result<Json<Dashboard>> {
    let folder_id = body.folder_id.map(Id::from_string);
    if let Some(folder_id) = &folder_id {
        state
            .dashboard
            .folders()
            .get(&dashboard.org_id, folder_id)
            .await?;
    }
    let saved = state
        .dashboard
        .update_model(dashboard, folder_id, ctx.user_id.clone(), body.model)
        .await?;
    emit_cud(
        &state,
        &saved.org_id,
        ResourceKind::Dashboard,
        CudAction::Updated,
        &saved.id.0,
        &saved,
    )
    .await;
    Ok(Json(saved))
}

#[resource_permission(
    action = "dashboards.delete",
    resource = Dashboard,
    id = Id::from_string(id),
    bind = dashboard
)]
async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<&'static str> {
    let rid = dashboard.id.clone();
    state.dashboard.delete(&rid).await?;
    emit_cud(
        &state,
        &dashboard.org_id,
        ResourceKind::Dashboard,
        CudAction::Deleted,
        &rid.0,
        &delete_payload(&rid.0),
    )
    .await;
    Ok("deleted")
}
