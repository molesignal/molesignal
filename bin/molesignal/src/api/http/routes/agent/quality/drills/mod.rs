// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Drills of the Quality tab: resolved incidents investigated again and graded against the root
//! cause people established (`agent::scoring::drills`).
//!
//! - `GET /api/v1/agent/quality/drills`: drills with their runs counted and their last run
//! - `GET /api/v1/agent/quality/drills/candidates`: incidents resolved in the last 180 days whose
//!   internal postmortem states a root cause, among those the reader may read
//! - `POST /api/v1/agent/quality/drills` `{incidentId, reference?}`: a drill; without `reference`
//!   the postmortem's root cause is the reference
//! - `DELETE /api/v1/agent/quality/drills/{id}`
//! - `GET /api/v1/agent/quality/drills/{id}/runs`: its runs, newest first; `POST` starts one
//! - `POST /api/v1/agent/quality/drills/runs` `{drillIds?}`: starts a run of every drill (or of
//!   those named) that has none going, all in one batch
//!
//! Reading needs `agent.quality.read`, the rest `agent.quality.manage`. A run reads as the person
//! who starts it, who must be able to read the incident. Grading is in [`grade`], run by the
//! scoring worker.

mod grade;
mod start;

use agent::scoring::drills::{
    CANDIDATE_DAYS, Drill, DrillSource, MAX_BATCH, MAX_REFERENCE_CHARS, grading::root_cause_of,
};
use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{delete, get, post},
};
use domain::{
    alerting::incident::Incident,
    iam::{IamContext, permission},
};
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde::Deserialize;
use serde_json::{Value, json};

pub(crate) use self::grade::process_due;
use crate::api::{
    AppState,
    http::{
        middleware::{Permission, authorize_resource},
        routes::activity_audit,
    },
};

const MICROS_PER_DAY: i64 = 86_400_000_000;
/// Candidates read, and listed at most.
const CANDIDATES_READ: i64 = 200;
const CANDIDATES_LISTED: usize = 50;
/// The root cause of a candidate is shown this long at most.
const ROOT_CAUSE_EXCERPT: usize = 500;
/// Runs of a drill listed.
const RUNS_LISTED: i64 = 20;
const TITLE_CHARS: usize = 300;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/agent/quality/drills", get(list).post(create))
        .route("/agent/quality/drills/candidates", get(candidates))
        .route("/agent/quality/drills/runs", post(run_all))
        .route("/agent/quality/drills/{id}", delete(remove))
        .route("/agent/quality/drills/{id}/runs", get(runs).post(run_one))
}

fn excerpt(text: &str, chars: usize) -> String {
    let text = text.trim();

    if text.chars().count() <= chars {
        return text.to_string();
    }

    let mut cut = text.chars().take(chars).collect::<String>();
    cut.push('…');
    cut
}

#[permission("agent.quality.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    let drills = state.agent.scoring.list_drills(&ctx.org_id).await?;

    Ok(Json(json!({ "drills": drills })))
}

