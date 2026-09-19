// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};
use serde::Deserialize;

use super::parse_category;
use crate::{
    api::AppState,
    app::{iam::IamContext, notify::NotifyDefaultInput},
    domain::{
        iam::permission,
        notify::routing::{NotifyDefaultRoute, OrganizationNotifyDefault, TeamNotifyDefault},
    },
    shared::{Error, Result, ids::Id},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/notify/team-defaults/{team_id}", get(list_team))
        .route(
            "/notify/team-defaults/{team_id}/{category}",
            get(get_team).put(upsert_team).delete(delete_team),
        )
        .route("/notify/organization-defaults", get(list_organization))
        .route(
            "/notify/organization-defaults/{category}",
            get(get_organization)
                .put(upsert_organization)
                .delete(delete_organization),
        )
}

#[derive(Debug, Deserialize)]
struct DefaultRequest {
    routes: Vec<NotifyDefaultRoute>,
    #[serde(default = "default_true")]
    enabled: bool,
}

const fn default_true() -> bool {
    true
}

impl From<DefaultRequest> for NotifyDefaultInput {
    fn from(request: DefaultRequest) -> Self {
        Self {
            routes: request.routes,
            enabled: request.enabled,
        }
    }
}

#[permission("alerts.read")]
async fn list_team(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(team_id): Path<String>,
) -> Result<Json<Vec<TeamNotifyDefault>>> {
    Ok(Json(
        state
            .alerting
            .notify_engine
            .list_team_defaults(&ctx.org_id, &Id::from_string(team_id))
            .await?,
    ))
}

#[permission("alerts.read")]
async fn get_team(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path((team_id, category)): Path<(String, String)>,
) -> Result<Json<TeamNotifyDefault>> {
    Ok(Json(
        state
            .alerting
            .notify_engine
            .get_team_default(
                &ctx.org_id,
                &Id::from_string(team_id),
                parse_category(&category)?,
            )
            .await?
            .ok_or_else(|| Error::not_found("team notify default"))?,
    ))
}

#[permission("alerts.manage")]
async fn upsert_team(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path((team_id, category)): Path<(String, String)>,
    Json(request): Json<DefaultRequest>,
) -> Result<Json<TeamNotifyDefault>> {
    Ok(Json(
        state
            .alerting
            .notify_engine
            .upsert_team_default(
                &ctx.org_id,
                &Id::from_string(team_id),
                parse_category(&category)?,
                request.into(),
            )
            .await?,
    ))
}

#[permission("alerts.manage")]
async fn delete_team(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path((team_id, category)): Path<(String, String)>,
) -> Result<StatusCode> {
    state
        .alerting
        .notify_engine
        .delete_team_default(
            &ctx.org_id,
            &Id::from_string(team_id),
            parse_category(&category)?,
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[permission("alerts.read")]
async fn list_organization(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<OrganizationNotifyDefault>>> {
    Ok(Json(
        state
            .alerting
            .notify_engine
            .list_organization_defaults(&ctx.org_id)
            .await?,
    ))
}

#[permission("alerts.read")]
async fn get_organization(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(category): Path<String>,
) -> Result<Json<OrganizationNotifyDefault>> {
    Ok(Json(
        state
            .alerting
            .notify_engine
            .get_organization_default(&ctx.org_id, parse_category(&category)?)
            .await?
            .ok_or_else(|| Error::not_found("organization notify default"))?,
    ))
}

#[permission("alerts.manage")]
async fn upsert_organization(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(category): Path<String>,
    Json(request): Json<DefaultRequest>,
) -> Result<Json<OrganizationNotifyDefault>> {
    Ok(Json(
        state
            .alerting
            .notify_engine
            .upsert_organization_default(&ctx.org_id, parse_category(&category)?, request.into())
            .await?,
    ))
}

#[permission("alerts.manage")]
async fn delete_organization(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(category): Path<String>,
) -> Result<StatusCode> {
    state
        .alerting
        .notify_engine
        .delete_organization_default(&ctx.org_id, parse_category(&category)?)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
