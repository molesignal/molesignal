// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::{
    api::{AppState, http::routes::activity_audit},
    app::{iam::IamContext, synthetics::CreateMonitorInput},
    domain::{
        iam::permission,
        synthetics::{
            ActiveMonitorRevision, MonitorLifecycle, MonitorRevision, ProbeTask, SyntheticMonitor,
            SyntheticResult,
        },
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/synthetics/monitors", get(list).post(create))
        .route("/synthetics/monitors/{monitor_id}", get(get_one))
        .route(
            "/synthetics/monitors/{monitor_id}/revisions",
            post(create_revision),
        )
        .route(
            "/synthetics/monitors/{monitor_id}/revisions/{revision_id}/test",
            post(test_revision),
        )
        .route(
            "/synthetics/monitors/{monitor_id}/revisions/{revision_id}/publish",
            post(publish_revision),
        )
        .route("/synthetics/monitors/{monitor_id}/run", post(run_monitor))
        .route("/synthetics/monitors/{monitor_id}/pause", post(pause))
        .route("/synthetics/monitors/{monitor_id}/resume", post(resume))
        .route("/synthetics/monitors/{monitor_id}/archive", post(archive))
        .route(
            "/synthetics/monitors/{monitor_id}/results",
            get(list_results),
        )
}

#[derive(Serialize)]
struct MonitorDetail {
    monitor: SyntheticMonitor,
    revisions: Vec<MonitorRevision>,
}

#[derive(Debug, Deserialize)]
struct ResultListQuery {
    before_micros: Option<i64>,
    #[serde(default = "default_result_limit")]
    limit: u32,
}

const fn default_result_limit() -> u32 {
    100
}

#[permission("synthetics.read")]
async fn list(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
) -> Result<Json<Vec<SyntheticMonitor>>> {
    Ok(Json(state.synthetics.list_monitors(&context.org_id).await?))
}

#[permission("synthetics.read")]
async fn get_one(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(monitor_id): Path<String>,
) -> Result<Json<MonitorDetail>> {
    let monitor_id = Id(monitor_id);
    let monitor = state
        .synthetics
        .get_monitor(&context.org_id, &monitor_id)
        .await?;
    let revisions = state
        .synthetics
        .list_revisions(&context.org_id, &monitor_id)
        .await?;
    Ok(Json(MonitorDetail { monitor, revisions }))
}

#[permission("synthetics.manage")]
async fn create(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Json(input): Json<CreateMonitorInput>,
) -> Result<(StatusCode, Json<ActiveMonitorRevision>)> {
    let created = state
        .synthetics
        .create_monitor(&context.org_id, &context.user_id, input)
        .await?;
    audit(&state, &context, &created.monitor, "create", None).await;
    Ok((StatusCode::CREATED, Json(created)))
}

#[permission("synthetics.manage")]
async fn create_revision(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(monitor_id): Path<String>,
    Json(input): Json<CreateMonitorInput>,
) -> Result<(StatusCode, Json<MonitorRevision>)> {
    let monitor_id = Id(monitor_id);
    let revision = state
        .synthetics
        .create_draft_revision(&context.org_id, &context.user_id, &monitor_id, input)
        .await?;
    let monitor = state
        .synthetics
        .get_monitor(&context.org_id, &monitor_id)
        .await?;
    audit(
        &state,
        &context,
        &monitor,
        "revision.create",
        Some(&revision),
    )
    .await;
    Ok((StatusCode::CREATED, Json(revision)))
}

#[permission("synthetics.manage")]
async fn test_revision(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((monitor_id, revision_id)): Path<(String, String)>,
) -> Result<(StatusCode, Json<Vec<ProbeTask>>)> {
    let tasks = state
        .synthetics
        .create_test_run(&context.org_id, &Id(monitor_id), &Id(revision_id))
        .await?;
    Ok((StatusCode::ACCEPTED, Json(tasks)))
}

#[permission("synthetics.manage")]
async fn publish_revision(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((monitor_id, revision_id)): Path<(String, String)>,
) -> Result<Json<ActiveMonitorRevision>> {
    let published = state
        .synthetics
        .publish_revision(&context.org_id, &Id(monitor_id), &Id(revision_id))
        .await?;
    audit(
        &state,
        &context,
        &published.monitor,
        "revision.publish",
        Some(&published.revision),
    )
    .await;
    Ok(Json(published))
}

#[permission("synthetics.manage")]
async fn run_monitor(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(monitor_id): Path<String>,
) -> Result<(StatusCode, Json<Vec<ProbeTask>>)> {
    let tasks = state
        .synthetics
        .run_monitor(&context.org_id, &Id(monitor_id))
        .await?;
    Ok((StatusCode::ACCEPTED, Json(tasks)))
}

#[permission("synthetics.manage")]
async fn pause(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(monitor_id): Path<String>,
) -> Result<Json<SyntheticMonitor>> {
    lifecycle(&state, &context, Id(monitor_id), MonitorLifecycle::Paused).await
}

#[permission("synthetics.manage")]
async fn resume(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(monitor_id): Path<String>,
) -> Result<Json<SyntheticMonitor>> {
    lifecycle(&state, &context, Id(monitor_id), MonitorLifecycle::Active).await
}

#[permission("synthetics.manage")]
async fn archive(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(monitor_id): Path<String>,
) -> Result<Json<SyntheticMonitor>> {
    lifecycle(&state, &context, Id(monitor_id), MonitorLifecycle::Archived).await
}

#[permission("synthetics.read")]
async fn list_results(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(monitor_id): Path<String>,
    Query(query): Query<ResultListQuery>,
) -> Result<Json<Vec<SyntheticResult>>> {
    Ok(Json(
        state
            .synthetics
            .list_results(
                &context.org_id,
                &Id(monitor_id),
                query.before_micros.map(TimestampMicros),
                query.limit,
            )
            .await?,
    ))
}

async fn lifecycle(
    state: &AppState,
    context: &IamContext,
    monitor_id: Id,
    lifecycle: MonitorLifecycle,
) -> Result<Json<SyntheticMonitor>> {
    let monitor = state
        .synthetics
        .set_lifecycle(&context.org_id, &monitor_id, lifecycle)
        .await?;
    audit(state, context, &monitor, lifecycle.as_str(), None).await;
    Ok(Json(monitor))
}

async fn audit(
    state: &AppState,
    context: &IamContext,
    monitor: &SyntheticMonitor,
    verb: &str,
    revision: Option<&MonitorRevision>,
) {
    activity_audit::record(
        state,
        context,
        &format!("synthetic.monitor.{verb}"),
        "synthetic_monitor",
        monitor.id.as_str(),
        serde_json::json!({
            "kind": monitor.kind,
            "lifecycle": monitor.lifecycle,
            "revision_id": revision.map(|value| value.id.as_str()),
            "revision_number": revision.map(|value| value.number),
        }),
    )
    .await;
}
