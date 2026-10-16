// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A subscriber reads and changes what it hears about, through the link its
//! messages carry. The token is sent in the body, never in the URL.

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::post,
};
use serde::{Deserialize, Serialize};

use super::mask_target;
use crate::{
    api::AppState,
    domain::status_page::{
        StatusPageSubscriber, StatusPageSubscriberChannel, StatusPageSubscriberStatus,
        SubscriberComponentScope,
    },
    shared::{Result, ids::Id},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/v1/public/status-pages/{slug}/subscriptions/preferences",
        post(read).put(update),
    )
}

#[derive(Debug, Deserialize)]
struct ReadRequest {
    token: String,
}

#[derive(Debug, Deserialize)]
struct UpdateRequest {
    token: String,
    /// The components to hear about; none means the whole page.
    #[serde(default)]
    component_ids: Vec<Id>,
}

#[derive(Debug, Serialize)]
struct PreferencesResponse {
    channel: StatusPageSubscriberChannel,
    masked_target: String,
    target_label: Option<String>,
    status: StatusPageSubscriberStatus,
    component_scope: SubscriberComponentScope,
    component_ids: Vec<Id>,
}

impl From<StatusPageSubscriber> for PreferencesResponse {
    fn from(subscriber: StatusPageSubscriber) -> Self {
        Self {
            channel: subscriber.channel,
            masked_target: mask_target(subscriber.channel, &subscriber.target),
            target_label: subscriber.target_label,
            status: subscriber.status,
            component_scope: subscriber.components.scope,
            component_ids: subscriber.components.component_ids,
        }
    }
}

async fn read(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Json(request): Json<ReadRequest>,
) -> Result<Json<PreferencesResponse>> {
    Ok(Json(
        state
            .status_pages
            .subscription_preferences(&slug, &request.token)
            .await?
            .into(),
    ))
}

async fn update(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Json(request): Json<UpdateRequest>,
) -> Result<Json<PreferencesResponse>> {
    Ok(Json(
        state
            .status_pages
            .update_subscription_preferences(&slug, &request.token, request.component_ids)
            .await?
            .into(),
    ))
}
