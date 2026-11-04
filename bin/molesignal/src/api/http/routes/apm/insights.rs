// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What APM finds worth a look in the range: moves against the stretch
//! before and the same stretch a week before, versions that did worse, and
//! where time could be won back; for the services in view or one of them.

use std::time::Instant;

use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    routing::get,
};

use super::QueryParams;
use crate::{
    api::AppState,
    app::{
        apm::{InsightsResponse, record_apm_query_latency},
        iam::IamContext,
    },
    domain::iam::permission,
    shared::Result,
};

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/apm/insights", get(insights))
}

#[permission(any("apm.services.read", "sys.telemetry.read"))]
async fn insights(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(params): Query<QueryParams>,
) -> Result<Json<InsightsResponse>> {
    let started = Instant::now();
    let context = state.telemetry.apm_query.context(
        ctx.org_id.clone(),
        params.request(),
        &["request_count"],
        "request_count",
    )?;

    let response = state.telemetry.apm_query.insights(&context).await?;

    record_apm_query_latency("insights", started.elapsed());
    Ok(Json(response))
}
