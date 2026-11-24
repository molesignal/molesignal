// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! How well each service in view can be tied to its logs, metrics, traces and
//! changes, and the service names in logs and metrics that APM does not know.
//! Only counts and names are read from the streams, never lines.

use std::time::Instant;

use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    routing::get,
};
use domain::iam::{IamContext, permission};
use kernel::Result;

use super::QueryParams;
use crate::{
    api::AppState,
    app::apm::{
        correlation::{CorrelationReader, CorrelationReport},
        record_apm_query_latency,
    },
};

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/apm/correlation", get(correlation))
}

#[permission(any("apm.services.read", "sys.telemetry.read"))]
async fn correlation(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(params): Query<QueryParams>,
) -> Result<Json<CorrelationReport>> {
    let started = Instant::now();
    // The platform's own log stream only for the platform scope in its own organization.
    let system_streams = ctx.is_system_scope() && ctx.org_id == state.iam.system_org_id;

    let reader = CorrelationReader {
        query: &state.query,
        streams: state.telemetry.streams.as_ref(),
        apm: &state.telemetry.apm_query,
        catalog: state.telemetry.apm_catalog.as_ref(),
    };

    let report = reader
        .report(&ctx.org_id, params.request(), system_streams)
        .await?;

    record_apm_query_latency("correlation", started.elapsed());
    Ok(Json(report))
}
