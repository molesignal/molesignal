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
    app::iam::IamContext,
    domain::notify::preference::UserNotifyPreference,
    shared::{Result, ids::Id},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/users/{user_id}/notify-preferences", get(list_preferences))
        .route(
            "/users/{user_id}/notify-preferences/{category}",
            axum::routing::put(upsert_preference),
        )
}

#[derive(Debug, Deserialize)]
struct PreferenceRequest {
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    endpoint_ids: Vec<Id>,
    #[serde(default)]
    quiet_hours: Option<Value>,
    #[serde(default = "default_true")]
    allow_critical_bypass: bool,
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
                request.endpoint_ids,
                request.quiet_hours,
                request.allow_critical_bypass,
            )
            .await?,
    ))
}
