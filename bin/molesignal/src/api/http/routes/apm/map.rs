// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The APM service map, one of its calls, and what an incident on a service
//! reaches along it. Calls between services come from the service graph,
//! the rest from the APM buckets.

use std::time::Instant;

use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    routing::get,
};
use serde::Deserialize;

use super::QueryParams;
use crate::{
    api::AppState,
    app::{
        apm::{BlastRadius, MapEdgeDetail, MapResponse, record_apm_query_latency},
        iam::IamContext,
    },
    domain::iam::permission,
    shared::{Error, Result},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/apm/map", get(map))
        .route("/apm/map/edge", get(map_edge))
        .route("/apm/blast-radius", get(blast_radius))
}

/// The two ends of a call, by node id; read beside the common parameters.
#[derive(Debug, Deserialize)]
struct EdgeEnds {
    source: String,
    target: String,
}

#[permission(any("apm.services.read", "sys.telemetry.read"))]
async fn map(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(params): Query<QueryParams>,
) -> Result<Json<MapResponse>> {
    let started = Instant::now();
    let context = state.telemetry.apm_query.context(
        ctx.org_id.clone(),
        params.request(),
        &["request_count"],
        "request_count",
    )?;

    let response = state
        .telemetry
        .apm_query
        .map_of(&context, state.telemetry.service_graph.as_ref())
        .await?;

    record_apm_query_latency("map", started.elapsed());
    Ok(Json(response))
}

#[permission(any("apm.services.read", "sys.telemetry.read"))]
async fn map_edge(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(params): Query<QueryParams>,
    Query(ends): Query<EdgeEnds>,
) -> Result<Json<MapEdgeDetail>> {
    let started = Instant::now();
    let context = state.telemetry.apm_query.context(
        ctx.org_id.clone(),
        params.request(),
        &["request_count"],
        "request_count",
    )?;

    let response = state
        .telemetry
        .apm_query
        .map_edge_of(
            &context,
            state.telemetry.service_graph.as_ref(),
            &ends.source,
            &ends.target,
        )
        .await?;

    record_apm_query_latency("map_edge", started.elapsed());
    Ok(Json(response))
}

/// What reaches a service and what it reaches along the map: who calls it,
/// and what it calls, in the range in view.
#[permission(any("apm.services.read", "sys.telemetry.read"))]
async fn blast_radius(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(params): Query<QueryParams>,
) -> Result<Json<BlastRadius>> {
    let started = Instant::now();
    let service = params
        .service
        .clone()
        .filter(|service| !service.trim().is_empty())
        .ok_or_else(|| Error::invalid("the blast radius is of a service"))?;
    // The whole map of the environment: what reaches the service crosses namespaces.
    let whole = QueryParams {
        service: None,
        namespace: None,
        ..params
    };
    let context = state.telemetry.apm_query.context(
        ctx.org_id.clone(),
        whole.request(),
        &["request_count"],
        "request_count",
    )?;

    let response = state
        .telemetry
        .apm_query
        .blast_radius(&context, state.telemetry.service_graph.as_ref(), &service)
        .await?;

    record_apm_query_latency("blast_radius", started.elapsed());
    Ok(Json(response))
}
