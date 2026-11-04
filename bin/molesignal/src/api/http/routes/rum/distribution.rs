// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `GET|POST /rum/distribution`: how the values of a number fall across the rows a query matches, in
//! bars, with the percentiles that mark them.

use axum::{
    Extension, Json,
    extract::{Query, State},
};
use serde::{Deserialize, Serialize};

use super::{
    address::json_list,
    reader::{Counted, Reader},
    selection::Selection,
};
use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::permission,
    infra::rum::query::{
        BarPlan, Column, DEFAULT_BARS, Distribution, Filter, MAX_BARS, Scope, Unavailable,
        bars_sql, bounds_sql, read_bars, read_bounds, single_bar,
    },
    shared::{Error, Result},
};

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DistributionRequest {
    #[serde(rename = "type")]
    kind: Option<String>,
    from: Option<i64>,
    to: Option<i64>,
    #[serde(default, deserialize_with = "json_list")]
    filters: Vec<Filter>,
    #[serde(default, deserialize_with = "json_list")]
    free_text: Vec<String>,
    field: Option<String>,
    bars: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(super) struct DistributionResponse {
    field: String,
    unit: &'static str,
    #[serde(flatten)]
    found: Distribution,
    #[serde(skip_serializing_if = "Option::is_none")]
    unavailable: Option<Unavailable>,
}

#[permission(any("rum.events.read", "sys.telemetry.read"))]
pub(super) async fn distribution_post(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Json(request): Json<DistributionRequest>,
) -> Result<Json<DistributionResponse>> {
    distribution(&state, &iam, request).await.map(Json)
}

#[permission(any("rum.events.read", "sys.telemetry.read"))]
pub(super) async fn distribution_get(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Query(request): Query<DistributionRequest>,
) -> Result<Json<DistributionResponse>> {
    distribution(&state, &iam, request).await.map(Json)
}

async fn distribution(
    state: &AppState,
    iam: &IamContext,
    request: DistributionRequest,
) -> Result<DistributionResponse> {
    let selection = Selection::new(
        request.kind.as_deref(),
        request.from,
        request.to,
        request.filters,
        request.free_text,
    )?;

    let name = request
        .field
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| Error::invalid("field is required"))?;

    let column = Column::resolve(selection.stream, name)?
        .filter(Column::is_numeric)
        .ok_or_else(|| {
            Error::invalid(format!(
                "`{name}` is not a number of {}",
                selection.stream.name()
            ))
        })?;

    let conditions = selection.conditions()?;

    let scope = Scope {
        stream: selection.stream,
        from: selection.span.from,
        to: selection.span.to,
        conditions: &conditions,
    };

    let answer = |found, unavailable| DistributionResponse {
        field: name.to_string(),
        unit: column.unit(),
        found,
        unavailable,
    };

    let reader = Reader::new(state, iam);

    let bounds = match reader
        .count(
            selection.stream,
            selection.span,
            bounds_sql(&scope, &column),
        )
        .await?
    {
        Counted::Rows(Some(result)) => read_bounds(&result.columns, &result.rows),
        Counted::Rows(None) => Distribution::default(),
        Counted::Unavailable(reason) => return Ok(answer(Distribution::default(), Some(reason))),
    };

    let wanted = request.bars.unwrap_or(DEFAULT_BARS).clamp(1, MAX_BARS);

    let Some(plan) = BarPlan::of(&bounds, &column, wanted) else {
        return Ok(answer(single_bar(bounds), None));
    };

    match reader
        .count(
            selection.stream,
            selection.span,
            bars_sql(&scope, &column, plan),
        )
        .await?
    {
        Counted::Rows(Some(result)) => Ok(answer(
            read_bars(bounds, plan, &result.columns, &result.rows),
            None,
        )),
        Counted::Rows(None) => Ok(answer(bounds, None)),
        Counted::Unavailable(reason) => Ok(answer(bounds, Some(reason))),
    }
}
