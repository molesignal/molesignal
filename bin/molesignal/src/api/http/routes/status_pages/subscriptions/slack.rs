// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! "Add to Slack": the page sends the visitor to Slack, and Slack sends them
//! back to the fixed callback of this installation, which sends them on to
//! the page. A page offers it once its settings name a Slack App connector.

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    http::{
        HeaderValue, StatusCode,
        header::{CACHE_CONTROL, LOCATION},
    },
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::{
    api::{AppState, http::routes::activity_audit},
    app::{iam::IamContext, status_page::StatusPageSlackAppSetting},
    domain::iam::permission,
    infra::status_page::SLACK_CALLBACK_PATH,
    shared::{Error, Result, ids::Id},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/public/status-pages/{slug}/subscriptions/slack",
            post(begin),
        )
        .route(SLACK_CALLBACK_PATH, get(callback))
}

pub(super) fn management_routes() -> Router<AppState> {
    Router::new().route(
        "/status-pages/{page_id}/slack-app",
        get(get_slack_app).put(set_slack_app),
    )
}

#[derive(Debug, Deserialize)]
struct SlackAppRequest {
    /// The Slack App connector to add the page with; none stops offering it.
    #[serde(default)]
    connector_id: Option<Id>,
}

#[derive(Debug, Deserialize)]
struct BeginRequest {
    /// The components to hear about; none means the whole page.
    #[serde(default)]
    component_ids: Vec<Id>,
}

#[derive(Debug, Serialize)]
struct BeginResponse {
    authorize_url: String,
}

#[derive(Debug, Deserialize)]
struct CallbackQuery {
    state: Option<String>,
    code: Option<String>,
}

#[permission("status_pages.manage")]
async fn get_slack_app(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(page_id): Path<String>,
) -> Result<Json<StatusPageSlackAppSetting>> {
    Ok(Json(
        state
            .status_pages
            .page_slack_app_setting(&context.org_id, &Id(page_id))
            .await?,
    ))
}

#[permission("status_pages.manage")]
async fn set_slack_app(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(page_id): Path<String>,
    Json(request): Json<SlackAppRequest>,
) -> Result<Json<StatusPageSlackAppSetting>> {
    let page_id = Id(page_id);

    let setting = state
        .status_pages
        .set_page_slack_app(&context.org_id, &page_id, request.connector_id)
        .await?;

    activity_audit::record(
        &state,
        &context,
        "status_page.slack_app.update",
        "status_page",
        page_id.as_str(),
        serde_json::json!({"connector_id": setting.connector_id}),
    )
    .await;

    Ok(Json(setting))
}

async fn begin(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Json(request): Json<BeginRequest>,
) -> Result<Json<BeginResponse>> {
    let authorize_url = state
        .status_pages
        .begin_slack_app_subscription(&slug, request.component_ids)
        .await?;

    Ok(Json(BeginResponse { authorize_url }))
}

async fn callback(
    State(state): State<AppState>,
    Query(query): Query<CallbackQuery>,
) -> Result<Response> {
    let slack_state = query
        .state
        .as_deref()
        .ok_or_else(|| Error::invalid("Slack sent back no state"))?;

    let destination = state
        .status_pages
        .complete_slack_app_subscription(slack_state, query.code.as_deref())
        .await?;

    let location = HeaderValue::from_str(&destination)
        .map_err(|_| Error::internal("the status page address is not a valid header"))?;

    let mut response = StatusCode::SEE_OTHER.into_response();
    response.headers_mut().insert(LOCATION, location);

    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));

    Ok(response)
}
