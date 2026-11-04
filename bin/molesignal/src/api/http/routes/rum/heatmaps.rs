// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `/rum/heatmaps/{clicks,scroll,snapshot}`: where people click on a page and how far down they go,
//! and a recording of the page to draw that over. The numbers take `rum.analytics.read`; the
//! recording shows what a replay shows, so it takes `rum.replay.read`. Each is asked with `GET` (the
//! filters are a JSON list in a parameter, so a question is an address) or `POST`.

use axum::{
    Extension, Json,
    extract::{Query, State},
};
use serde::Deserialize;

use super::{address::json_list, reader::Reader};
use crate::{
    api::AppState,
    app::{
        iam::IamContext,
        rum::{
            analytics::Answer,
            heatmap::{self, MapRequest, SnapshotAnswer, SnapshotRequest},
        },
    },
    domain::iam::permission,
    infra::rum::query::{ClickMap, Filter, ScrollMap},
    shared::Result,
};

#[derive(Debug, Default, Deserialize)]
pub(super) struct MapQuery {
    from: Option<i64>,
    to: Option<i64>,
    #[serde(default, deserialize_with = "json_list")]
    filters: Vec<Filter>,
    limit: Option<usize>,
}

impl From<MapQuery> for MapRequest {
    fn from(query: MapQuery) -> Self {
        Self {
            from: query.from,
            to: query.to,
            filters: query.filters,
            limit: query.limit,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SnapshotQuery {
    from: Option<i64>,
    to: Option<i64>,
    #[serde(default, deserialize_with = "json_list")]
    filters: Vec<Filter>,
    view_id: Option<String>,
}

impl From<SnapshotQuery> for SnapshotRequest {
    fn from(query: SnapshotQuery) -> Self {
        Self {
            from: query.from,
            to: query.to,
            filters: query.filters,
            view_id: query.view_id,
        }
    }
}

#[permission(any("rum.analytics.read", "sys.telemetry.read"))]
pub(super) async fn clicks_post(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Json(request): Json<MapRequest>,
) -> Result<Json<Answer<ClickMap>>> {
    heatmap::clicks(&Reader::new(&state, &iam), &request)
        .await
        .map(Json)
}

#[permission(any("rum.analytics.read", "sys.telemetry.read"))]
pub(super) async fn clicks_get(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Query(query): Query<MapQuery>,
) -> Result<Json<Answer<ClickMap>>> {
    heatmap::clicks(&Reader::new(&state, &iam), &query.into())
        .await
        .map(Json)
}

#[permission(any("rum.analytics.read", "sys.telemetry.read"))]
pub(super) async fn scroll_post(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Json(request): Json<MapRequest>,
) -> Result<Json<Answer<ScrollMap>>> {
    heatmap::scroll(&Reader::new(&state, &iam), &request)
        .await
        .map(Json)
}

#[permission(any("rum.analytics.read", "sys.telemetry.read"))]
pub(super) async fn scroll_get(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Query(query): Query<MapQuery>,
) -> Result<Json<Answer<ScrollMap>>> {
    heatmap::scroll(&Reader::new(&state, &iam), &query.into())
        .await
        .map(Json)
}

#[permission(any("rum.replay.read", "sys.telemetry.read"))]
pub(super) async fn snapshot_post(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Json(request): Json<SnapshotRequest>,
) -> Result<Json<SnapshotAnswer>> {
    heatmap::snapshot(
        &Reader::new(&state, &iam),
        &state.telemetry.rum_replay,
        &iam.org_id,
        request,
    )
    .await
    .map(Json)
}

#[permission(any("rum.replay.read", "sys.telemetry.read"))]
pub(super) async fn snapshot_get(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Query(query): Query<SnapshotQuery>,
) -> Result<Json<SnapshotAnswer>> {
    heatmap::snapshot(
        &Reader::new(&state, &iam),
        &state.telemetry.rum_replay,
        &iam.org_id,
        query.into(),
    )
    .await
    .map(Json)
}