#[permission("agent.quality.read")]
async fn candidates(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    let since = TimestampMicros(TimestampMicros::now().0 - CANDIDATE_DAYS * MICROS_PER_DAY);

    let rows = state
        .agent
        .scoring
        .drill_candidates(&ctx.org_id, since, CANDIDATES_READ)
        .await?;

    let mut candidates = Vec::new();

    for row in rows {
        let Some(root_cause) = root_cause_of(&row.body) else {
            continue;
        };

        // Only incidents the reader may read: a candidate shows its summary and root cause.
        let readable = Permission::require_resource(
            &state,
            &ctx,
            "alerts.incidents.read",
            &ctx.org_id,
            "alert",
            &row.incident_id.0,
        )
        .await
        .is_ok();

        if !readable {
            continue;
        }

        candidates.push(json!({
            "incidentId": row.incident_id,
            "summary": row.summary,
            "severity": row.severity,
            "createdAt": row.created_at,
            "resolvedAt": row.resolved_at,
            "rootCause": excerpt(&root_cause, ROOT_CAUSE_EXCERPT),
            "drilled": row.drilled,
        }));

        if candidates.len() >= CANDIDATES_LISTED {
            break;
        }
    }

    Ok(Json(json!({ "candidates": candidates })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateDrill {
    incident_id: String,
    /// Written by hand; the postmortem's root cause when absent.
    #[serde(default)]
    reference: Option<String>,
}

/// The root cause the incident's internal postmortem states.
async fn postmortem_root_cause(state: &AppState, incident: &Incident) -> Result<Option<String>> {
    Ok(state
        .alerting
        .postmortems
        .repository()
        .for_incident(&incident.org_id, &incident.id)
        .await?
        .and_then(|postmortem| root_cause_of(&postmortem.body)))
}

#[permission("agent.quality.manage")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<CreateDrill>,
) -> Result<Json<Drill>> {
    let incident: Incident = authorize_resource(
        &state,
        &ctx,
        Id::from_string(request.incident_id.trim()),
        "alerts.incidents.read",
    )
    .await?;

    let manual = request
        .reference
        .as_deref()
        .map(str::trim)
        .filter(|reference| !reference.is_empty())
        .map(str::to_string);

    let (reference, source) = match manual {
        Some(reference) => (reference, DrillSource::Manual),
        None => {
            let reference = postmortem_root_cause(&state, &incident)
                .await?
                .ok_or_else(|| {
                    Error::invalid(
                        "the incident's postmortem states no root cause; write the reference root cause for the drill",
                    )
                })?;

            (reference, DrillSource::Postmortem)
        }
    };

    if reference.chars().count() > MAX_REFERENCE_CHARS {
        return Err(Error::invalid(format!(
            "a reference root cause has at most {MAX_REFERENCE_CHARS} characters"
        )));
    }

    let now = TimestampMicros::now();

    let drill = Drill {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        incident_id: incident.id.clone(),
        title: incident.summary.chars().take(TITLE_CHARS).collect(),
        reference,
        source,
        created_by: ctx.user_id.clone(),
        created_at: now,
        updated_at: now,
    };

    state.agent.scoring.create_drill(&drill).await?;

    activity_audit::record(
        &state,
        &ctx,
        "agent.drill.created",
        "agent_drill",
        &drill.id.0,
        json!({ "incidentId": drill.incident_id, "source": drill.source }),
    )
    .await;

    Ok(Json(drill))
}

#[permission("agent.quality.manage")]
async fn remove(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let id = Id(id);

    if !state.agent.scoring.delete_drill(&ctx.org_id, &id).await? {
        return Err(Error::not_found(format!("drill `{}` not found", id.0)));
    }

    activity_audit::record(
        &state,
        &ctx,
        "agent.drill.deleted",
        "agent_drill",
        &id.0,
        json!({}),
    )
    .await;

    Ok(Json(json!({ "deleted": true })))
}

#[permission("agent.quality.read")]
async fn runs(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let drill = state.agent.scoring.get_drill(&ctx.org_id, &Id(id)).await?;

    let runs = state
        .agent
        .scoring
        .list_drill_runs(&ctx.org_id, &drill.id, RUNS_LISTED)
        .await?;

    Ok(Json(json!({ "drill": drill, "runs": runs })))
}

#[permission("agent.quality.manage")]
async fn run_one(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let drill = state.agent.scoring.get_drill(&ctx.org_id, &Id(id)).await?;

    let going = state
        .agent
        .scoring
        .list_drill_runs(&ctx.org_id, &drill.id, 1)
        .await?
        .first()
        .is_some_and(|run| !run.status.is_over());

    if going {
        return Err(Error::conflict("a run of this drill is still going"));
    }

    let run = start::start(&state, &ctx, &drill, None).await?;

    Ok(Json(json!({ "run": run })))
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunAll {
    /// The drills to run; every drill when absent.
    #[serde(default)]
    drill_ids: Option<Vec<String>>,
}

#[permission("agent.quality.manage")]
async fn run_all(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<RunAll>,
) -> Result<Json<Value>> {
    let named = request.drill_ids;
    let drills = state.agent.scoring.list_drills(&ctx.org_id).await?;
    let batch_id = Id::new();
    let mut started = Vec::new();
    let mut skipped = 0usize;

    for summary in drills {
        let wanted = named
            .as_ref()
            .is_none_or(|ids| ids.contains(&summary.drill.id.0));

        if !wanted {
            continue;
        }

        let going = summary
            .last_run
            .as_ref()
            .is_some_and(|run| !run.status.is_over());

        if going || started.len() >= MAX_BATCH {
            skipped += 1;
            continue;
        }

        match start::start(&state, &ctx, &summary.drill, Some(&batch_id)).await {
            Ok(run) => started.push(run),
            // The starter cannot read this incident: the others still start.
            Err(Error::Forbidden(_)) => skipped += 1,
            Err(error) => return Err(error),
        }
    }

    activity_audit::record(
        &state,
        &ctx,
        "agent.drill.batch_started",
        "agent_drill",
        &batch_id.0,
        json!({ "started": started.len(), "skipped": skipped }),
    )
    .await;

    Ok(Json(json!({
        "batchId": batch_id,
        "runs": started,
        "skipped": skipped,
    })))
}
