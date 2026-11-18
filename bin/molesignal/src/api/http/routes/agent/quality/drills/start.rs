// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Starting a run of a drill.
//!
//! The run is a hidden chat in the organization's scope (it loads organization resources and no
//! memory, and holders of the Quality tab can open it from there), bound to the drill and holding
//! the question; and a Run queued as the starter, unattended, read-only and pinned to the time the
//! incident happened. The incident's facts reach the model as data, without how it went later;
//! the tools that would tell that are denied for drill Runs (`chat::run::denied`). The run's row is
//! written once its Run is queued; a run that cannot start is written failed with the reason.

use agent::{
    references::{MessageReference, ReferenceKind},
    runs::{Run, RunPrincipal, RunSource},
    scoring::drills::{
        Drill, DrillRun, DrillRunStatus, RUN_DEADLINE_SECS,
        grading::{DRILL_INSTRUCTION, incident_brief, incident_context, iso, window},
        reasons,
    },
};
use domain::{
    alerting::incident::Incident, i18n_language::FALLBACK_LANGUAGE_CODE, iam::IamContext,
};
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use postgres::repositories::agent::chats::{Chat, ChatMessage};
use serde_json::json;

use crate::{
    api::{
        AppState,
        http::{
            interface_language::interface_language,
            middleware::Permission,
            routes::agent::chat::{
                PostMessageReq,
                run::request::{AnswerRequest, RunRequest, TurnSnapshot},
            },
        },
    },
    app::alerting::rule_guide::guide_facts,
};

/// The question the chat starts with, in the starter's language.
const QUESTION_PREFIX: &str = "agent.drill";
const QUESTION_KEY: &str = "agent.drill.question";

/// The chat and the Run a started run is made of.
struct Begun {
    chat_id: Id,
    run_id: Id,
}

/// Why a run could not start; written on it.
struct Refusal {
    reason: &'static str,
    detail: Option<String>,
}

impl Refusal {
    fn of(reason: &'static str) -> Self {
        Self {
            reason,
            detail: None,
        }
    }
}

/// Starts a run of `drill` as the person in `ctx`. A run that cannot start (no model, no budget
/// left, the incident gone) is written failed and returned; a starter who cannot read the
/// incident is refused.
pub(super) async fn start(
    state: &AppState,
    ctx: &IamContext,
    drill: &Drill,
    batch_id: Option<&Id>,
) -> Result<DrillRun> {
    let now = TimestampMicros::now();

    let language = interface_language(state, ctx)
        .await
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "the interface language of a drill's starter could not be read");
            FALLBACK_LANGUAGE_CODE.to_string()
        });

    let mut run = DrillRun {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        drill_id: drill.id.clone(),
        chat_id: None,
        run_id: None,
        status: DrillRunStatus::Running,
        score: None,
        reasoning: String::new(),
        reason: None,
        detail: None,
        model: None,
        prompt_version: None,
        grader_model: None,
        language,
        batch_id: batch_id.cloned(),
        attempts: 0,
        started_by: ctx.user_id.clone(),
        created_at: now,
        finished_at: None,
        updated_at: now,
        not_before: None,
    };

    match begin(state, ctx, drill, &run).await? {
        Ok(begun) => {
            run.chat_id = Some(begun.chat_id);
            run.run_id = Some(begun.run_id.clone());

            if let Err(error) = state.agent.scoring.create_drill_run(&run).await {
                // Nothing would grade it: stop it.
                let _ = state
                    .agent
                    .runs
                    .request_cancel(&ctx.org_id, &begun.run_id)
                    .await;

                return Err(error);
            }
        }
        Err(refusal) => {
            run.fail(refusal.reason, refusal.detail, now);
            state.agent.scoring.create_drill_run(&run).await?;
        }
    }

    Ok(run)
}

/// Makes the chat and queues the Run; the inner error is a refusal written on the run, the outer
/// one fails the request.
async fn begin(
    state: &AppState,
    ctx: &IamContext,
    drill: &Drill,
    run: &DrillRun,
) -> Result<std::result::Result<Begun, Refusal>> {
    let incident = match state
        .alerting
        .service
        .get_incident(&drill.incident_id)
        .await
    {
        Ok(incident) if incident.org_id == ctx.org_id => incident,
        Ok(_) | Err(Error::NotFound(_)) => return Ok(Err(Refusal::of(reasons::INCIDENT_GONE))),
        Err(error) => return Err(error),
    };

    // The run reads as the starter: someone who cannot read the incident cannot start it.
    Permission::require_resource(
        state,
        ctx,
        "alerts.incidents.read",
        &ctx.org_id,
        "alert",
        &incident.id.0,
    )
    .await
    .map_err(|_| Error::forbidden("you cannot read the incident of this drill"))?;

    if state.agent.usage.gate(&ctx.org_id, false).await.refused() {
        return Ok(Err(Refusal::of(reasons::BUDGET_EXHAUSTED)));
    }

    let has_model = state
        .agent
        .model_providers
        .list(&ctx.org_id)
        .await?
        .iter()
        .any(|provider| provider.enabled);

    if !has_model {
        return Ok(Err(Refusal::of(reasons::NO_MODEL)));
    }

    match queue(state, ctx, drill, run, &incident).await {
        Ok(begun) => Ok(Ok(begun)),
        Err(error) => Ok(Err(Refusal {
            reason: reasons::START_FAILED,
            detail: Some(error.to_string()),
        })),
    }
}

