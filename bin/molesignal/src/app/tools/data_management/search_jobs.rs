// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::ToolResult;

use super::super::{
    ToolRuntime,
    common::{json_result, parse_args, redact_credentials},
};
use crate::{
    app::iam::IamContext,
    infra::persistence::repositories::search::jobs::SearchJob,
    shared::{Error, Result, ids::Id},
};

pub(super) async fn list(
    runtime: &ToolRuntime,
    auth: &IamContext,
    value: Value,
) -> Result<ToolResult> {
    let args: ListArgs = parse_args(value)?;
    let jobs = runtime
        .data
        .search_jobs
        .list(&auth.org_id, args.limit.unwrap_or(50).clamp(1, 200) as i64)
        .await?;
    jobs_result(jobs)
}

pub(super) async fn get(
    runtime: &ToolRuntime,
    auth: &IamContext,
    value: Value,
) -> Result<ToolResult> {
    let args: JobArgs = parse_args(value)?;
    jobs_result(vec![
        runtime
            .data
            .search_jobs
            .get(&auth.org_id, &Id(required_job_id(&args.job_id)?))
            .await?,
    ])
}

pub(super) async fn results(
    runtime: &ToolRuntime,
    auth: &IamContext,
    value: Value,
) -> Result<ToolResult> {
    let args: JobArgs = parse_args(value)?;
    json_result(
        &runtime
            .data
            .search_jobs
            .results(
                &auth.org_id,
                &Id(required_job_id(&args.job_id)?),
                args.page.unwrap_or(1),
                args.page_size.unwrap_or(100).clamp(1, 1_000),
            )
            .await?,
    )
}

fn jobs_result(jobs: Vec<SearchJob>) -> Result<ToolResult> {
    let safe = jobs
        .into_iter()
        .map(|job| {
            json!({
                "id": job.id,
                "user_id": job.user_id,
                "request": redact_credentials(&job.request_json),
                "state": job.state,
                "attempt": job.attempt,
                "result_rows": job.result_rows,
                "error": job.error,
                "submitted_at": job.submitted_at,
                "started_at": job.started_at,
                "finished_at": job.finished_at,
                "expires_at": job.expires_at,
            })
        })
        .collect::<Vec<_>>();
    Ok(ToolResult::json(json!({"jobs": safe})))
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListArgs {
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JobArgs {
    job_id: String,
    #[serde(default)]
    page: Option<i64>,
    #[serde(default)]
    page_size: Option<i64>,
}

fn required_job_id(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        Err(Error::invalid("job_id is required"))
    } else {
        Ok(value.to_string())
    }
}
