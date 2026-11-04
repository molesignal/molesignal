// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 改事件的优先级与紧急度（`alerts.incidents.update`）：
//!
//! - `PUT /api/v1/alerts/incidents/{id}/priority` `{"priority": "P2" | null}`
//! - `PUT /api/v1/alerts/incidents/{id}/urgency` `{"urgency": "high" | "low"}`：从低调到高时未确认的
//!   事件从第一级重新通知。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::put,
};
use serde::Deserialize;
use serde_json::Value;

use super::incident_actions::{actor, incident_json};
use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{
        alerting::{incident::Incident, priority::Priority, urgency::Urgency},
        iam::resource_permission,
    },
    shared::{Result, ids::Id},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/alerts/incidents/{id}/priority", put(set_priority))
        .route("/alerts/incidents/{id}/urgency", put(set_urgency))
}

#[derive(Deserialize)]
struct PriorityReq {
    #[serde(default)]
    priority: Option<Priority>,
}

#[derive(Deserialize)]
struct UrgencyReq {
    urgency: Urgency,
}

#[resource_permission(
    action = "alerts.incidents.update",
    resource = Incident,
    id = Id::from_string(id),
    bind = incident
)]
async fn set_priority(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<PriorityReq>,
) -> Result<Json<Value>> {
    let incident = state
        .alerting
        .actions
        .set_priority(&actor(&ctx), &incident.id, req.priority)
        .await?;

    Ok(incident_json(incident))
}

#[resource_permission(
    action = "alerts.incidents.update",
    resource = Incident,
    id = Id::from_string(id),
    bind = incident
)]
async fn set_urgency(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<UrgencyReq>,
) -> Result<Json<Value>> {
    let incident = state
        .alerting
        .actions
        .set_urgency(&actor(&ctx), &incident.id, req.urgency)
        .await?;

    Ok(incident_json(incident))
}
