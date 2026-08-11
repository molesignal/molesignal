// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::Deserialize;

use crate::{
    api::{AppState, http::routes::activity_audit},
    app::{
        iam::IamContext,
        status_page::{StatusPageIncidentInput, StatusPageIncidentUpdateInput},
    },
    domain::{
        iam::permission,
        status_page::{
            IncidentImpact, PublicIncidentStatus, StatusPageEventList, StatusPageEventView,
            StatusPageHistoryPage, StatusPageHistoryQuery, StatusPageIncident,
            StatusPageIncidentKind, StatusPagePublicationState,
        },
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/status-pages/{page_id}/incidents",
            get(list_events).post(create_event),
        )
        .route(
            "/status-pages/{page_id}/incidents/{event_id}",
            get(get_event).put(update_event),
        )
        .route(
            "/status-pages/{page_id}/incidents/{event_id}/publish",
            post(publish_draft),
        )
        .route(
            "/status-pages/{page_id}/incidents/{event_id}/updates",
            post(append_update),
        )
        .route("/status-pages/{page_id}/history", get(history))
}

#[derive(Debug, Deserialize)]
struct EventListQuery {
    #[serde(default = "default_kind")]
    kind: StatusPageIncidentKind,
    view: Option<StatusPageEventView>,
}

#[derive(Debug, Deserialize)]
struct EventWriteRequest {
    #[serde(default)]
    source_incident_id: Option<Id>,
    #[serde(default = "default_kind")]
    kind: StatusPageIncidentKind,
    title: String,
    #[serde(default = "default_impact")]
    impact: IncidentImpact,
    status: Option<PublicIncidentStatus>,
    #[serde(default = "default_publication_state")]
    publication_state: StatusPagePublicationState,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    component_ids: Vec<Id>,
    #[serde(default)]
    started_at: Option<TimestampMicros>,
}

impl EventWriteRequest {
    fn into_input(self) -> StatusPageIncidentInput {
        let status = self.status.unwrap_or(match self.kind {
            StatusPageIncidentKind::Incident => PublicIncidentStatus::Investigating,
            StatusPageIncidentKind::Maintenance => PublicIncidentStatus::Scheduled,
        });
        StatusPageIncidentInput {
            source_incident_id: self.source_incident_id,
            kind: self.kind,
            title: self.title,
            impact: self.impact,
            status,
            publication_state: self.publication_state,
            message: self.message,
            component_ids: self.component_ids,
            started_at: self.started_at,
        }
    }
}

#[derive(Debug, Deserialize)]
struct EventUpdateRequest {
    status: PublicIncidentStatus,
    message: String,
}

#[derive(Debug, Deserialize)]
struct HistoryQuery {
    #[serde(alias = "type")]
    kind: Option<StatusPageIncidentKind>,
    status: Option<PublicIncidentStatus>,
    #[serde(alias = "component")]
    component_id: Option<Id>,
    from: Option<TimestampMicros>,
    to: Option<TimestampMicros>,
    #[serde(alias = "q")]
    search: Option<String>,
    #[serde(default = "default_page")]
    page: u32,
}

#[permission("status_pages.read")]
async fn list_events(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(page_id): Path<String>,
    Query(query): Query<EventListQuery>,
) -> Result<Json<StatusPageEventList>> {
    let view = query.view.unwrap_or(match query.kind {
        StatusPageIncidentKind::Incident => StatusPageEventView::Current,
        StatusPageIncidentKind::Maintenance => StatusPageEventView::Upcoming,
    });
    Ok(Json(
        state
            .status_pages
            .list_events(&context.org_id, &Id(page_id), query.kind, view)
            .await?,
    ))
}

#[permission("status_pages.read")]
async fn get_event(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((page_id, event_id)): Path<(String, String)>,
) -> Result<Json<StatusPageIncident>> {
    Ok(Json(
        state
            .status_pages
            .get_event(&context.org_id, &Id(page_id), &Id(event_id))
            .await?,
    ))
}

