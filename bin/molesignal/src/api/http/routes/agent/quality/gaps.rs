// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The data gaps of a period: what the tool calls of its answers say is missing from the data.
//!
//! - `GET /api/v1/agent/quality/gaps?days=`: answers that called tools, how many found a gap of
//!   each kind, and the gaps grouped by rule, stream and service with the answers that found them.
//!
//! Personal chats count in the totals only (`agent::scoring::gaps`).

use agent::scoring::gaps::aggregate;
use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    routing::get,
};
use domain::iam::{IamContext, permission};
use kernel::{Result, time::TimestampMicros};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::api::AppState;

const MICROS_PER_DAY: i64 = 86_400_000_000;
const DEFAULT_DAYS: i64 = 30;
const MAX_DAYS: i64 = 90;
/// The most answers a period reads; the newest are kept.
const MOST_ANSWERS: i64 = 5_000;

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/agent/quality/gaps", get(gaps))
}

#[derive(Debug, Deserialize)]
struct GapsQuery {
    #[serde(default)]
    days: Option<i64>,
}

#[permission("agent.quality.read")]
async fn gaps(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<GapsQuery>,
) -> Result<Json<Value>> {
    let days = query.days.unwrap_or(DEFAULT_DAYS).clamp(1, MAX_DAYS);
    let since = TimestampMicros(TimestampMicros::now().0 - days * MICROS_PER_DAY);

    let rows = state
        .agent
        .scoring
        .trajectories(&ctx.org_id, since, MOST_ANSWERS)
        .await?;

    Ok(Json(json!({
        "days": days,
        "gaps": aggregate(&rows),
    })))
}
