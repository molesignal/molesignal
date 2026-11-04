// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::Deserialize;
use serde_json::Value;

use super::{ensure_user_scope, parse_category};
use crate::{
    api::AppState,
    app::{iam::IamContext, notify::PreferenceSteps},
    domain::notify::preference::UserNotifyPreference,
    shared::{Result, ids::Id},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/users/{userId}/notify-preferences", get(list_preferences))
        .route(
            "/users/{userId}/notify-preferences/{category}",
            axum::routing::put(upsert_preference),
        )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreferenceRequest {
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    endpoint_ids: Vec<Id>,
    /// 各步的等待时间（秒），与 `endpoint_ids` 一一对应；不传表示都不等。
    #[serde(default)]
    step_delays_secs: Vec<i32>,
    /// 低紧急度告警用的步骤（只有告警和升级两类有）；空着时只用上面那组的第一步。
    #[serde(default)]
    low_endpoint_ids: Vec<Id>,
    #[serde(default)]
    low_step_delays_secs: Vec<i32>,
    #[serde(default)]
    quiet_hours: Option<Value>,
    /// 免打扰时段里仍接收高紧急度的告警。
    #[serde(default = "default_true")]
    allow_high_urgency_bypass: bool,
}

fn default_true() -> bool {
    true
}

async fn list_preferences(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(user_id): Path<String>,
) -> Result<Json<Vec<UserNotifyPreference>>> {
    let user_id = Id::from_string(user_id);
    ensure_user_scope(&state, &context, &user_id, false).await?;

    Ok(Json(
        state
            .alerting
            .notify
            .list_preferences(&context.org_id, &user_id)
            .await?,
    ))
}

async fn upsert_preference(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((user_id, category)): Path<(String, String)>,
    Json(request): Json<PreferenceRequest>,
) -> Result<Json<UserNotifyPreference>> {
    let user_id = Id::from_string(user_id);
    ensure_user_scope(&state, &context, &user_id, true).await?;

    Ok(Json(
        state
            .alerting
            .notify
            .upsert_preference(
                &context.org_id,
                &user_id,
                parse_category(&category)?,
                request.enabled,
                PreferenceSteps {
                    endpoint_ids: request.endpoint_ids,
                    step_delays_secs: request.step_delays_secs,
                    low_endpoint_ids: request.low_endpoint_ids,
                    low_step_delays_secs: request.low_step_delays_secs,
                },
                request.quiet_hours,
                request.allow_high_urgency_bypass,
            )
            .await?,
    ))
}
