// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Grading the runs of drills whose investigation has ended.
//!
//! The scoring worker claims them ([`process_due`]): a model compares the answer's root cause with
//! the drill's reference and scores it from 1 to 5, mapped to 0 to 1, with a reason in the
//! starter's language. The model is the first enabled evaluator's, else the organization's first
//! model; the system message is the organization's evaluation Prompt filled with the drill's rubric.
//! Usage counts as evaluation. A run whose investigation failed, was stopped or wrote no answer
//! ends without a score; when today's budget is used up, grading waits; a grading error is tried
//! again up to [`MAX_GRADING_ATTEMPTS`] times.

use agent::{
    runs::RunStatus,
    scoring::{
        drills::{
            DrillRun, DrillRunStatus, MAX_GRADING_ATTEMPTS,
            grading::{GRADING_RUBRIC, grading_material, incident_brief},
            reasons,
        },
        evaluation::{EVALUATION_PURPOSE, VERDICT_INSTRUCTION, output_language, parse_verdict},
    },
};
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use postgres::repositories::agent::prompts::render_prompt;
use serde_json::{Map, Value};

use super::super::{evaluating, facts::readable};
use crate::api::{AppState, rca::system_user_id};

/// How long a claimed run may take to grade before another node takes it.
const LEASE_MICROS: i64 = 5 * 60 * 1_000_000;
/// Runs claimed at a time.
const BATCH: i64 = 5;
/// When today's budget is used up, grading waits this long.
const WAIT_MICROS: i64 = 10 * 60 * 1_000_000;
/// After a grading error, the n-th try waits n times this long.
const RETRY_MICROS: i64 = 2 * 60 * 1_000_000;
/// The grading model answers with a score and two sentences.
const VERDICT_TOKENS: i32 = 400;

