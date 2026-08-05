// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Scheduled pipelines CRUD。Admin+。
//!
//! cron 字段限于 `every:Ns/m/h`；future 跑完整 cron 解析时不破坏字段。

use std::collections::HashMap;

use axum::{
    Extension, Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::header::CONTENT_TYPE,
    response::Response,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    api::{AppState, http::middleware::ProtectedResource},
    app::iam::IamContext,
    domain::{
        iam::{permission, resource_permission},
        query::{QueryLanguage, QueryRequest, StreamHint},
    },
    infra::{
        persistence::repositories::{
            pipelines::runs::{PipelineRun, PipelineRunSummary},
            search::jobs::{SearchJob, SearchJobState},
        },
        pipeline::{
            ScheduledPipeline,
            exec::{parse_signal_type, validate_pipeline_streams},
        },
    },
    shared::{
        Error, Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

const MAX_BACKFILL_WINDOW_MICROS: i64 = 31 * 24 * 3600 * 1_000_000;
const OVERVIEW_WINDOW_MICROS: i64 = 24 * 3600 * 1_000_000;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/scheduled_pipelines", get(list).post(create))
        .route(
            "/scheduled_pipelines/{id}",
            get(get_one).put(update).delete(delete),
        )
        .route("/scheduled_pipelines/{id}/runs", get(list_runs))
        .route("/scheduled_pipelines/{id}/backfill", post(submit_backfill))
}

#[async_trait::async_trait]
impl ProtectedResource for ScheduledPipeline {
    type Id = Id;

    async fn load(state: &AppState, id: Self::Id) -> Result<Self> {
        state.storage.scheduled_pipelines.get_by_id(&id).await
    }

    fn organization_id(&self) -> &Id {
        &self.org_id
    }

    fn resource_type(&self) -> &str {
        "pipeline"
    }

    fn resource_id(&self) -> &str {
        self.id.as_str()
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateReq {
    pub name: String,
    pub source_stream: String,
    pub target_stream: String,
    pub function_steps: Value,
    pub cron: String,
    #[serde(default = "default_lookback")]
    pub lookback_secs: i32,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateReq {
    pub name: String,
    pub source_stream: String,
    pub target_stream: String,
    pub function_steps: Value,
    pub cron: String,
    pub lookback_secs: i32,
    pub enabled: bool,
}

fn update_permissions(existing: &ScheduledPipeline, request: &UpdateReq) -> Vec<&'static str> {
    let configuration_changed = existing.name != request.name
        || existing.source_stream != request.source_stream.trim()
        || existing.target_stream != request.target_stream.trim()
        || existing.function_steps != request.function_steps
        || existing.cron != request.cron
        || existing.lookback_secs != request.lookback_secs;
    let enabled_changed = existing.enabled != request.enabled;
    let mut permissions = Vec::with_capacity(2);
    if configuration_changed || !enabled_changed {
        permissions.push("pipelines.edit");
    }
    if enabled_changed {
        permissions.push("pipelines.pause");
    }
    permissions
}

fn default_lookback() -> i32 {
    300
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize)]
pub struct Resp {
    pub id: String,
    pub name: String,
    pub source_stream: String,
    pub target_stream: String,
    pub function_steps: Value,
    pub cron: String,
    pub lookback_secs: i32,
    pub enabled: bool,
    pub last_run_at_micros: Option<i64>,
    pub created_at_micros: i64,
    pub updated_at_micros: i64,
}

fn to_resp(p: ScheduledPipeline) -> Resp {
    Resp {
        id: p.id.0,
        name: p.name,
        source_stream: p.source_stream,
        target_stream: p.target_stream,
        function_steps: p.function_steps,
        cron: p.cron,
        lookback_secs: p.lookback_secs,
        enabled: p.enabled,
        last_run_at_micros: p.last_run_at.map(|t| t.0),
        created_at_micros: p.created_at.0,
        updated_at_micros: p.updated_at.0,
    }
}

#[derive(Debug, Serialize)]
pub struct ListResp {
    #[serde(flatten)]
    pub pipeline: Resp,
    pub last_run_state: Option<String>,
    pub last_run_started_at_micros: Option<i64>,
    pub last_run_finished_at_micros: Option<i64>,
    pub last_run_scanned_rows: Option<i64>,
    pub last_run_error: Option<String>,
    pub runs_24h: i64,
    pub succeeded_runs_24h: i64,
    pub failed_runs_24h: i64,
}

fn to_list_resp(pipeline: ScheduledPipeline, summary: Option<&PipelineRunSummary>) -> ListResp {
    ListResp {
        pipeline: to_resp(pipeline),
        last_run_state: summary.map(|item| item.last_state.as_str().to_string()),
        last_run_started_at_micros: summary.map(|item| item.last_started_at.0),
        last_run_finished_at_micros: summary
            .and_then(|item| item.last_finished_at.map(|time| time.0)),
        last_run_scanned_rows: summary.map(|item| item.last_scanned_rows),
        last_run_error: summary.and_then(|item| item.last_error.clone()),
        runs_24h: summary.map_or(0, |item| item.runs_in_window),
        succeeded_runs_24h: summary.map_or(0, |item| item.succeeded_runs_in_window),
        failed_runs_24h: summary.map_or(0, |item| item.failed_runs_in_window),
    }
}

#[permission("pipelines.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<ListResp>>> {
    let pipelines = state.storage.scheduled_pipelines.list(&ctx.org_id).await?;
    let since = TimestampMicros::now().0 - OVERVIEW_WINDOW_MICROS;
    let summaries: HashMap<String, PipelineRunSummary> = state
        .storage
        .pipeline_runs
        .summaries(&ctx.org_id, since)
        .await?
        .into_iter()
        .map(|summary| (summary.pipeline_id.0.clone(), summary))
        .collect();
    Ok(Json(
        pipelines
            .into_iter()
            .map(|pipeline| {
                let summary = summaries.get(&pipeline.id.0);
                to_list_resp(pipeline, summary)
            })
            .collect(),
    ))
}

#[resource_permission(
    action = "pipelines.read",
    resource = ScheduledPipeline,
    id = Id(id),
    bind = pipeline
)]
async fn get_one(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Resp>> {
    Ok(Json(to_resp(pipeline)))
}

#[permission("pipelines.create")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<CreateReq>,
) -> Result<Json<Resp>> {
    let source_stream = req.source_stream.trim().to_string();
    let target_stream = req.target_stream.trim().to_string();
    let stream_type = parse_signal_type(&req.function_steps);
    validate_pipeline_streams(&source_stream, &target_stream, stream_type)?;
    let now = TimestampMicros::now();
    let p = ScheduledPipeline {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        name: req.name,
        source_stream,
        target_stream,
        function_steps: req.function_steps,
        cron: req.cron,
        lookback_secs: req.lookback_secs,
        last_run_at: None,
        enabled: req.enabled,
        created_at: now,
        updated_at: now,
    };
    let p = state.storage.scheduled_pipelines.create(p).await?;
    Ok(Json(to_resp(p)))
}

