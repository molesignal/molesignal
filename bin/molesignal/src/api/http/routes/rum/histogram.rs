// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `GET|POST /rum/histogram`: how many rows the explorer's query matches, bar by bar.
//!
//! The list returns a page. This counts everything the same query matches in its range, selecting
//! with the conditions the list itself selects by.

use axum::{
    Extension, Json,
    extract::{Query, State},
};
use serde::Deserialize;

use super::{
    address::json_list,
    reader::{Counted, Reader},
    selection::Selection,
};
use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::permission,
    infra::rum::query::{BucketPlan, Filter, Histogram, Scope, histogram_sql},
    shared::Result,
};

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct HistogramRequest {
    #[serde(rename = "type")]
    kind: Option<String>,
    from: Option<i64>,
    to: Option<i64>,
    #[serde(default, deserialize_with = "json_list")]
    filters: Vec<Filter>,
    #[serde(default, deserialize_with = "json_list")]
    free_text: Vec<String>,
    bucket_time: Option<i64>,
    origin_time: Option<i64>,
}

#[permission(any("rum.events.read", "sys.telemetry.read"))]
pub(super) async fn histogram_post(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Json(request): Json<HistogramRequest>,
) -> Result<Json<Histogram>> {
    histogram(&state, &iam, request).await.map(Json)
}

#[permission(any("rum.events.read", "sys.telemetry.read"))]
pub(super) async fn histogram_get(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Query(request): Query<HistogramRequest>,
) -> Result<Json<Histogram>> {
    histogram(&state, &iam, request).await.map(Json)
}

async fn histogram(
    state: &AppState,
    iam: &IamContext,
    request: HistogramRequest,
) -> Result<Histogram> {
    let plan = BucketPlan::new(
        request.from,
        request.to,
        request.bucket_time,
        request.origin_time,
    )?;

    let selection = Selection::new(
        request.kind.as_deref(),
        Some(plan.from),
        Some(plan.to),
        request.filters,
        request.free_text,
    )?;

    let conditions = selection.conditions()?;

    let statement = histogram_sql(
        &Scope {
            stream: selection.stream,
            from: plan.from,
            to: plan.to,
            conditions: &conditions,
        },
        plan.origin_micros,
        plan.bucket_micros,
    );

    match Reader::new(state, iam)
        .count(selection.stream, selection.span, statement)
        .await?
    {
        Counted::Rows(Some(result)) => plan.counted(&result.columns, &result.rows),
        Counted::Rows(None) => Ok(plan.empty()),
        Counted::Unavailable(reason) => Ok(plan.unavailable(reason)),
    }
}
