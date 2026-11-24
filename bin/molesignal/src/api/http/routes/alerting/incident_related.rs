// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 和一个事件有关的别的东西：
//!
//! - `GET /alerts/incidents/{id}/similar`：近 90 天里像它的事件；
//! - `GET /alerts/incidents/{id}/changes`：它触发前后，它的服务和下游服务上报的变更，以及前一天里改过的
//!   这条规则。下游从 APM 的服务地图读，没有地图时只看它自己的服务。
//! - `GET /alerts/incidents/{id}/business-impact`: for each business key the Spans carry, the
//!   values (tenants, plans) whose requests to its services failed most around the time it
//!   began. The figures are read from Spans, so a reader who may not read them is refused.

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::get,
};
use domain::{
    alerting::{incident::Incident, rca_evidence::EvidenceSource},
    iam::{IamContext, resource_permission},
};
use kernel::{
    Error, Result,
    ids::Id,
    time::{TimeRange, TimestampMicros},
};
use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    api::AppState,
    app::{
        alerting::related::{downstream_of, incident_services, incident_window},
        business_impact::{ImpactQuery, KeyImpact, business_impact},
    },
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/alerts/incidents/{id}/similar", get(similar))
        .route("/alerts/incidents/{id}/changes", get(changes))
        .route(
            "/alerts/incidents/{id}/business-impact",
            get(incident_business_impact),
        )
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BusinessImpactResponse {
    from: TimestampMicros,
    to: TimestampMicros,
    services: Vec<String>,
    /// Whether the organization declared any business key the Spans carry.
    declared: bool,
    keys: Vec<KeyImpact>,
}

#[resource_permission(
    action = "alerts.incidents.read",
    resource = Incident,
    id = Id::from_string(id),
    bind = incident
)]
async fn incident_business_impact(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<BusinessImpactResponse>> {
    let readable = EvidenceSource::Traces
        .permissions()
        .iter()
        .any(|permission| ctx.has_permission(permission));

    if !readable {
        return Err(Error::forbidden(
            "reading an incident's business impact needs the right to query Spans",
        ));
    }

    let window: TimeRange = incident_window(&incident, TimestampMicros::now());
    let mut services = incident_services(&incident).into_iter().collect::<Vec<_>>();
    services.sort();

    let context = state
        .telemetry
        .business_context
        .business_context(&ctx.org_id)
        .await?;

    let declared = context
        .carried_by(domain::business_context::BusinessSignal::Span)
        .next()
        .is_some();

    let keys = if declared {
        business_impact(
            &state.query,
            state.telemetry.streams.as_ref(),
            &ImpactQuery {
                org_id: &ctx.org_id,
                context: &context,
                services: &services,
                window,
            },
        )
        .await?
    } else {
        Vec::new()
    };

    Ok(Json(BusinessImpactResponse {
        from: window.start,
        to: window.end,
        services,
        declared,
        keys,
    }))
}