#[resource_permission(
    action = resolve_all(|pipeline: &ScheduledPipeline| Ok(update_permissions(pipeline, &req))),
    resource = ScheduledPipeline,
    id = Id(id),
    bind = existing
)]
async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<UpdateReq>,
) -> Result<Json<Resp>> {
    let source_stream = req.source_stream.trim().to_string();
    let target_stream = req.target_stream.trim().to_string();
    let stream_type = parse_signal_type(&req.function_steps);
    validate_pipeline_streams(&source_stream, &target_stream, stream_type)?;
    let p = ScheduledPipeline {
        id: existing.id,
        org_id: existing.org_id,
        name: req.name,
        source_stream,
        target_stream,
        function_steps: req.function_steps,
        cron: req.cron,
        lookback_secs: req.lookback_secs,
        last_run_at: existing.last_run_at,
        enabled: req.enabled,
        created_at: existing.created_at,
        updated_at: TimestampMicros::now(),
    };
    let p = state.storage.scheduled_pipelines.update(p).await?;
    Ok(Json(to_resp(p)))
}

#[resource_permission(
    action = "pipelines.delete",
    resource = ScheduledPipeline,
    id = Id(id),
    bind = pipeline
)]
async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    state
        .storage
        .scheduled_pipelines
        .delete(&pipeline.org_id, &pipeline.id)
        .await?;
    Ok(Json(serde_json::json!({"deleted": true})))
}

#[derive(Debug, Deserialize)]
pub struct ListRunsQuery {
    #[serde(default = "default_run_limit")]
    pub limit: i64,
    pub before_micros: Option<i64>,
}

fn default_run_limit() -> i64 {
    50
}

#[derive(Debug, Serialize)]
pub struct RunResp {
    pub id: String,
    pub pipeline_id: String,
    pub state: String,
    pub started_at_micros: i64,
    pub finished_at_micros: Option<i64>,
    pub scanned_rows: i64,
    pub error: Option<String>,
}

