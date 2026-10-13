// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Search Jobs HTTP routes（spec search-jobs）。
//!
//! 提供提交、状态、列表与各状态计数、分页结果、取消、重试与删除；后台 worker 将结果写为 NDJSON。
//!
//! 任务属于提交它的人：除提交外的每个接口都只对提交人和组织管理员开放
//! （见 [`SearchJobViewer`]），别人的任务与不存在的任务一样返回 404。

use std::collections::{BTreeSet, HashMap};

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    api::AppState,
    app::{iam::IamContext, query::jobs::SearchJobViewer},
    domain::{
        iam::permission,
        query::{QueryLanguage, QueryRequest, StreamHint},
    },
    infra::persistence::repositories::search::jobs::{
        SearchJob, SearchJobCounts, SearchJobKind, SearchJobState,
    },
    shared::{Error, Result, ids::Id, time::TimeRange},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/query/jobs", post(submit).get(list))
        .route("/query/jobs/summary", get(summary))
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
    /// `query`：直接提交的查询；`backfill`：数据管道的回填。
    pub kind: SearchJobKind,
    /// 回填任务所属的数据管道。
    pub pipeline_id: Option<String>,
    pub attempt: i32,
    /// 提交任务的用户（`users.id`）。
    pub user_id: String,
    /// 这位用户的显示名和邮箱。用户记录已经不存在时为 `null`。
    pub submitted_by: Option<SubmitterResp>,
    /// 任务要跑的查询。任务行上的请求解不出来时为 `null`，调度器会把这样的任务记为失败。
    pub request: Option<JobRequestResp>,
    pub submitted_at_micros: i64,
    pub started_at_micros: Option<i64>,
    pub finished_at_micros: Option<i64>,
    pub result_object_key: Option<String>,
    pub result_rows: Option<i64>,
    pub error: Option<String>,
    pub expires_at_micros: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubmitterResp {
    pub display_name: String,
    pub email: String,
}

/// 提交的查询里给人看的那一部分，字段名与提交时的请求体相同。
#[derive(Debug, Serialize)]
pub struct JobRequestResp {
    pub language: QueryLanguage,
    pub statement: String,
    pub time_range: TimeRange,
    pub stream: Option<StreamHint>,
    pub limit: Option<usize>,
}

fn request_resp(request_json: &Value) -> Option<JobRequestResp> {
    let request = QueryRequest::deserialize(request_json).ok()?;

    Some(JobRequestResp {
        language: request.language,
        statement: request.statement,
        time_range: request.time_range,
        stream: request.stream,
        limit: request.limit,
    })
}

fn to_resp(j: SearchJob, submitters: &HashMap<Id, SubmitterResp>) -> JobResp {
    JobResp {
        kind: j.kind(),
        pipeline_id: j.pipeline_id().map(str::to_string),
        submitted_by: submitters.get(&j.user_id).cloned(),
        request: request_resp(&j.request_json),
        job_id: j.id.0,
        state: j.state.as_str().to_string(),
        attempt: j.attempt,
        user_id: j.user_id.0,
        submitted_at_micros: j.submitted_at.0,
        started_at_micros: j.started_at.map(|t| t.0),
        finished_at_micros: j.finished_at.map(|t| t.0),
        result_object_key: j.result_object_key,
        result_rows: j.result_rows,
        error: j.error,
        expires_at_micros: j.expires_at.0,
    }
}

