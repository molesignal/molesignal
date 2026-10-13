// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 定时日志巡检的 HTTP 接口：保存、启停、手动执行，以及读取每次执行的报告。
//!
//! 报告里引用日志，执行又由 Mole Agent 完成，所以读取需要 `agent.use` 和 `streams.query` 两项；
//! 保存、启停、删除和手动执行需要 `inspections.manage`。保存者成为巡检的执行身份，自己还得
//! 持有执行所需的两项权限，这一条由 `LogInspectionService` 核对。

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};

use self::model::{InspectionResp, LimitsResp, RunResp, SaveReq, profiles};
use crate::{
    api::{AppState, http::middleware::Permission},
    app::iam::IamContext,
    domain::{iam::permission, log_inspection::LogInspection},
    shared::{Result, ids::Id, time::TimestampMicros},
};

mod model;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/logs/inspections", get(list).post(create))
        .route("/logs/inspections/limits", get(limits))
        .route(
            "/logs/inspections/{id}",
            get(get_one).put(update).delete(delete),
        )
        .route("/logs/inspections/{id}/enabled", post(set_enabled))
        .route("/logs/inspections/{id}/run", post(run_now))
        .route("/logs/inspections/{id}/runs", get(list_runs))
        .route("/logs/inspections/{id}/runs/{run_id}", get(get_run))
}

/// 读巡检和报告的第二项权限；第一项 `agent.use` 在路由上声明。
fn require_log_access(ctx: &IamContext) -> Result<()> {
    Permission::require_key(ctx, "streams.query")
}

/// 一条巡检连同它此刻有没有在执行、以谁的身份执行。
async fn respond(state: &AppState, inspection: LogInspection) -> Result<Json<InspectionResp>> {
    let active = state
        .platform
        .log_inspections
        .active_runs(&inspection.org_id)
        .await?;

    let users = profiles(state, [&inspection.run_as_user_id]).await;

    let running = active.iter().find(|run| run.inspection_id == inspection.id);

    Ok(Json(InspectionResp::new(inspection, running, &users)))
}

#[permission("agent.use")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<InspectionResp>>> {
    require_log_access(&ctx)?;

    let service = &state.platform.log_inspections;
    let inspections = service.list(&ctx.org_id).await?;
    let active = service.active_runs(&ctx.org_id).await?;

    let users = profiles(
        &state,
        inspections
            .iter()
            .map(|inspection| &inspection.run_as_user_id),
    )
    .await;

    Ok(Json(
        inspections
            .into_iter()
            .map(|inspection| {
                let running = active.iter().find(|run| run.inspection_id == inspection.id);

                InspectionResp::new(inspection, running, &users)
            })
            .collect(),
    ))
}

#[permission("agent.use")]
async fn limits(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<LimitsResp>> {
    require_log_access(&ctx)?;

    Ok(Json(LimitsResp::from(
        state.platform.log_inspections.limits(),
    )))
}

#[permission("agent.use")]
async fn get_one(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<InspectionResp>> {
    require_log_access(&ctx)?;

    let inspection = state
        .platform
        .log_inspections
        .get(&ctx.org_id, &Id(id))
        .await?;

    respond(&state, inspection).await
}

#[permission("inspections.manage")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<SaveReq>,
) -> Result<Json<InspectionResp>> {
    let inspection = state
        .platform
        .log_inspections
        .create(&ctx, req.into())
        .await?;

    respond(&state, inspection).await
}

#[permission("inspections.manage")]
async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<SaveReq>,
) -> Result<Json<InspectionResp>> {
    let inspection = state
        .platform
        .log_inspections
        .update(&ctx, &Id(id), req.into())
        .await?;

    respond(&state, inspection).await
}

#[derive(Debug, Deserialize)]
struct EnabledReq {
    enabled: bool,
}

#[permission("inspections.manage")]
async fn set_enabled(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<EnabledReq>,
) -> Result<Json<InspectionResp>> {
    let inspection = state
        .platform
        .log_inspections
        .set_enabled(&ctx.org_id, &Id(id), req.enabled)
        .await?;

    respond(&state, inspection).await
}

#[permission("inspections.manage")]
async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    state
        .platform
        .log_inspections
        .delete(&ctx.org_id, &Id(id))
        .await?;

    Ok(Json(json!({ "deleted": true })))
}

#[permission("inspections.manage")]
async fn run_now(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<RunResp>> {
    let run = state
        .platform
        .log_inspections
        .run_now(&ctx, &Id(id))
        .await?;

    let users = profiles(&state, run.requested_by.iter()).await;

    Ok(Json(RunResp::new(run, &users)))
}

#[derive(Debug, Deserialize)]
struct RunsQuery {
    /// 只要创建时间早于它的，用上一页最后一条的 `created_at_micros` 翻页。
    #[serde(default)]
    before: Option<i64>,
    #[serde(default = "default_runs_limit")]
    limit: i64,
}

fn default_runs_limit() -> i64 {
    50
}

#[permission("agent.use")]
async fn list_runs(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Query(query): Query<RunsQuery>,
) -> Result<Json<Vec<RunResp>>> {
    require_log_access(&ctx)?;

    let runs = state
        .platform
        .log_inspections
        .list_runs(
            &ctx.org_id,
            &Id(id),
            query.before.map(TimestampMicros),
            query.limit,
        )
        .await?;

    let users = profiles(
        &state,
        runs.iter().filter_map(|run| run.requested_by.as_ref()),
    )
    .await;

    Ok(Json(
        runs.into_iter()
            .map(|run| RunResp::new(run, &users))
            .collect(),
    ))
}

#[permission("agent.use")]
async fn get_run(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path((id, run_id)): Path<(String, String)>,
) -> Result<Json<RunResp>> {
    require_log_access(&ctx)?;

    let run = state
        .platform
        .log_inspections
        .get_run(&ctx.org_id, &Id(id), &Id(run_id))
        .await?;

    let users = profiles(&state, run.requested_by.iter()).await;

    Ok(Json(RunResp::new(run, &users)))
}