fn to_run_resp(r: PipelineRun) -> RunResp {
    RunResp {
        id: r.id.0,
        pipeline_id: r.pipeline_id.0,
        state: r.state.as_str().to_string(),
        started_at_micros: r.started_at.0,
        finished_at_micros: r.finished_at.map(|t| t.0),
        scanned_rows: r.scanned_rows,
        error: r.error,
    }
}

/// `GET /scheduled_pipelines/{id}/runs` — pipeline 执行历史。
/// Cross-org 命中（unknown 或他 org）统一返 404，不区分。
#[resource_permission(
    action = "pipelines.read",
    resource = ScheduledPipeline,
    id = Id(id),
    bind = pipeline
)]
async fn list_runs(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Query(q): Query<ListRunsQuery>,
) -> Result<Json<Vec<RunResp>>> {
    let rows = state
        .storage
        .pipeline_runs
        .list(&pipeline.org_id, &pipeline.id, q.limit, q.before_micros)
        .await?;
    Ok(Json(rows.into_iter().map(to_run_resp).collect()))
}

#[derive(Debug, Deserialize)]
pub struct BackfillReq {
    pub start_micros: i64,
    pub end_micros: i64,
}

#[derive(Debug, Serialize)]
pub struct BackfillResp {
    pub job_id: String,
    pub monitor: String,
}

/// `POST /scheduled_pipelines/{id}/backfill` — 提交一次 backfill。复用 search-job 通道。
#[resource_permission(
    action = "pipelines.run",
    resource = ScheduledPipeline,
    id = Id(id),
    bind = pipeline
)]
async fn submit_backfill(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<BackfillReq>,
) -> Result<Response> {
    if req.end_micros <= req.start_micros {
        return Err(Error::invalid(
            "end_micros must be greater than start_micros",
        ));
    }
    let window = req.end_micros - req.start_micros;
    if window > MAX_BACKFILL_WINDOW_MICROS {
        return Err(Error::invalid("backfill window must be <= 31 days"));
    }
    let pipeline_id = pipeline.id.clone();
    let stream_type = parse_signal_type(&pipeline.function_steps);
    validate_pipeline_streams(
        &pipeline.source_stream,
        &pipeline.target_stream,
        stream_type,
    )?;

    // 合成 QueryRequest 读源：复用 pipeline 的 source stream + 用户窗口，`SELECT *` 取原始行。
    // request_json 里带上 `pipeline_id`，SearchJob worker 据此对结果跑 function_steps（VRL）
    // → 写目标 stream → connector egress（见 bootstrap workers::pipeline_exec）。
    let synth = QueryRequest {
        org_id: pipeline.org_id.clone(),
        language: QueryLanguage::Sql,
        statement: format!("SELECT * FROM {}", pipeline.source_stream),
        time_range: TimeRange::new(
            TimestampMicros(req.start_micros),
            TimestampMicros(req.end_micros),
        ),
        stream: Some(StreamHint {
            name: pipeline.source_stream.clone(),
            stream_type,
        }),
        limit: None,
        federation_clusters: Vec::new(),
    };
    let mut request_json =
        serde_json::to_value(&synth).map_err(|e| Error::internal(format!("request json: {e}")))?;
    if let Some(obj) = request_json.as_object_mut() {
        obj.insert(
            "pipeline_id".to_string(),
            Value::String(pipeline_id.0.clone()),
        );
        obj.insert(
            "backfill_window_micros".to_string(),
            Value::Number(window.into()),
        );
    }

    let now = TimestampMicros::now();
    let ttl_secs: i64 = 7 * 86400;
    let job = SearchJob {
        id: Id::new(),
        org_id: pipeline.org_id.clone(),
        user_id: ctx.user_id.clone(),
        request_json,
        trace_link: crate::shared::trace_context::current_trace_context()
            .map(|context| context.serialized_link()),
        state: SearchJobState::Pending,
        result_object_key: None,
        result_rows: None,
        error: None,
        submitted_at: now,
        started_at: None,
        finished_at: None,
        expires_at: TimestampMicros(now.0 + ttl_secs * 1_000_000),
    };
    let job = state.storage.search_jobs.create(job).await?;
    let body = BackfillResp {
        job_id: job.id.0.clone(),
        monitor: format!("/api/v1/query/jobs/{}", job.id.0),
    };
    let resp = Response::builder()
        .status(202)
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap_or_default()))
        .map_err(|e| Error::internal(format!("response build: {e}")))?;
    Ok(resp)
}
