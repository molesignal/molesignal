// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Search Jobs HTTP routes（spec search-jobs）。
//!
//! 当前实装范围：CRUD + submission；真正的后台 worker（claim_next_pending + 跑
//! QueryService::run + 写 Parquet）作为 follow-up（design D3）。本 handler 已建
//! row，state=pending；worker 上线后会自动 pickup。

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{iam::permission, query::QueryRequest},
    infra::persistence::repositories::search::jobs::{SearchJob, SearchJobState},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const DEFAULT_TTL_SECS: i64 = 7 * 86400;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/query/jobs", post(submit).get(list))
        .route("/query/jobs/{id}", get(get_one).delete(delete))
        .route("/query/jobs/{id}/results", get(results))
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
    Json(mut req): Json<SubmitReq>,
) -> Result<Json<JobResp>> {
    req.request.org_id = ctx.org_id.clone();
    let now = TimestampMicros::now();
    let ttl = req
        .ttl_secs
        .unwrap_or(DEFAULT_TTL_SECS)
        .clamp(60, 30 * 86400);
    let job = SearchJob {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        user_id: ctx.user_id.clone(),
        request_json: serde_json::to_value(&req.request)
            .map_err(|e| Error::internal(format!("request json: {e}")))?,
        trace_link: crate::shared::trace_context::current_trace_context()
            .map(|context| context.serialized_link()),
        state: SearchJobState::Pending,
        result_object_key: None,
        result_rows: None,
        error: None,
        submitted_at: now,
        started_at: None,
        finished_at: None,
        expires_at: TimestampMicros(now.0 + ttl * 1_000_000),
    };
    let job = state.storage.search_jobs.create(job).await?;
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
    let jobs = state
        .storage
        .search_jobs
        .list(&ctx.org_id, p.limit.clamp(1, 1000))
        .await?;
    Ok(Json(jobs.into_iter().map(to_resp).collect()))
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn get_one(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<JobResp>> {
    let job = state.storage.search_jobs.get(&ctx.org_id, &Id(id)).await?;
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
    /// state == done 时填 result_object_key；调用方按 object_store 路径下载并自行解 Parquet。
    /// （当前暂未实装服务端流式 Parquet → JSON 解码；那需要 DataFusion 读端依赖。）
    pub result_object_key: Option<String>,
    pub result_rows: Option<i64>,
    pub page: i64,
    pub page_size: i64,
    pub error: Option<String>,
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn results(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Query(p): Query<ResultParams>,
) -> Result<Json<ResultResp>> {
    let job = state.storage.search_jobs.get(&ctx.org_id, &Id(id)).await?;
    Ok(Json(ResultResp {
        state: job.state.as_str().to_string(),
        result_object_key: job.result_object_key,
        result_rows: job.result_rows,
        page: p.page.max(1),
        page_size: p.page_size.clamp(1, 10000),
        error: job.error,
    }))
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    // 校验 org ownership 再删
    let _ = state
        .storage
        .search_jobs
        .get(&ctx.org_id, &Id(id.clone()))
        .await?;
    state.storage.search_jobs.delete(&Id(id)).await?;
    Ok(Json(serde_json::json!({"deleted": true})))
}
