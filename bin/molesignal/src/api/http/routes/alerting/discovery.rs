// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 自动发现找到的问题（`alerts.discovery.read`）：
//!
//! - `GET /api/v1/alerts/discovery/findings?status=open|ended|all&kind=&limit=`：最近看到的在前；
//!   `status` 默认 `open`，`limit` 默认 100、最多 500。
//! - `GET /api/v1/alerts/discovery/findings/{id}`：一条发现。

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use serde::Deserialize;

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{
        alerting::discovery::{Finding, FindingFilter, FindingKind},
        iam::permission,
    },
    shared::{Error, Result, ids::Id},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/alerts/discovery/findings", get(list_findings))
        .route("/alerts/discovery/findings/{id}", get(get_finding))
}

#[derive(Debug, Default, Deserialize)]
struct FindingQuery {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[permission("alerts.discovery.read")]
async fn list_findings(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<FindingQuery>,
) -> Result<Json<Vec<Finding>>> {
    let open = match query.status.as_deref().unwrap_or("open") {
        "open" => Some(true),
        "ended" => Some(false),
        "all" => None,
        _ => return Err(Error::invalid("status must be open, ended, or all")),
    };

    let kind = query
        .kind
        .as_deref()
        .filter(|kind| !kind.is_empty())
        .map(|kind| FindingKind::parse(kind).ok_or_else(|| Error::invalid("unknown finding kind")))
        .transpose()?;

    let findings = state
        .alerting
        .discovery
        .list(
            &ctx.org_id,
            &FindingFilter {
                open,
                kind,
                since: None,
                limit: query.limit.unwrap_or(100).clamp(1, 500),
            },
        )
        .await?;

    Ok(Json(findings))
}

#[permission("alerts.discovery.read")]
async fn get_finding(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Finding>> {
    state
        .alerting
        .discovery
        .get(&ctx.org_id, &Id(id))
        .await?
        .map(Json)
        .ok_or_else(|| Error::not_found("finding not found"))
}
