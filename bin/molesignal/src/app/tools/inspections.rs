// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Log inspections: the organization's inspections, their runs and one run's report. Running one
//! now is a managed mutation (`mutations.rs`), carried out by the operation registry.

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::{ToolResult, catalog::BuiltinToolKind};

use super::{
    ToolRuntime,
    common::{json_result, parse_args},
};
use crate::{
    app::{
        iam::IamContext,
        reporting::{RunAsProfile, run_as_profiles},
    },
    domain::log_inspection::{LogInspection, LogInspectionRun},
    shared::{Error, Result, ids::Id},
};

const DEFAULT_RUNS: i64 = 10;
const MAX_RUNS: i64 = 50;

#[derive(Debug, Default, Deserialize)]
struct InspectionArgs {
    inspection_id: Option<String>,
    run_id: Option<String>,
    limit: Option<i64>,
}

pub(super) async fn execute(
    runtime: &ToolRuntime,
    auth: &IamContext,
    kind: BuiltinToolKind,
    arguments: Value,
) -> Result<ToolResult> {
    let service = runtime.log_inspections()?;
    let args: InspectionArgs = parse_args(arguments)?;

    match kind {
        BuiltinToolKind::ListLogInspections => {
            let inspections = service.list(&auth.org_id).await?;
            let active = service.active_runs(&auth.org_id).await?;

            let users = profiles(
                runtime,
                inspections
                    .iter()
                    .map(|inspection| &inspection.run_as_user_id),
            )
            .await;

            let rows = inspections
                .into_iter()
                .map(|inspection| {
                    let running = active.iter().find(|run| run.inspection_id == inspection.id);

                    inspection_row(inspection, running, &users)
                })
                .collect::<Vec<_>>();

            json_result(&rows)
        }
        BuiltinToolKind::ListLogInspectionRuns => {
            let id = required_id(args.inspection_id, "inspection_id")?;
            let limit = args.limit.unwrap_or(DEFAULT_RUNS).clamp(1, MAX_RUNS);

            let runs = service.list_runs(&auth.org_id, &id, None, limit).await?;

            json_result(&runs.iter().map(run_summary).collect::<Vec<_>>())
        }
        BuiltinToolKind::GetLogInspectionRun => {
            let id = required_id(args.inspection_id, "inspection_id")?;
            let run_id = required_id(args.run_id, "run_id")?;

            let run = service.get_run(&auth.org_id, &id, &run_id).await?;

            json_result(&run_detail(&run))
        }
        _ => unreachable!("log-inspection executor received unrelated tool"),
    }
}

/// Who each inspection runs as, by name; an unreadable user list leaves only the ids.
async fn profiles<'a>(
    runtime: &ToolRuntime,
    user_ids: impl IntoIterator<Item = &'a Id>,
) -> HashMap<Id, RunAsProfile> {
    run_as_profiles(runtime.administration.iam.users.as_ref(), user_ids)
        .await
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "log inspection user profiles could not be read");
            HashMap::new()
        })
}

fn inspection_row(
    inspection: LogInspection,
    running: Option<&LogInspectionRun>,
    users: &HashMap<Id, RunAsProfile>,
) -> Value {
    let run_as = users.get(&inspection.run_as_user_id).map_or_else(
        || json!({ "id": inspection.run_as_user_id }),
        |user| json!({ "id": user.id, "name": user.display_name, "email": user.email }),
    );

    json!({
        "id": inspection.id,
        "name": inspection.name,
        "stream": inspection.stream,
        "statement": inspection.statement,
        "window_secs": inspection.window_secs,
        "requirements": inspection.requirements,
        "schedule": inspection.schedule,
        "timezone": inspection.timezone,
        "model": inspection.model,
        "notify_when": inspection.notify_when,
        "notify_routes": inspection.notify_routes,
        "language": inspection.language,
        "enabled": inspection.enabled,
        "run_as_user": run_as,
        "next_run_at_micros": inspection.next_run_at.map(|at| at.0),
        "last_run": inspection.last_run.map(|run| json!({
            "at_micros": run.at.0,
            "status": run.status,
            "level": run.level,
        })),
        "active_run": running.map(|run| json!({
            "id": run.id,
            "status": run.status,
            "trigger": run.trigger,
            "created_at_micros": run.created_at.0,
        })),
    })
}

/// A run as the list shows it: how it ended, without the report.
fn run_summary(run: &LogInspectionRun) -> Value {
    json!({
        "id": run.id,
        "trigger": run.trigger,
        "status": run.status,
        "level": run.level,
        "summary": run.summary,
        "failure": run.failure,
        "window_start_micros": run.window_start.0,
        "window_end_micros": run.window_end.0,
        "created_at_micros": run.created_at.0,
        "finished_at_micros": run.finished_at.map(|at| at.0),
    })
}

fn run_detail(run: &LogInspectionRun) -> Value {
    let mut detail = run_summary(run);

    detail["report"] = json!(run.report);
    detail["error"] = json!(run.error);
    detail["model"] = json!(run.model);
    detail["requested_by"] = json!(run.requested_by);
    detail["started_at_micros"] = json!(run.started_at.map(|at| at.0));
    detail["tool_calls"] = json!(run.tool_calls);
    detail["prompt_tokens"] = json!(run.prompt_tokens);
    detail["completion_tokens"] = json!(run.completion_tokens);
    detail["notifications"] = json!(run.notifications);

    detail
}

fn required_id(value: Option<String>, field: &str) -> Result<Id> {
    value
        .filter(|value| !value.trim().is_empty())
        .map(Id)
        .ok_or_else(|| Error::invalid(format!("{field} is required")))
}
