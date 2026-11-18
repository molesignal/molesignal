// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 和一个事件有关的别的东西：
//!
//! - `GET /alerts/incidents/{id}/similar`：近 90 天里像它的事件；
//! - `GET /alerts/incidents/{id}/changes`：它触发前后，它的服务和下游服务上报的变更，以及前一天里改过的
//!   这条规则。下游从 APM 的服务地图读，没有地图时只看它自己的服务。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::get,
};
use domain::{
    alerting::incident::Incident,
    iam::{IamContext, resource_permission},
};
use kernel::{Result, ids::Id};
use serde_json::{Value, json};

use crate::{api::AppState, app::alerting::related::downstream_of};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/alerts/incidents/{id}/similar", get(similar))
        .route("/alerts/incidents/{id}/changes", get(changes))
}

#[resource_permission(
    action = "alerts.incidents.read",
    resource = Incident,
    id = Id::from_string(id),
    bind = incident
)]
async fn similar(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let similar = state.alerting.related.similar(&incident).await?;

    Ok(Json(json!({ "incidents": similar })))
}

#[resource_permission(
    action = "alerts.incidents.read",
    resource = Incident,
    id = Id::from_string(id),
    bind = incident
)]
async fn changes(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let downstream = downstream_of(
        &state.telemetry.apm_query,
        state.telemetry.service_graph.as_ref(),
        &ctx.org_id,
        &incident,
    )
    .await;

    let changes = state
        .alerting
        .related
        .changes(&incident, &downstream)
        .await?;

    Ok(Json(serde_json::to_value(changes).unwrap_or(Value::Null)))
}
