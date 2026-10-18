// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 会话结束后提炼记忆：组织打开了提炼时，会话停下一段时间后由模型从上次提炼之后的对话里挑出
//! 值得记住的东西，写进这个会话写记忆的地方（团队会话是本队的，个人会话是发起人的）。计入
//! 组织的预算；预算拦截时不提炼。

use std::sync::Arc;

use serde_json::json;

use super::{
    super::provider::resolve_provider,
    Ending, RunControl,
    history::{system_message, to_model},
};
use crate::{
    agent::{
        chat::{
            ChatMessage as ExternalChatMessage, CompletionRequest, MessageRole, ModelCallObserver,
            ToolChoice, observed_complete,
        },
        memory::{DISTILL_INSTRUCTION, index_prompt, parse_drafts},
        runs::{RunEventKind, language_instruction},
        scope::{ChatScope, Scope},
        usage::UsageOrigin,
    },
    api::http::routes::agent::{budget, viewer::chat_scope},
    app::{
        agent_usage::{BudgetGate, UsageScope},
        iam::IamContext,
    },
    infra::persistence::repositories::agent::chats::Chat,
    shared::{ids::Id, time::TimestampMicros},
};

/// 交给模型的对话最多这么多字符，多了只留最近的。
const TRANSCRIPT_CHARS: usize = 60_000;

pub(super) async fn distill(
    control: &Arc<RunControl>,
    ctx: &IamContext,
    chat: Chat,
    since_micros: Option<i64>,
) -> Ending {
    let state = &control.state;

    if let BudgetGate::Refuse(_) = state.agent.usage.gate(&ctx.org_id, false).await {
        return Ending::refused(budget::BUDGET_EXHAUSTED);
    }

    let resolved = match resolve_provider(state, ctx, &chat, None, None).await {
        Ok(resolved) => resolved,
        Err(error) => return Ending::failed(error.to_string()),
    };

    let rows = match state.agent.chats.list_messages(&chat.id).await {
        Ok(rows) => rows,
        Err(error) => return Ending::failed(error.to_string()),
    };

    // 上次提炼之后的问答，只要文字（不带工具调用），最近的在后。
    let mut rows = rows
        .into_iter()
        .filter(|row| since_micros.is_none_or(|since| row.created_at.0 > since))
        .filter(|row| row.role == "user" || row.role == "assistant")
        .map(|mut row| {
            row.tool_transcript = None;
            row
        })
        .collect::<Vec<_>>();

    let mut chars = 0;
    rows.reverse();
    rows.retain(|row| {
        chars += row.content.chars().count();
        chars <= TRANSCRIPT_CHARS
    });
    rows.reverse();

    if !rows.iter().any(|row| row.role == "user") {
        return Ending::succeeded();
    }

    let scope = match chat_scope(&chat) {
        ChatScope::Team { team_id } => Scope::Team { team_id },
        ChatScope::Personal { owner_id, .. } => Scope::Personal { owner_id },
    };

    let existing = state
        .agent
        .memory
        .list(&ctx.org_id, &scope)
        .await
        .unwrap_or_default();

    let mut messages = vec![system_message(
        "You keep the long-term memory of an operations assistant.".into(),
    )];

    messages.extend(language_instruction(&chat.reply_language).map(system_message));
    messages.extend(index_prompt(&existing, "this chat").map(system_message));
    messages.extend(to_model(&rows));
    messages.push(ExternalChatMessage {
        role: MessageRole::User,
        ..system_message(DISTILL_INSTRUCTION.into())
    });

    let recorder = state.agent.usage.recorder(UsageScope {
        user_id: Some(ctx.user_id.clone()),
        provider_id: resolved.provider_id.clone().map(Id),
        prices: resolved.prices,
        chat_id: Some(chat.id.clone()),
        ..UsageScope::new(ctx.org_id.clone(), UsageOrigin::Chat)
    });

    let observer: Arc<dyn ModelCallObserver> = recorder.clone();

    let request = CompletionRequest {
        model: resolved.model.clone(),
        messages,
        tools: None,
        tool_choice: ToolChoice::None,
        max_tokens: Some(2_048),
        temperature: Some(0.2),
    };

    let response = observed_complete(resolved.adapter.as_ref(), request, Some(&observer)).await;
    let totals = recorder.totals();

    let ending = |base: Ending| Ending {
        prompt_tokens: totals.prompt_tokens,
        completion_tokens: totals.completion_tokens,
        cost: recorder.cost(),
        ..base
    };

    let text = match response {
        Ok(response) => response.content.unwrap_or_default(),
        Err(error) => return ending(Ending::failed(error.to_string())),
    };

    let mut names = Vec::new();

    for draft in parse_drafts(&text) {
        match state
            .agent
            .memory
            .remember(&ctx.org_id, &scope, &draft, Some(&chat.id), &ctx.user_id)
            .await
        {
            Ok(memory) => names.push(memory.name),
            Err(error) => {
                tracing::warn!(chat_id = %chat.id.0, %error, "a distilled memory could not be saved")
            }
        }
    }

    if !names.is_empty() {
        control
            .emit(
                RunEventKind::Notice,
                json!({ "kind": "memories", "count": names.len(), "names": names, "at_micros": TimestampMicros::now().0 }),
            )
            .await;
    }

    ending(Ending::succeeded())
}
