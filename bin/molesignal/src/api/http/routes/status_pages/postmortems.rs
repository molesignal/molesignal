// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The public postmortem of an incident: drafted by those who manage the page,
//! published, revised or taken back by those who publish to it.

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{post, put},
};
use serde::Deserialize;

use crate::{
    api::{AppState, http::routes::activity_audit},
    app::iam::IamContext,
    domain::{
        iam::permission,
        status_page::{PostmortemWrite, StatusPageIncident, StatusPagePublicationState},
    },
    shared::{Error, Result, ids::Id},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/status-pages/{page_id}/incidents/{event_id}/postmortem",
            put(save_draft).delete(delete_draft),
        )
        .route(
            "/status-pages/{page_id}/incidents/{event_id}/postmortem/publish",
            post(publish),
        )
        .route(
            "/status-pages/{page_id}/incidents/{event_id}/postmortem/unpublish",
            post(unpublish),
        )
}

#[derive(Debug, Deserialize)]
struct DraftRequest {
    body: String,
}

#[derive(Debug, Deserialize)]
struct PublishRequest {
    body: String,
    /// Whether subscribers are told. A first publication usually tells them, a
    /// revision usually does not.
    #[serde(default = "notify_by_default")]
    notify: bool,
}

#[permission("status_pages.manage")]
async fn save_draft(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((page_id, event_id)): Path<(String, String)>,
    Json(request): Json<DraftRequest>,
) -> Result<Json<StatusPageIncident>> {
    let (page_id, event_id) = (Id(page_id), Id(event_id));

    let existing = state
        .status_pages
        .get_event(&context.org_id, &page_id, &event_id)
        .await?;

    // Saving a draft never takes a published postmortem off the page.
    if is_published(&existing) {
        return Err(Error::conflict(
            "the postmortem is published; revise it or take it back to a draft",
        ));
    }

    let event = state
        .status_pages
        .save_postmortem(
            &context.org_id,
            &page_id,
            &event_id,
            &request.body,
            PostmortemWrite::Draft,
            &context.user_id,
        )
        .await?;

    record_postmortem_audit(&state, &context, &event, "draft", None).await;
    Ok(Json(event))
}

#[permission("status_pages.publish")]
async fn publish(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((page_id, event_id)): Path<(String, String)>,
    Json(request): Json<PublishRequest>,
) -> Result<Json<StatusPageIncident>> {
    let event = state
        .status_pages
        .save_postmortem(
            &context.org_id,
            &Id(page_id),
            &Id(event_id),
            &request.body,
            PostmortemWrite::Publish {
                notify_subscribers: request.notify,
            },
            &context.user_id,
        )
        .await?;

    record_postmortem_audit(&state, &context, &event, "publish", Some(request.notify)).await;
    Ok(Json(event))
}

#[permission("status_pages.publish")]
async fn unpublish(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((page_id, event_id)): Path<(String, String)>,
) -> Result<Json<StatusPageIncident>> {
    let (page_id, event_id) = (Id(page_id), Id(event_id));

    let existing = state
        .status_pages
        .get_event(&context.org_id, &page_id, &event_id)
        .await?;

    let body = existing
        .postmortem
        .as_ref()
        .filter(|_| is_published(&existing))
        .map(|postmortem| postmortem.body.clone())
        .ok_or_else(|| Error::conflict("the postmortem is not published"))?;

    let event = state
        .status_pages
        .save_postmortem(
            &context.org_id,
            &page_id,
            &event_id,
            &body,
            PostmortemWrite::Draft,
            &context.user_id,
        )
        .await?;

    record_postmortem_audit(&state, &context, &event, "unpublish", None).await;
    Ok(Json(event))
}

#[permission("status_pages.manage")]
async fn delete_draft(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((page_id, event_id)): Path<(String, String)>,
) -> Result<Json<StatusPageIncident>> {
    let event = state
        .status_pages
        .delete_postmortem_draft(&context.org_id, &Id(page_id), &Id(event_id))
        .await?;

    record_postmortem_audit(&state, &context, &event, "delete", None).await;
    Ok(Json(event))
}

fn is_published(event: &StatusPageIncident) -> bool {
    event.postmortem.as_ref().is_some_and(|postmortem| {
        postmortem.publication_state == StatusPagePublicationState::Published
    })
}

async fn record_postmortem_audit(
    state: &AppState,
    context: &IamContext,
    event: &StatusPageIncident,
    verb: &str,
    notified: Option<bool>,
) {
    activity_audit::record(
        state,
        context,
        &format!("status_page.postmortem.{verb}"),
        "status_page_event",
        event.id.as_str(),
        serde_json::json!({
            "status_page_id": event.status_page_id,
            "notified_subscribers": notified,
        }),
    )
    .await;
}

fn notify_by_default() -> bool {
    true
}
