// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 分给自己的复盘改进项：`GET /alerts/postmortem-actions/mine` 列出调用者名下还没做完的，到期早的在前，
//! 工作台的待办读它。

use axum::{Extension, Json, Router, extract::State, routing::get};
use domain::iam::{IamContext, permission};
use kernel::Result;
use serde_json::{Value, json};

use crate::api::AppState;

/// 工作台最多列这么多项。
const MINE_LIMIT: i64 = 50;

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/alerts/postmortem-actions/mine", get(mine))
}

#[permission("postmortems.read")]
async fn mine(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    let actions = state
        .alerting
        .postmortems
        .repository()
        .open_actions_owned_by(&ctx.org_id, &ctx.user_id, MINE_LIMIT)
        .await?;

    Ok(Json(json!({ "actions": actions })))
}
