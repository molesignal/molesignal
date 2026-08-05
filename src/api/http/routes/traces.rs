// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! traces 派生查询路由。
//!
//! 仅暴露 `GET /api/v1/traces/service_graph?from=&to=&service=` 一个查询接口。
//! 真正的 traces ingest 走通用 `/api/v1/ingest/traces/:stream`。

use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    routing::get,
};
use serde::Deserialize;
use serde_json::Value;

use crate::{api::AppState, app::iam::IamContext, domain::iam::permission, shared::Result};

pub fn routes() -> Router<AppState> {
    Router::new().route("/traces/service_graph", get(service_graph))
}

#[derive(Debug, Deserialize)]
pub struct ServiceGraphParams {
    pub from: i64,
    pub to: i64,
    pub service: Option<String>,
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn service_graph(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(p): Query<ServiceGraphParams>,
) -> Result<Json<Value>> {
    let edges = state
        .telemetry
        .service_graph
        .query(&ctx.org_id, p.from, p.to, p.service.as_deref())
        .await?;
    Ok(Json(serde_json::json!({
        "edges": edges,
        "count": edges.len(),
    })))
}
