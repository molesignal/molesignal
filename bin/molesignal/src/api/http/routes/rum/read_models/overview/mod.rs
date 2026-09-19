// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Json,
    extract::{Query, State},
};

use super::{ReadModelContext, ReadModelQuery};
use crate::{api::AppState, app::iam::IamContext, domain::iam::permission, shared::Result};

mod actions;
mod errors;
mod model;
mod sessions;

use model::{OverviewInsightsResponse, OverviewSummaryResponse};

#[permission(any("streams.query", "sys.telemetry.read"))]
pub(super) async fn read(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Query(request): Query<ReadModelQuery>,
) -> Result<Json<OverviewSummaryResponse>> {
    let context = ReadModelContext::resolve(request)?;
    let (sessions, actions) = tokio::try_join!(
        sessions::load(&state, &iam, &context),
        actions::load_metrics(&state, &iam, &context),
    )?;
    Ok(Json(OverviewSummaryResponse::from_parts(sessions, actions)))
}

#[permission(any("streams.query", "sys.telemetry.read"))]
pub(super) async fn insights(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Query(request): Query<ReadModelQuery>,
) -> Result<Json<OverviewInsightsResponse>> {
    let context = ReadModelContext::resolve(request)?;
    let (actions, frequent_errors) = tokio::try_join!(
        actions::load_insights(&state, &iam, &context),
        errors::load(&state, &iam, &context),
    )?;
    Ok(Json(OverviewInsightsResponse::from_parts(
        actions,
        frequent_errors,
    )))
}
