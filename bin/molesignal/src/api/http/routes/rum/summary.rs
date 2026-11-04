// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The summary of a session and the chapters of its replay, written by a model.
//!
//! `GET /rum/sessions/{id}/summary` reads the one kept in the reader's language. `POST` asks the
//! model of the organization, under its daily budget, with someone waiting: the call is counted
//! apart (`replay_summary`), and the same events are not asked about twice.

use std::sync::Arc;

use axum::{
    Extension, Json,
    extract::{Path, Query, State},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{
    entity::EntityQuery,
    reader::Reader,
    timeline::{self, Timeline},
};
use crate::{
    agent::{
        chat::{
            ChatMessage, CompletionRequest, MessageRole, ModelCallObserver, Provider, ToolChoice,
            TransportOptions, adapter_from_parts, observed_complete,
        },
        usage::UsageOrigin,
    },
    api::{
        AppState,
        http::interface_language::{interface_language, interface_language_pack},
        rca::pick_provider,
    },
    app::{
        agent_usage::{BudgetGate, UsageScope},
        iam::IamContext,
        rum::summary::{digest_of, input_of, read_answer},
    },
    domain::{
        iam::permission,
        rum::{RumSessionSummary, RumSummaryChapter},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// What the model is given to answer in, unless its settings say less.
const MAX_ANSWER_TOKENS: i32 = 1_500;
const INSTRUCTIONS_PREFIX: &str = "rum.session_summary.";
const INSTRUCTIONS_KEY: &str = "rum.session_summary.instructions";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SummaryView {
    session_id: String,
    language: String,
    summary: String,
    chapters: Vec<RumSummaryChapter>,
    model: String,
    prompt_tokens: i64,
    completion_tokens: i64,
    generated_at: i64,
}

impl From<RumSessionSummary> for SummaryView {
    fn from(summary: RumSessionSummary) -> Self {
        Self {
            session_id: summary.session_id,
            language: summary.language,
            summary: summary.summary,
            chapters: summary.chapters,
            model: summary.model,
            prompt_tokens: summary.prompt_tokens,
            completion_tokens: summary.completion_tokens,
            generated_at: summary.generated_at.0,
        }
    }
}

/// The summary kept, if any: a session without one is not an error.
#[derive(Debug, Serialize)]
pub(super) struct SummaryAnswer {
    summary: Option<SummaryView>,
}

#[permission(any("rum.replay.read", "sys.telemetry.read"))]
pub(super) async fn read(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(session_id): Path<String>,
) -> Result<Json<SummaryAnswer>> {
    let language = interface_language(&state, &ctx).await?;

    let summary = state
        .telemetry
        .rum_summaries
        .get(&ctx.org_id, &session_id, &language)
        .await?;

    Ok(Json(SummaryAnswer {
        summary: summary.map(SummaryView::from),
    }))
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GenerateBody {
    /// Ask again though a summary is kept. Events that have not changed are still not asked about.
    #[serde(default)]
    regenerate: bool,
}

fn session_end(session: &Map<String, Value>, start_micros: i64) -> i64 {
    session
        .get("session.last_activity")
        .and_then(Value::as_i64)
        .map_or(start_micros, |last_ms| last_ms.saturating_mul(1_000))
        .max(start_micros)
}

#[permission("rum.replay.summaries.generate")]
pub(super) async fn generate(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(session_id): Path<String>,
    Query(query): Query<EntityQuery>,
    body: Option<Json<GenerateBody>>,
) -> Result<Json<SummaryAnswer>> {
    if ctx.is_system_scope() {
        return Err(Error::not_found("RUM session summaries"));
    }

    let regenerate = body.is_some_and(|Json(body)| body.regenerate);
    let language = interface_language(&state, &ctx).await?;

    let kept = state
        .telemetry
        .rum_summaries
        .get(&ctx.org_id, &session_id, &language)
        .await?;

    if !regenerate && let Some(kept) = kept {
        return Ok(Json(SummaryAnswer {
            summary: Some(kept.into()),
        }));
    }

    let reader = Reader::new(&state, &ctx);

    let Timeline { session, items, .. } = timeline::read(&reader, &session_id, &query).await?;

    let session = session.ok_or_else(|| Error::not_found("RUM session"))?;

    let start_micros = session
        .get("timestamp")
        .and_then(Value::as_i64)
        .unwrap_or_default();

    let application = session
        .get("application.id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let input = input_of(start_micros, &items)
        .ok_or_else(|| Error::invalid("nothing happened in this session to summarize"))?;

    let pack = interface_language_pack(&state, &ctx, INSTRUCTIONS_PREFIX).await;

    let instructions = pack
        .get(INSTRUCTIONS_KEY)
        .map(str::to_string)
        .ok_or_else(|| Error::internal("the instructions of a session summary are missing"))?;

    let digest = digest_of(&instructions, &input);

    // The same events in the same words have the same answer.
    if let Some(kept) = state
        .telemetry
        .rum_summaries
        .get(&ctx.org_id, &session_id, &language)
        .await?
        .filter(|kept| kept.input_digest == digest)
    {
        return Ok(Json(SummaryAnswer {
            summary: Some(kept.into()),
        }));
    }

    // Someone is waiting for this: the organization's choice for such calls decides.
    if let BudgetGate::Refuse(status) = state.agent.usage.gate(&ctx.org_id, true).await {
        return Err(Error::ResourceExhausted(format!(
            "daily Mole Agent token budget exhausted ({} of {} tokens used)",
            status.used_tokens, status.limit_tokens
        )));
    }

    let providers = state.agent.model_providers.list(&ctx.org_id).await?;

    let provider = pick_provider(&providers)
        .ok_or_else(|| Error::invalid("no enabled model provider with an API key is configured"))?;

    let key = state
        .agent
        .model_providers
        .get_plaintext_key(&ctx.org_id, &provider.id)
        .await?
        .ok_or_else(|| Error::invalid("the model provider has no API key"))?;

    let adapter = adapter_from_parts(
        Provider::parse(&provider.provider)?,
        provider.base_url.clone(),
        key,
        TransportOptions::from_timeout_ms(provider.timeout_ms),
    )?;

    let recorder: Arc<dyn ModelCallObserver> = state.agent.usage.recorder(UsageScope {
        user_id: Some(ctx.user_id.clone()),
        provider_id: Some(provider.id.clone()),
        prices: provider.prices(),
        ..UsageScope::new(ctx.org_id.clone(), UsageOrigin::ReplaySummary)
    });

    let now = TimestampMicros::now();

    let request = CompletionRequest {
        model: provider.default_model.clone(),
        messages: vec![
            message(MessageRole::System, instructions, now),
            message(MessageRole::User, input, now),
        ],
        tools: None,
        tool_choice: ToolChoice::None,
        max_tokens: provider
            .max_tokens
            .map(|tokens| i32::try_from(tokens).unwrap_or(i32::MAX))
            .or(Some(MAX_ANSWER_TOKENS)),
        temperature: Some(0.2),
    };

    let response = observed_complete(adapter.as_ref(), request, Some(&recorder)).await?;

    let (summary, chapters) = read_answer(
        response.content.as_deref().unwrap_or_default(),
        start_micros,
        session_end(&session, start_micros),
    )?;

    let summary = RumSessionSummary {
        org_id: ctx.org_id.clone(),
        application_id: application,
        session_id: session_id.clone(),
        language,
        summary,
        chapters,
        model: provider.default_model.clone(),
        prompt_tokens: i64::from(response.prompt_tokens),
        completion_tokens: i64::from(response.completion_tokens),
        input_digest: digest,
        generated_by: Some(ctx.user_id.clone()),
        generated_at: now,
    };

    state.telemetry.rum_summaries.put(&summary).await?;

    Ok(Json(SummaryAnswer {
        summary: Some(summary.into()),
    }))
}

fn message(role: MessageRole, content: String, now: TimestampMicros) -> ChatMessage {
    ChatMessage {
        id: Id::new(),
        chat_id: Id(String::new()),
        role,
        content,
        tool_call_id: None,
        tool_calls: Value::Null,
        created_at: now,
        prompt_tokens: None,
        completion_tokens: None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_session_ends_at_its_last_activity_and_not_before_it_began() {
        let session = json!({"session.last_activity": 2_000})
            .as_object()
            .unwrap()
            .clone();

        assert_eq!(session_end(&session, 1_000_000), 2_000_000);
        assert_eq!(session_end(&session, 3_000_000), 3_000_000);
        assert_eq!(session_end(&Map::new(), 1_000), 1_000);
    }
}