/// 提交人的显示名和邮箱，按用户 id 索引。直接读用户记录：能看任务的人不一定能读成员目录
/// （`org.members.read`）。用户表读不了时任务照常返回，只是这些任务只剩用户 id。
async fn submitters(state: &AppState, jobs: &[SearchJob]) -> HashMap<Id, SubmitterResp> {
    let ids = jobs
        .iter()
        .map(|job| job.user_id.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(Id::from_string)
        .collect::<Vec<_>>();

    if ids.is_empty() {
        return HashMap::new();
    }

    match state.iam.service.users.get_many(&ids).await {
        Ok(users) => users
            .into_iter()
            .map(|user| {
                let submitter = SubmitterResp {
                    display_name: user.display_name,
                    email: user.email,
                };

                (user.id, submitter)
            })
            .collect(),
        Err(error) => {
            tracing::warn!(%error, "search job submitters could not be read");
            HashMap::new()
        }
    }
}

async fn job_resp(state: &AppState, job: SearchJob) -> JobResp {
    let submitters = submitters(state, std::slice::from_ref(&job)).await;
    to_resp(job, &submitters)
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

    Ok(Json(job_resp(&state, job).await))
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    /// 跳过最近提交的多少条，与 `limit` 一起翻页。
    #[serde(default)]
    pub offset: i64,
    /// 只要这些状态的任务，逗号分隔；缺省或为空表示全部。
    #[serde(default)]
    pub state: Option<String>,
}

fn default_limit() -> i64 {
    100
}

/// `state=pending,running` → 状态列表。写错的状态名直接拒绝，而不是悄悄当成别的状态。
fn parse_states(raw: Option<&str>) -> Result<Vec<SearchJobState>> {
    let mut states = Vec::new();

    for name in raw.unwrap_or_default().split(',').map(str::trim) {
        if name.is_empty() {
            continue;
        }

        let state = SearchJobState::try_parse(name)
            .ok_or_else(|| Error::invalid(format!("unknown search job state `{name}`")))?;

        if !states.contains(&state) {
            states.push(state);
        }
    }

    Ok(states)
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(p): Query<ListQuery>,
) -> Result<Json<Vec<JobResp>>> {
    let states = parse_states(p.state.as_deref())?;
    let viewer = SearchJobViewer::of(&ctx);

    let jobs = state
        .search_jobs
        .list_page(&ctx.org_id, &viewer, states, p.limit, p.offset)
        .await?;

    let submitters = submitters(&state, &jobs).await;

    Ok(Json(
        jobs.into_iter()
            .map(|job| to_resp(job, &submitters))
            .collect(),
    ))
}

#[derive(Debug, Serialize)]
pub struct SummaryResp {
    pub pending: i64,
    pub running: i64,
    pub done: i64,
    pub failed: i64,
    pub cancelled: i64,
    pub total: i64,
}

impl From<SearchJobCounts> for SummaryResp {
    fn from(counts: SearchJobCounts) -> Self {
        Self {
            pending: counts.pending,
            running: counts.running,
            done: counts.done,
            failed: counts.failed,
            cancelled: counts.cancelled,
            total: counts.total(),
        }
    }
}

/// `GET /query/jobs/summary` — 调用方看得到的任务在各状态的数目（管理员是整个组织的，其他人
/// 是自己提交的），不受列表的状态筛选与翻页影响。
#[permission(any("streams.query", "sys.telemetry.read"))]
async fn summary(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<SummaryResp>> {
    let counts = state
        .search_jobs
        .counts(&ctx.org_id, &SearchJobViewer::of(&ctx))
        .await?;

    Ok(Json(counts.into()))
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn get_one(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<JobResp>> {
    let job = state
        .search_jobs
        .get(&ctx.org_id, &Id(id), &SearchJobViewer::of(&ctx))
        .await?;

    Ok(Json(job_resp(&state, job).await))
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
        .results(
            &ctx.org_id,
            &Id(id),
            &SearchJobViewer::of(&ctx),
            p.page,
            p.page_size,
        )
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
    let job = state
        .search_jobs
        .cancel(&ctx.org_id, &Id(id), &SearchJobViewer::of(&ctx))
        .await?;

    Ok(Json(job_resp(&state, job).await))
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn retry(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<JobResp>> {
    let job = state
        .search_jobs
        .retry(&ctx.org_id, &Id(id), &SearchJobViewer::of(&ctx))
        .await?;

    Ok(Json(job_resp(&state, job).await))
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    state
        .search_jobs
        .delete(&ctx.org_id, &Id(id), &SearchJobViewer::of(&ctx))
        .await?;

    Ok(Json(serde_json::json!({"deleted": true})))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::time::TimestampMicros;

    fn job(request_json: Value) -> SearchJob {
        SearchJob {
            id: Id::from_string("job-1"),
            org_id: Id::from_string("org-1"),
            user_id: Id::from_string("u-ada"),
            role_key: "editor".into(),
            request_json,
            trace_link: None,
            state: SearchJobState::Running,
            attempt: 2,
            result_object_key: None,
            result_rows: None,
            error: None,
            submitted_at: TimestampMicros(10),
            started_at: Some(TimestampMicros(20)),
            finished_at: None,
            expires_at: TimestampMicros(90),
        }
    }

    fn request_json() -> Value {
        serde_json::json!({
            "org_id": "org-1",
            "language": "sql",
            "statement": "SELECT * FROM app",
            "time_range": {"start": 1_000, "end": 2_000},
            "stream": {"name": "app", "stream_type": "logs"},
            "limit": 500,
            "federation_clusters": [],
        })
    }

    #[test]
    fn states_are_parsed_from_a_comma_separated_list() {
        assert_eq!(parse_states(None).unwrap(), Vec::new());
        assert_eq!(parse_states(Some("")).unwrap(), Vec::new());
        assert_eq!(
            parse_states(Some("pending, running,,pending")).unwrap(),
            vec![SearchJobState::Pending, SearchJobState::Running]
        );
    }

    #[test]
    fn an_unknown_state_is_refused_instead_of_read_as_pending() {
        let error = parse_states(Some("running,queued")).unwrap_err();

        assert!(matches!(error, Error::InvalidArgument(_)), "{error}");
        assert!(error.to_string().contains("queued"), "{error}");
    }

    #[test]
    fn a_query_job_carries_what_was_submitted_and_who_submitted_it() {
        let submitters = HashMap::from([(
            Id::from_string("u-ada"),
            SubmitterResp {
                display_name: "Ada".into(),
                email: "ada@example.com".into(),
            },
        )]);

        let body = serde_json::to_value(to_resp(job(request_json()), &submitters)).unwrap();

        assert_eq!(body["job_id"], "job-1");
        assert_eq!(body["state"], "running");
        assert_eq!(body["kind"], "query");
        assert_eq!(body["pipeline_id"], Value::Null);
        assert_eq!(body["attempt"], 2);
        assert_eq!(body["user_id"], "u-ada");
        assert_eq!(
            body["submitted_by"],
            serde_json::json!({"display_name": "Ada", "email": "ada@example.com"})
        );
        assert_eq!(
            body["request"],
            serde_json::json!({
                "language": "sql",
                "statement": "SELECT * FROM app",
                "time_range": {"start": 1_000, "end": 2_000},
                "stream": {"name": "app", "stream_type": "logs"},
                "limit": 500,
            })
        );
    }

    #[test]
    fn a_backfill_job_names_its_pipeline() {
        let mut request = request_json();
        request["pipeline_id"] = "pipe-7".into();
        request["backfill_window_micros"] = 1_000.into();

        let body = serde_json::to_value(to_resp(job(request), &HashMap::new())).unwrap();

        assert_eq!(body["kind"], "backfill");
        assert_eq!(body["pipeline_id"], "pipe-7");
        assert_eq!(body["request"]["statement"], "SELECT * FROM app");
    }

    #[test]
    fn a_job_keeps_its_user_id_when_the_user_record_is_gone() {
        let body = serde_json::to_value(to_resp(job(request_json()), &HashMap::new())).unwrap();

        assert_eq!(body["user_id"], "u-ada");
        assert_eq!(body["submitted_by"], Value::Null);
    }

    #[test]
    fn a_job_whose_request_cannot_be_decoded_still_lists() {
        let body = serde_json::to_value(to_resp(
            job(serde_json::json!({"statement": 7})),
            &HashMap::new(),
        ))
        .unwrap();

        assert_eq!(body["request"], Value::Null);
        assert_eq!(body["kind"], "query");
    }

    #[test]
    fn the_summary_totals_every_state() {
        let body = serde_json::to_value(SummaryResp::from(SearchJobCounts {
            pending: 2,
            running: 1,
            done: 40,
            failed: 3,
            cancelled: 4,
        }))
        .unwrap();

        assert_eq!(
            body,
            serde_json::json!({
                "pending": 2,
                "running": 1,
                "done": 40,
                "failed": 3,
                "cancelled": 4,
                "total": 50,
            })
        );
    }
}