async fn queue(
    state: &AppState,
    ctx: &IamContext,
    drill: &Drill,
    run: &DrillRun,
    incident: &Incident,
) -> Result<Begun> {
    let guide = state
        .alerting
        .service
        .get_rule(&incident.rule_id)
        .await
        .ok()
        .and_then(|rule| guide_facts(&rule));

    let brief = incident_brief(incident, guide.as_deref());
    let (start_at, end_at) = window(incident.created_at);
    let question = question(state, &run.language, incident, start_at, end_at).await;
    let now = TimestampMicros::now();

    // `automation_id` keeps the chat out of chat lists and read-only, as a run of an automation's
    // is; `subject_kind` says it is a drill's.
    let chat = Chat {
        scope: "org".into(),
        team_id: None,
        reply_language: run.language.clone(),
        analysis_mode: Some("root_cause".into()),
        time_range_start: Some(start_at.0),
        time_range_end: Some(end_at.0),
        automation_id: Some(run.id.0.clone()),
        subject_kind: Some("drill".into()),
        subject_id: Some(drill.id.0.clone()),
        ..Chat::minimal(
            Id::new(),
            ctx.org_id.clone(),
            ctx.user_id.clone(),
            String::new(),
            String::new(),
            drill.title.clone(),
            now,
        )
    };

    let chat = state.agent.chats.create_chat(chat).await?;

    let reference = MessageReference {
        kind: ReferenceKind::Incident,
        id: incident.id.0.clone(),
        label: incident.summary.clone(),
    };

    let asked = state
        .agent
        .chats
        .append_message(ChatMessage {
            user_id: Some(ctx.user_id.clone()),
            references_json: serde_json::to_value(vec![reference]).ok(),
            ..ChatMessage::minimal(
                Id::new(),
                chat.id.clone(),
                ctx.org_id.clone(),
                "user",
                question.clone(),
                now,
            )
        })
        .await?;

    let message: PostMessageReq = serde_json::from_value(json!({
        "content": question,
        "analysisMode": "root_cause",
        "executionPolicy": "read_only",
        "timeRange": { "startTime": start_at.0, "endTime": end_at.0 },
    }))
    .map_err(|error| Error::internal(format!("a drill's question: {error}")))?;

    let request = RunRequest::Answer(Box::new(AnswerRequest {
        message,
        turn: TurnSnapshot {
            system_texts: vec![DRILL_INSTRUCTION.to_string(), incident_context(&brief)],
            alert_name: Some(incident.summary.clone()),
            ..TurnSnapshot::default()
        },
        regenerate: false,
        dashboard: None,
        asked_at: Some(asked.created_at.0),
        deadline_secs: Some(RUN_DEADLINE_SECS),
    }));

    let request = serde_json::to_value(&request)
        .map_err(|error| Error::internal(format!("a drill's run could not be queued: {error}")))?;

    let queued = state
        .agent
        .runs
        .create(Run::queued(
            ctx.org_id.clone(),
            chat.id.clone(),
            RunSource::Drill,
            RunPrincipal::of(ctx),
            request,
            Some(asked.id.clone()),
        ))
        .await?;

    Ok(Begun {
        chat_id: chat.id,
        run_id: queued.id,
    })
}

/// The question, from `i18n_language` in the starter's language; the incident's summary when
/// the text cannot be read.
async fn question(
    state: &AppState,
    language: &str,
    incident: &Incident,
    start_at: TimestampMicros,
    end_at: TimestampMicros,
) -> String {
    let (start, end) = (iso(start_at), iso(end_at));
    let fills = [
        ("summary", incident.summary.as_str()),
        ("start", start.as_str()),
        ("end", end.as_str()),
    ];

    let written = match state
        .platform
        .i18n_language
        .load_pack(QUESTION_PREFIX, language)
        .await
    {
        Ok(pack) => pack.render(QUESTION_KEY, &fills),
        Err(error) => Err(error),
    };

    written.unwrap_or_else(|error| {
        tracing::warn!(%error, "the question of a drill could not be written");
        incident.summary.clone()
    })
}
