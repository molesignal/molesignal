// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::post,
};
use domain::{
    alerting::rule::AlertRule,
    iam::{IamContext, resource_permission},
};
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde_json::Value;

use crate::api::AppState;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/alerts/rules/{id}/test", post(test_rule))
        .route("/alerts/rules/{id}/trigger", post(trigger_rule))
}

#[resource_permission(
    action = "alerts.rules.manage",
    resource = AlertRule,
    id = Id::from_string(id),
    bind = rule
)]
async fn test_rule(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let result = state
        .alerting
        .evaluator
        .test_rule(&rule, TimestampMicros::now())
        .await?;

    Ok(Json(serde_json::to_value(result).map_err(|error| {
        Error::internal(format!("serialize alert test: {error}"))
    })?))
}

#[resource_permission(
    action = "alerts.rules.manage",
    resource = AlertRule,
    id = Id::from_string(id),
    bind = rule
)]
async fn trigger_rule(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let incident = state
        .alerting
        .evaluator
        .trigger_rule(&rule, &context.user_id, TimestampMicros::now())
        .await?;

    Ok(Json(serde_json::to_value(incident).map_err(|error| {
        Error::internal(format!("serialize incident: {error}"))
    })?))
}