enum Graded {
    Done,
    Failed(&'static str, Option<String>),
    /// Try again later without counting an attempt.
    Later,
}

/// Grades the runs that are due; returns how many were claimed.
pub(crate) async fn process_due(state: &AppState) -> Result<usize> {
    let now = TimestampMicros::now();

    let runs = state
        .agent
        .scoring
        .claim_drill_runs(now, TimestampMicros(now.0 + LEASE_MICROS), BATCH)
        .await?;

    for run in &runs {
        settle(state, run.clone()).await;
    }

    Ok(runs.len())
}

async fn settle(state: &AppState, mut run: DrillRun) {
    let outcome = grade(state, &mut run).await;
    let now = TimestampMicros::now();

    match outcome {
        Ok(Graded::Done) => {}
        Ok(Graded::Failed(reason, detail)) => run.fail(reason, detail, now),
        Ok(Graded::Later) => {
            run.status = DrillRunStatus::Running;
            run.attempts = (run.attempts - 1).max(0);
            run.not_before = Some(TimestampMicros(now.0 + WAIT_MICROS));
            run.updated_at = now;
        }
        Err(error) if run.attempts < MAX_GRADING_ATTEMPTS => {
            tracing::warn!(drill_run = %run.id.0, %error, "a drill run could not be graded; trying again later");

            run.status = DrillRunStatus::Running;
            run.not_before = Some(TimestampMicros(
                now.0 + RETRY_MICROS * i64::from(run.attempts.max(1)),
            ));
            run.updated_at = now;
        }
        Err(error) => run.fail(reasons::GRADING_FAILED, Some(error.to_string()), now),
    }

    if let Err(error) = state.agent.scoring.save_drill_run(&run).await {
        tracing::warn!(drill_run = %run.id.0, %error, "a drill run could not be saved");
    }
}

async fn grade(state: &AppState, run: &mut DrillRun) -> Result<Graded> {
    let Some(run_id) = run.run_id.clone() else {
        return Ok(Graded::Failed(reasons::START_FAILED, None));
    };

    let investigated = match state.agent.runs.get(&run.org_id, &run_id).await {
        Ok(investigated) => investigated,
        Err(Error::NotFound(_)) => return Ok(Graded::Failed(reasons::RUN_FAILED, None)),
        Err(error) => return Err(error),
    };

    match investigated.status {
        RunStatus::Succeeded => {}
        RunStatus::Failed => {
            return Ok(Graded::Failed(reasons::RUN_FAILED, investigated.error));
        }
        RunStatus::Cancelled => return Ok(Graded::Failed(reasons::RUN_CANCELLED, None)),
        RunStatus::Interrupted => return Ok(Graded::Failed(reasons::RUN_INTERRUPTED, None)),
        _ => return Ok(Graded::Later),
    }

    let (Some(chat_id), Some(message_id)) = (
        run.chat_id.clone(),
        investigated.assistant_message_id.clone(),
    ) else {
        return Ok(Graded::Failed(reasons::NO_ANSWER, None));
    };

    let chat = state.agent.chats.get_chat(&run.org_id, &chat_id).await?;
    let messages = state.agent.chats.list_messages(&chat_id).await?;

    let Some(message) = messages.iter().find(|message| message.id == message_id) else {
        return Ok(Graded::Failed(reasons::NO_ANSWER, None));
    };

    let answer = readable(&message.content);

    if answer.trim().is_empty() {
        return Ok(Graded::Failed(reasons::NO_ANSWER, None));
    }

    run.model = Some(chat.model.clone()).filter(|model| !model.is_empty());
    run.prompt_version = message.prompt_version;

    if state.agent.usage.gate(&run.org_id, false).await.refused() {
        return Ok(Graded::Later);
    }

    let drill = state
        .agent
        .scoring
        .get_drill(&run.org_id, &run.drill_id)
        .await?;

    let brief = match state
        .alerting
        .service
        .get_incident(&drill.incident_id)
        .await
    {
        Ok(incident) if incident.org_id == run.org_id => incident_brief(&incident, None),
        _ => format!("Incident: {}", drill.title),
    };

    let (provider_id, model_name, temperature) = grader(state, &run.org_id).await?;
    let model = evaluating::model(
        state,
        &run.org_id,
        provider_id.as_ref(),
        model_name.as_deref(),
    )
    .await?;

    let template = state
        .agent
        .prompts
        .resolve(&run.org_id, &system_user_id(), EVALUATION_PURPOSE)
        .await?;

    let mut vars = Map::new();
    vars.insert("rubric".into(), Value::String(GRADING_RUBRIC.into()));
    vars.insert(
        "output_language".into(),
        Value::String(output_language(&run.language).into()),
    );

    let system = format!(
        "{}\n\n{VERDICT_INSTRUCTION}",
        render_prompt(&template.body, &vars)
    );

    let (content, _) = evaluating::complete(
        state,
        &run.org_id,
        &model,
        (system, grading_material(&brief, &drill.reference, &answer)),
        (VERDICT_TOKENS, temperature),
    )
    .await?;

    let verdict = parse_verdict(&content)?;
    let now = TimestampMicros::now();

    run.status = DrillRunStatus::Done;
    run.score = Some(verdict.score);
    run.reasoning = verdict.reason;
    run.grader_model = Some(model.model);
    run.reason = None;
    run.detail = None;
    run.finished_at = Some(now);
    run.updated_at = now;
    run.not_before = None;

    Ok(Graded::Done)
}

/// The model that grades: the first enabled evaluator's, with its temperature; else the
/// organization's first model at temperature 0.
async fn grader(state: &AppState, org_id: &Id) -> Result<(Option<Id>, Option<String>, f32)> {
    let evaluator = state
        .agent
        .scoring
        .list_evaluators(org_id)
        .await?
        .into_iter()
        .find(|evaluator| evaluator.enabled);

    Ok(match evaluator {
        Some(evaluator) => (
            evaluator.provider_id,
            evaluator.model,
            evaluator.temperature,
        ),
        None => (None, None, 0.0),
    })
}
