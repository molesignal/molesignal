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
    app::{
        iam::IamContext,
        synthetics::{CreateMonitorInput, MissingRuns},
    },
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
        .route("/synthetics/monitors/{monitorId}", get(get_one))
        .route(
            "/synthetics/monitors/{monitorId}/revisions",
            post(create_revision),
        )
        .route(
            "/synthetics/monitors/{monitorId}/revisions/{revisionId}/test",
            post(test_revision),
        )
        .route(
            "/synthetics/monitors/{monitorId}/revisions/{revisionId}/publish",
            post(publish_revision),
        )
        .route("/synthetics/monitors/{monitorId}/run", post(run_monitor))
        .route("/synthetics/monitors/{monitorId}/pause", post(pause))
        .route("/synthetics/monitors/{monitorId}/resume", post(resume))
        .route("/synthetics/monitors/{monitorId}/archive", post(archive))
        .route(
            "/synthetics/monitors/{monitorId}/results",
            get(list_results),
        )
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MonitorDetail {
    monitor: SyntheticMonitor,
    revisions: Vec<MonitorRevision>,
    /// Where the published revision is missing runs.
    missing_runs: MissingRuns,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResultListQuery {
    before_time: Option<i64>,
    #[serde(default = "default_result_limit")]
    limit: u32,
}

const fn default_result_limit() -> u32 {
    100
}

#[permission("synthetics.monitors.read")]
async fn list(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
) -> Result<Json<Vec<SyntheticMonitor>>> {
    Ok(Json(state.synthetics.list_monitors(&context.org_id).await?))
}

#[permission("synthetics.monitors.read")]
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

    let missing_runs = state
        .synthetics
        .missing_runs(&context.org_id, &monitor_id)
        .await?;

    Ok(Json(MonitorDetail {
        monitor,
        revisions,
        missing_runs,
    }))
}

#[permission("synthetics.monitors.manage")]
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

#[permission("synthetics.monitors.manage")]
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

#[permission("synthetics.monitors.manage")]
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

#[permission("synthetics.monitors.manage")]
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

#[permission("synthetics.monitors.run")]
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

#[permission("synthetics.monitors.manage")]
async fn pause(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(monitor_id): Path<String>,
) -> Result<Json<SyntheticMonitor>> {
    lifecycle(&state, &context, Id(monitor_id), MonitorLifecycle::Paused).await
}

#[permission("synthetics.monitors.manage")]
async fn resume(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(monitor_id): Path<String>,
) -> Result<Json<SyntheticMonitor>> {
    lifecycle(&state, &context, Id(monitor_id), MonitorLifecycle::Active).await
}

#[permission("synthetics.monitors.archive")]
async fn archive(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(monitor_id): Path<String>,
) -> Result<Json<SyntheticMonitor>> {
    lifecycle(&state, &context, Id(monitor_id), MonitorLifecycle::Archived).await
}

#[permission("synthetics.results.read")]
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
                query.before_time.map(TimestampMicros),
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
            "revisionId": revision.map(|value| value.id.as_str()),
            "revisionNumber": revision.map(|value| value.number),
        }),
    )
    .await;
}
