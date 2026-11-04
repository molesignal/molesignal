// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `POST /rum/panel-statement`: the statement a dashboard panel runs to draw what the explorer
//! draws, its chart of the rows its query matches, and the columns it answers with.
//!
//! Nothing is read: the statement names no time, since a dashboard runs it over the range its
//! reader picks, and the page that adds the panel puts it into a dashboard.

use axum::{Extension, Json, extract::State};
use serde::Deserialize;

use super::selection::Selection;
use crate::{
    api::AppState,
    app::{
        iam::IamContext,
        rum::{
            aggregate::descending,
            panel::{self, Chart, Panel},
        },
    },
    domain::iam::permission,
    infra::rum::query::{Filter, MeasureSpec},
    shared::Result,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PanelRequest {
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    filters: Vec<Filter>,
    #[serde(default)]
    free_text: Vec<String>,
    chart: Chart,
    #[serde(default)]
    measures: Vec<MeasureSpec>,
    #[serde(default)]
    group_by: Vec<String>,
    /// What the panel calls the groups, in their order.
    #[serde(default)]
    group_names: Vec<String>,
    order_by: Option<String>,
    /// `asc` or `desc`.
    order: Option<String>,
    limit: Option<usize>,
    /// The number a distribution is of.
    field: Option<String>,
    /// The interval a chart over time is cut into: `${interval}` or a length such as `15 minutes`.
    interval: Option<String>,
}

#[permission(any("rum.events.read", "sys.telemetry.read"))]
pub(super) async fn statement(
    State(_state): State<AppState>,
    Extension(_iam): Extension<IamContext>,
    Json(request): Json<PanelRequest>,
) -> Result<Json<Panel>> {
    // The range is the dashboard's: the selection's own is never read.
    let selection = Selection::new(
        request.kind.as_deref(),
        None,
        None,
        request.filters,
        request.free_text,
    )?;

    panel::panel(&panel::Request {
        selection: &selection,
        chart: request.chart,
        measures: &request.measures,
        group_by: &request.group_by,
        group_names: &request.group_names,
        order_by: request.order_by.as_deref(),
        descending: descending(request.order.as_deref())?,
        limit: request.limit,
        field: request.field.as_deref(),
        interval: request.interval.as_deref(),
    })
    .map(Json)
}
