// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What services run on: `GET /entities` lists clusters, nodes, hosts, pods and containers seen
//! lately with how many services run on each; `GET /entities/detail` tells one with what it runs
//! on or is part of, what runs on or is part of it, and the services on it.

use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    routing::get,
};
use domain::{
    entity::{EntityKind, EntityQuery, MAX_LISTED_ENTITIES},
    iam::{IamContext, permission},
};
use kernel::{Error, Result, time::TimestampMicros};
use serde::Deserialize;

use crate::{
    api::AppState,
    app::entities::{EntityDetail, EntityRow},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/entities", get(list))
        .route("/entities/detail", get(detail))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListParams {
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub search: Option<String>,
    /// Only those seen since, in microseconds.
    #[serde(default)]
    pub seen_since: Option<i64>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct DetailParams {
    pub kind: String,
    pub key: String,
}

fn kind_of(kind: &str) -> Result<EntityKind> {
    EntityKind::parse(kind).ok_or_else(|| Error::invalid(format!("unknown entity kind `{kind}`")))
}

#[permission(any("apm.services.read", "metrics.read", "logs.read", "sys.telemetry.read"))]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(p): Query<ListParams>,
) -> Result<Json<Vec<EntityRow>>> {
    let query = EntityQuery {
        kind: p
            .kind
            .as_deref()
            .filter(|kind| !kind.is_empty())
            .map(kind_of)
            .transpose()?,
        search: p.search,
        seen_since: p.seen_since.map(TimestampMicros),
        limit: p.limit.unwrap_or(MAX_LISTED_ENTITIES),
    };

    Ok(Json(
        state.telemetry.entities.list(&ctx.org_id, &query).await?,
    ))
}

#[permission(any("apm.services.read", "metrics.read", "logs.read", "sys.telemetry.read"))]
async fn detail(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(p): Query<DetailParams>,
) -> Result<Json<EntityDetail>> {
    let kind = kind_of(&p.kind)?;

    state
        .telemetry
        .entities
        .detail(&ctx.org_id, kind, &p.key)
        .await?
        .map(Json)
        .ok_or_else(|| Error::not_found(format!("no {} `{}` was seen", p.kind, p.key)))
}
