// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 组织的优先级与紧急度档位：
//!
//! - `GET /api/v1/alerts/importance`（`alerts.importance.read`）：`{levels, updated_by, updated_at}`；没改过时是
//!   内置的档位。
//! - `PUT /api/v1/alerts/importance`（`alerts.importance.manage`）：`{priorities, urgencies}`，整份换掉。

use axum::{Extension, Json, Router, extract::State, routing::get};
use serde_json::json;

use crate::{
    api::{AppState, http::routes::activity_audit},
    app::iam::IamContext,
    domain::{
        alerting::{
            importance::{ImportanceLevels, StoredImportance},
            rule::AlertTrigger,
        },
        iam::permission,
    },
    shared::{Result, ids::Id},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new().route(
        "/alerts/importance",
        get(get_importance).put(put_importance),
    )
}

#[permission("alerts.importance.read")]
async fn get_importance(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<StoredImportance>> {
    Ok(Json(state.alerting.importance.stored(&ctx.org_id).await?))
}

#[permission("alerts.importance.manage")]
async fn put_importance(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(levels): Json<ImportanceLevels>,
) -> Result<Json<StoredImportance>> {
    let saved = state
        .alerting
        .importance
        .save(&ctx.org_id, levels, Some(ctx.user_id.clone()))
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "alert_importance.updated",
        "alert_importance",
        "",
        json!({
            "priorities": saved.levels.priorities.len(),
            "urgencies": saved.levels.urgencies.iter().map(|level| level.key.as_str()).collect::<Vec<_>>(),
        }),
    )
    .await;

    Ok(Json(saved))
}

/// 规则开的事件带的优先级、紧急度是组织的档。
pub(crate) async fn check_trigger(
    state: &AppState,
    org_id: &Id,
    trigger: &AlertTrigger,
) -> Result<()> {
    let levels = state.alerting.importance.levels(org_id).await;

    if let Some(priority) = trigger.priority {
        levels.check_priority(priority)?;
    }

    if let Some(urgency) = &trigger.urgency {
        levels.check_urgency(urgency)?;
    }

    Ok(())
}
