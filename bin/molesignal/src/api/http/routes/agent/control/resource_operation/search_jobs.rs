// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::Deserialize;
use serde_json::{Value, json};

use super::super::OperationOutcome;
use crate::{
    agent::model::ApprovalRequest,
    api::AppState,
    app::iam::IamContext,
    domain::{
        query::{QueryLanguage, QueryRequest, StreamHint},
        stream::StreamType,
    },
    infra::persistence::repositories::search::jobs::SearchJob,
    shared::{
        Error, Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

pub(super) async fn execute(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    let job_id = Id(approval.target.clone());
    let (summary, job) = match approval.action.as_str() {
        "submit_search_job" => {
            let request: SubmitSearchJob = parse_parameters(&approval.parameters)?;
            let job = state
                .search_jobs
                .submit_with_id(
                    job_id,
                    ctx.org_id.clone(),
                    approval.requested_by.clone(),
                    ctx.organization_role_key().to_string(),
                    request.query_request(ctx.org_id.clone())?,
                    request.ttl_secs,
                )
                .await?;
            ("search job submitted", job)
        }
        "cancel_search_job" => {
            require_no_parameters(approval)?;
            (
                "search job cancelled",
                state.search_jobs.cancel(&ctx.org_id, &job_id).await?,
            )
        }
        "retry_search_job" => {
            require_no_parameters(approval)?;
            (
                "search job queued for retry",
                state.search_jobs.retry(&ctx.org_id, &job_id).await?,
            )
        }
        "delete_search_job" => {
            require_no_parameters(approval)?;
            state.search_jobs.delete(&ctx.org_id, &job_id).await?;
            return Ok(OperationOutcome {
                summary: "search job deleted".into(),
                verification: json!({"verified": true, "job_id": job_id, "deleted": true}),
            });
        }
        _ => unreachable!("search-job operation received unrelated action"),
    };
    Ok(OperationOutcome {
        summary: summary.into(),
        verification: json!({"verified": true, "job": safe_job(job)}),
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SubmitSearchJob {
    language: QueryLanguage,
    statement: String,
    time_range: TimeRangeInput,
    #[serde(default)]
    stream: Option<StreamInput>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    federation_clusters: Vec<String>,
    #[serde(default)]
    ttl_secs: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TimeRangeInput {
    start_micros: i64,
    end_micros: i64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StreamInput {
    name: String,
    stream_type: StreamType,
}

impl SubmitSearchJob {
    fn query_request(&self, org_id: Id) -> Result<QueryRequest> {
        let statement = self.statement.trim();
        if statement.is_empty() {
            return Err(Error::invalid("search job statement must not be empty"));
        }
        if self.time_range.end_micros < self.time_range.start_micros {
            return Err(Error::invalid(
                "time_range.end_micros must not precede start_micros",
            ));
        }
        Ok(QueryRequest {
            org_id,
            language: self.language,
            statement: statement.to_string(),
            time_range: TimeRange::new(
                TimestampMicros(self.time_range.start_micros),
                TimestampMicros(self.time_range.end_micros),
            ),
            stream: self.stream.as_ref().map(|stream| StreamHint {
                name: stream.name.clone(),
                stream_type: stream.stream_type,
            }),
            limit: self.limit.map(|limit| limit.clamp(1, 100_000)),
            federation_clusters: self.federation_clusters.clone(),
        })
    }
}

fn parse_parameters<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T> {
    serde_json::from_value(value.clone())
        .map_err(|error| Error::invalid(format!("invalid operation parameters: {error}")))
}

fn require_no_parameters(approval: &ApprovalRequest) -> Result<()> {
    let _: EmptyParameters = parse_parameters(&approval.parameters)?;
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyParameters {}

fn safe_job(job: SearchJob) -> Value {
    json!({
        "id": job.id,
        "user_id": job.user_id,
        "state": job.state,
        "attempt": job.attempt,
        "result_rows": job.result_rows,
        "error": job.error,
        "submitted_at": job.submitted_at,
        "started_at": job.started_at,
        "finished_at": job.finished_at,
        "expires_at": job.expires_at,
    })
}
