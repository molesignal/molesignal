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
use serde_json::{Value, json};

use crate::{
    api::AppState,
    app::{
        alerting::related::{AFTER, BEFORE},
        apm::{ApmQueryRequest, MapNodeKind},
        iam::IamContext,
    },
    domain::{alerting::incident::Incident, iam::resource_permission},
    shared::{Result, ids::Id},
};

/// 最多看几个服务的下游。
const MOST_SERVICES: usize = 3;

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
    let downstream = downstream_of(&state, &ctx, &incident).await;
    let changes = state
        .alerting
        .related
        .changes(&incident, &downstream)
        .await?;

    Ok(Json(serde_json::to_value(changes).unwrap_or(Value::Null)))
}

/// 事件的服务在服务地图上的下游服务，尽力而为：APM 没有地图或读不出来时为空。
async fn downstream_of(state: &AppState, ctx: &IamContext, incident: &Incident) -> Vec<String> {
    let apm = &state.telemetry.apm_query;
    let mut names = Vec::new();

    for service in incident
        .affected_services
        .iter()
        .filter(|service| !service.trim().is_empty())
        .take(MOST_SERVICES)
    {
        let request = ApmQueryRequest {
            from: incident.created_at.0 - BEFORE,
            to: incident.created_at.0 + AFTER,
            namespace: None,
            service: None,
            environment: None,
            version: None,
            resolution: Default::default(),
            sort: None,
            direction: None,
            limit: None,
            cursor: None,
        };

        let Ok(context) = apm.context(
            ctx.org_id.clone(),
            request,
            &["request_count"],
            "request_count",
        ) else {
            continue;
        };

        let Ok(radius) = apm
            .blast_radius(&context, state.telemetry.service_graph.as_ref(), service)
            .await
        else {
            continue;
        };

        names.extend(
            radius
                .downstream
                .into_iter()
                .filter(|node| node.kind == MapNodeKind::Service)
                .map(|node| node.name),
        );
    }

    names
}