#[permission("status_pages.manage")]
async fn create_event(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(page_id): Path<String>,
    Json(request): Json<EventWriteRequest>,
) -> Result<(StatusCode, Json<StatusPageIncident>)> {
    let page_id = Id(page_id);
    let event = state
        .status_pages
        .create_incident(&context.org_id, &page_id, request.into_input())
        .await?;
    record_event_audit(&state, &context, &event, "create").await;
    Ok((StatusCode::CREATED, Json(event)))
}

#[permission("status_pages.manage")]
async fn update_event(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((page_id, event_id)): Path<(String, String)>,
    Json(request): Json<EventWriteRequest>,
) -> Result<Json<StatusPageIncident>> {
    let page_id = Id(page_id);
    let event_id = Id(event_id);
    let existing = state
        .status_pages
        .get_event(&context.org_id, &page_id, &event_id)
        .await?;
    let (event, verb) = if existing.publication_state == StatusPagePublicationState::Draft {
        (
            state
                .status_pages
                .update_incident_draft(&context.org_id, &page_id, &event_id, request.into_input())
                .await?,
            "draft_update",
        )
    } else {
        (
            state
                .status_pages
                .update_scheduled_maintenance(
                    &context.org_id,
                    &page_id,
                    &event_id,
                    request.into_input(),
                )
                .await?,
            "reschedule",
        )
    };
    record_event_audit(&state, &context, &event, verb).await;
    Ok(Json(event))
}

#[permission("status_pages.manage")]
async fn publish_draft(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((page_id, event_id)): Path<(String, String)>,
    Json(request): Json<EventWriteRequest>,
) -> Result<Json<StatusPageIncident>> {
    let event = state
        .status_pages
        .publish_incident_draft(
            &context.org_id,
            &Id(page_id),
            &Id(event_id),
            request.into_input(),
        )
        .await?;
    record_event_audit(&state, &context, &event, "publish").await;
    Ok(Json(event))
}

#[permission("status_pages.manage")]
async fn append_update(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((page_id, event_id)): Path<(String, String)>,
    Json(request): Json<EventUpdateRequest>,
) -> Result<Json<StatusPageIncident>> {
    let event = state
        .status_pages
        .append_incident_update(
            &context.org_id,
            &Id(page_id),
            &Id(event_id),
            StatusPageIncidentUpdateInput {
                status: request.status,
                message: request.message,
            },
        )
        .await?;
    record_event_audit(&state, &context, &event, "update").await;
    Ok(Json(event))
}

#[permission("status_pages.read")]
async fn history(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(page_id): Path<String>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<StatusPageHistoryPage>> {
    Ok(Json(
        state
            .status_pages
            .query_history(
                &context.org_id,
                &Id(page_id),
                StatusPageHistoryQuery {
                    kind: query.kind,
                    status: query.status,
                    component_id: query.component_id,
                    from: query.from,
                    to: query.to,
                    search: query.search,
                    page: query.page,
                    per_page: 25,
                },
            )
            .await?,
    ))
}

async fn record_event_audit(
    state: &AppState,
    context: &IamContext,
    event: &StatusPageIncident,
    verb: &str,
) {
    activity_audit::record(
        state,
        context,
        &format!("status_page.event.{verb}"),
        "status_page_event",
        event.id.as_str(),
        serde_json::json!({
            "status_page_id": event.status_page_id,
            "kind": event.kind,
            "status": event.status,
            "publication_state": event.publication_state,
            "affected_component_count": event.component_ids.len(),
            "has_source_incident": event.source_incident_id.is_some(),
        }),
    )
    .await;
}

fn default_kind() -> StatusPageIncidentKind {
    StatusPageIncidentKind::Incident
}

fn default_impact() -> IncidentImpact {
    IncidentImpact::Minor
}

fn default_publication_state() -> StatusPagePublicationState {
    StatusPagePublicationState::Published
}

fn default_page() -> u32 {
    1
}
