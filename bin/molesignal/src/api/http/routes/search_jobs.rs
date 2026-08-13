// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Search Jobs HTTP routes（spec search-jobs）。
//!
//! 提供提交、状态、分页结果、取消、重试与删除；后台 worker 将结果写为 NDJSON。

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{iam::permission, query::QueryRequest},
    infra::persistence::repositories::search::jobs::SearchJob,
    shared::{Result, ids::Id},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/query/jobs", post(submit).get(list))
        .route("/query/jobs/{id}", get(get_one).delete(delete))
        .route("/query/jobs/{id}/results", get(results))
        .route("/query/jobs/{id}/cancel", post(cancel))
        .route("/query/jobs/{id}/retry", post(retry))
}

#[derive(Debug, Deserialize)]
pub struct SubmitReq {
    #[serde(flatten)]
    pub request: QueryRequest,
    #[serde(default)]
    pub ttl_secs: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct JobResp {
    pub job_id: String,
    pub state: String,
    pub attempt: i32,
    pub submitted_at_micros: i64,
    pub started_at_micros: Option<i64>,
    pub finished_at_micros: Option<i64>,
    pub result_object_key: Option<String>,
    pub result_rows: Option<i64>,
    pub error: Option<String>,
    pub expires_at_micros: i64,
}

fn to_resp(j: SearchJob) -> JobResp {
    JobResp {
        job_id: j.id.0,
        state: j.state.as_str().to_string(),
        attempt: j.attempt,
        submitted_at_micros: j.submitted_at.0,
        started_at_micros: j.started_at.map(|t| t.0),
        finished_at_micros: j.finished_at.map(|t| t.0),
        result_object_key: j.result_object_key,
        result_rows: j.result_rows,
        error: j.error,
        expires_at_micros: j.expires_at.0,
    }
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn submit(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<SubmitReq>,
) -> Result<Json<JobResp>> {
    let job = state
        .search_jobs
        .submit(
            ctx.org_id.clone(),
            ctx.user_id.clone(),
            ctx.organization_role_key().to_string(),
            req.request,
            req.ttl_secs,
        )
        .await?;
    Ok(Json(to_resp(job)))
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
}
fn default_limit() -> i64 {
    100
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(p): Query<ListQuery>,
) -> Result<Json<Vec<JobResp>>> {
    let jobs = state.search_jobs.list(&ctx.org_id, p.limit).await?;
    Ok(Json(jobs.into_iter().map(to_resp).collect()))
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn get_one(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<JobResp>> {
    let job = state.search_jobs.get(&ctx.org_id, &Id(id)).await?;
    Ok(Json(to_resp(job)))
}

#[derive(Debug, Deserialize)]
pub struct ResultParams {
    #[serde(default = "default_page")]
    pub page: i64,
    #[serde(default = "default_page_size")]
    pub page_size: i64,
}
fn default_page() -> i64 {
    1
}
fn default_page_size() -> i64 {
    1000
}

#[derive(Debug, Serialize)]
pub struct ResultResp {
    pub state: String,
    pub result_rows: Option<i64>,
    pub page: i64,
    pub page_size: i64,
    pub rows: Vec<Value>,
    pub has_more: bool,
    pub error: Option<String>,
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn results(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Query(p): Query<ResultParams>,
) -> Result<Json<ResultResp>> {
    let result = state
        .search_jobs
        .results(&ctx.org_id, &Id(id), p.page, p.page_size)
        .await?;
    Ok(Json(ResultResp {
        state: result.state.as_str().to_string(),
        result_rows: result.result_rows,
        page: result.page,
        page_size: result.page_size,
        rows: result.rows,
        has_more: result.has_more,
        error: result.error,
    }))
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn cancel(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<JobResp>> {
    let id = Id(id);
    let job = state.search_jobs.cancel(&ctx.org_id, &id).await?;
    Ok(Json(to_resp(job)))
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn retry(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<JobResp>> {
    let id = Id(id);
    let job = state.search_jobs.retry(&ctx.org_id, &id).await?;
    Ok(Json(to_resp(job)))
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    state.search_jobs.delete(&ctx.org_id, &Id(id)).await?;
    Ok(Json(serde_json::json!({"deleted": true})))
}
