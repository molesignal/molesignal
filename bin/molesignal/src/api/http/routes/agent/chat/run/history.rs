// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 交给模型的历史，以及上下文压缩。
//!
//! 历史截到这一轮的问题，排在后面的问题不算；每个回答连同它的工具调用与结果一起还原，追问时
//! 模型看得到上一轮查到的数据。会话有摘要时，摘要替代它覆盖的那些消息。估算超过上下文的
//! 80% 时把最近两轮之前的对话压缩成摘要；`/compact` 把到现在为止的对话全部压缩。

use std::sync::Arc;

use serde_json::{Value, json};

use super::{
    super::{messages::command_of, provider::ResolvedProvider},
    Ending, RunControl,
};
use crate::{
    agent::{
        chat::{
            ChatMessage as ExternalChatMessage, CompletionRequest, MessageRole, ModelCallObserver,
            ToolChoice, observed_complete,
        },
        commands::text_for_model,
        runs::{
            COMPACTION_INSTRUCTION, COMPACTION_KEEP_TURNS, RunEventKind, TranscriptStep,
            compaction_due, compaction_split, estimate_tokens, language_instruction, summary_text,
            transcript_messages,
        },
        usage::UsageOrigin,
    },
    app::{
        agent_usage::{UsageRecorder, UsageScope},
        iam::IamContext,
    },
    infra::persistence::repositories::agent::chats::{Chat, ChatMessage},
    shared::{Error, ids::Id, time::TimestampMicros},
};

/// 摘要最多写这么多 token。
const SUMMARY_MAX_TOKENS: i32 = 2_048;

/// 交给模型的这一轮。
pub(super) struct Conversation {
    pub(super) messages: Vec<ExternalChatMessage>,
    /// 这一轮问题的时间：回答排在它后面。
    pub(super) asked_at: Option<TimestampMicros>,
}

pub(super) fn system_message(text: String) -> ExternalChatMessage {
    message(MessageRole::System, text, TimestampMicros::now())
}

fn message(role: MessageRole, content: String, at: TimestampMicros) -> ExternalChatMessage {
    ExternalChatMessage {
        id: Id::new(),
        chat_id: Id(String::new()),
        role,
        content,
        tool_call_id: None,
        tool_calls: Value::Null,
        created_at: at,
        prompt_tokens: None,
        completion_tokens: None,
    }
}

/// 存下的消息 → 交给模型的消息。用户消息是命令时交给模型展开后的问题，转义去掉反斜杠；
/// 回答前面先还原它的工具调用与结果。
pub(super) fn to_model(rows: &[ChatMessage]) -> Vec<ExternalChatMessage> {
    let mut messages = Vec::new();

    for row in rows {
        let steps = row
            .tool_transcript
            .clone()
            .filter(|_| row.role == "assistant")
            .and_then(|value| serde_json::from_value::<Vec<TranscriptStep>>(value).ok());

        if let Some(steps) = steps {
            messages.extend(transcript_messages(&row.chat_id, &steps, row.created_at));
        }

        messages.push(ExternalChatMessage {
            id: row.id.clone(),
            chat_id: row.chat_id.clone(),
            role: match row.role.as_str() {
                "assistant" => MessageRole::Assistant,
                "tool" => MessageRole::Tool,
                "system" => MessageRole::System,
                _ => MessageRole::User,
            },
            content: if row.role == "user" {
                text_for_model(&row.content, command_of(row).as_ref())
            } else {
                row.content.clone()
            },
            tool_call_id: row.tool_result_for.clone(),
            tool_calls: row.tool_calls_json.clone().unwrap_or(Value::Null),
            created_at: row.created_at,
            prompt_tokens: row.prompt_tokens.map(|v| v as i32),
            completion_tokens: row.completion_tokens.map(|v| v as i32),
        });
    }

    messages
}

/// 截到这一轮的问题（含）；找不到时整段都算。
fn up_to_question(
    mut rows: Vec<ChatMessage>,
    question: Option<&Id>,
) -> (Vec<ChatMessage>, Option<TimestampMicros>) {
    let position = question.and_then(|question| rows.iter().position(|row| &row.id == question));

    match position {
        Some(position) => {
            let asked_at = rows[position].created_at;
            rows.truncate(position + 1);
            (rows, Some(asked_at))
        }
        None => (rows, None),
    }
}

fn assemble(
    system_texts: &[String],
    summary: Option<&str>,
    rows: &[ChatMessage],
) -> Vec<ExternalChatMessage> {
    let mut messages = system_texts
        .iter()
        .cloned()
        .map(system_message)
        .collect::<Vec<_>>();

    messages.extend(summary.map(|summary| system_message(summary_text(summary))));
    messages.extend(to_model(rows));
    messages
}

fn estimate(messages: &[ExternalChatMessage]) -> u64 {
    messages
        .iter()
        .map(|message| {
            let calls = if message.tool_calls.is_null() {
                0
            } else {
                estimate_tokens(&message.tool_calls.to_string())
            };

            estimate_tokens(&message.content) + calls
        })
        .sum()
}

/// 一轮回答的历史；太长时先压缩，并写一条提醒。
#[allow(clippy::too_many_arguments)]
pub(super) async fn for_answer(
    control: &RunControl,
    chat: &Chat,
    resolved: &ResolvedProvider,
    recorder: &Arc<UsageRecorder>,
    rows: Vec<ChatMessage>,
    system_texts: &[String],
    window: u64,
) -> Conversation {
    let (rows, asked_at) = up_to_question(rows, control.run.user_message_id.as_ref());

    // 摘要只在这一轮的问题晚于它覆盖的消息时用：重新生成更早的问题时，摘要里有之后的事。
    let upto = chat
        .context_summary_upto_micros
        .filter(|upto| asked_at.is_none_or(|asked| asked.0 > *upto));

    let (mut summary, mut rows) = match (upto, chat.context_summary.clone()) {
        (Some(upto), Some(summary)) => (
            Some(summary),
            rows.into_iter()
                .filter(|row| row.created_at.0 > upto)
                .collect::<Vec<_>>(),
        ),
        _ => (None, rows),
    };

    let mut messages = assemble(system_texts, summary.as_deref(), &rows);

    if compaction_due(estimate(&messages), window) {
        let compacted = summarize(
            control,
            chat,
            resolved,
            recorder,
            summary.as_deref(),
            &rows,
            COMPACTION_KEEP_TURNS,
        )
        .await;

        if let Some((text, upto)) = compacted {
            let before = rows.len();
            rows.retain(|row| row.created_at.0 > upto.0);

            control
                .emit(
                    RunEventKind::Notice,
                    json!({ "kind": "compacted", "automatic": true, "messages": before - rows.len() }),
                )
                .await;

            summary = Some(text);
            messages = assemble(system_texts, summary.as_deref(), &rows);
        }
    }

    Conversation { messages, asked_at }
}

/// 把 `rows` 里最近 `keep_turns` 轮之前的部分连同已有摘要写成新摘要，记到会话上。返回摘要
/// 与它覆盖到的最后一条消息的时间；没有可压缩的或模型没写出来时为空。
async fn summarize(
    control: &RunControl,
    chat: &Chat,
    resolved: &ResolvedProvider,
    recorder: &Arc<UsageRecorder>,
    previous: Option<&str>,
    rows: &[ChatMessage],
    keep_turns: usize,
) -> Option<(String, TimestampMicros)> {
    let roles = rows.iter().map(|row| row.role.as_str()).collect::<Vec<_>>();
    let split = compaction_split(&roles, keep_turns)?;
    let older = &rows[..split];
    let upto = older.last()?.created_at;

    let mut messages = vec![system_message(
        "You write the working notes of an operations investigation, for yourself to continue from."
            .into(),
    )];

    messages.extend(language_instruction(&chat.reply_language).map(system_message));
    messages.extend(previous.map(|summary| system_message(summary_text(summary))));
    messages.extend(to_model(older));
    messages.push(message(
        MessageRole::User,
        COMPACTION_INSTRUCTION.into(),
        TimestampMicros::now(),
    ));

    let request = CompletionRequest {
        model: resolved.model.clone(),
        messages,
        tools: None,
        tool_choice: ToolChoice::None,
        max_tokens: Some(SUMMARY_MAX_TOKENS),
        temperature: Some(0.2),
    };

    let observer: Arc<dyn ModelCallObserver> = recorder.clone();

    let response = match observed_complete(resolved.adapter.as_ref(), request, Some(&observer))
        .await
    {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(chat_id = %chat.id.0, %error, "the chat history could not be compacted");
            return None;
        }
    };

    let text = response.content.unwrap_or_default().trim().to_string();

    if text.is_empty() {
        return None;
    }

    if let Err(error) = control
        .state
        .agent
        .chats
        .set_context_summary(&chat.id, &text, upto.0)
        .await
    {
        tracing::warn!(chat_id = %chat.id.0, %error, "the chat summary could not be saved");
        return None;
    }

    Some((text, upto))
}

/// `/compact`：把到现在为止的对话全部压缩成摘要。
pub(super) async fn compact_now(control: &RunControl, ctx: &IamContext, chat: Chat) -> Ending {
    let state = &control.state;

    let resolved =
        match super::super::provider::resolve_provider(state, ctx, &chat, None, None).await {
            Ok(resolved) => resolved,
            Err(error) => return Ending::failed(error.to_string()),
        };

    let rows = match state.agent.chats.list_messages(&chat.id).await {
        Ok(rows) => rows,
        Err(error) => return Ending::failed(error.to_string()),
    };

    let (summary, rows) = match (
        chat.context_summary.clone(),
        chat.context_summary_upto_micros,
    ) {
        (Some(summary), Some(upto)) => (
            Some(summary),
            rows.into_iter()
                .filter(|row| row.created_at.0 > upto)
                .collect::<Vec<_>>(),
        ),
        _ => (None, rows),
    };

    if rows.is_empty() {
        control
            .emit(
                RunEventKind::Notice,
                json!({ "kind": "compacted", "automatic": false, "messages": 0 }),
            )
            .await;

        return Ending::succeeded();
    }

    let recorder = state.agent.usage.recorder(UsageScope {
        user_id: Some(ctx.user_id.clone()),
        provider_id: resolved.provider_id.clone().map(Id),
        prices: resolved.prices,
        chat_id: Some(chat.id.clone()),
        ..UsageScope::new(ctx.org_id.clone(), UsageOrigin::Chat)
    });

    let compacted = summarize(
        control,
        &chat,
        &resolved,
        &recorder,
        summary.as_deref(),
        &rows,
        0,
    )
    .await;

    let totals = recorder.totals();

    let Some(_) = compacted else {
        return Ending {
            prompt_tokens: totals.prompt_tokens,
            completion_tokens: totals.completion_tokens,
            cost: recorder.cost(),
            ..Ending::failed(Error::unavailable("the model did not write a summary").to_string())
        };
    };

    control
        .emit(
            RunEventKind::Notice,
            json!({ "kind": "compacted", "automatic": false, "messages": rows.len() }),
        )
        .await;

    Ending {
        prompt_tokens: totals.prompt_tokens,
        completion_tokens: totals.completion_tokens,
        cost: recorder.cost(),
        ..Ending::succeeded()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, role: &str, content: &str, at: i64) -> ChatMessage {
        ChatMessage::minimal(
            Id(id.into()),
            Id("chat-1".into()),
            Id("org-1".into()),
            role,
            content,
            TimestampMicros(at),
        )
    }

    #[test]
    fn the_history_stops_at_the_question_being_answered() {
        let rows = vec![
            row("u1", "user", "first", 1),
            row("a1", "assistant", "answer", 2),
            row("u2", "user", "second", 3),
            row("u3", "user", "queued behind", 4),
        ];

        let (kept, asked_at) = up_to_question(rows, Some(&Id("u2".into())));

        assert_eq!(kept.len(), 3);
        assert_eq!(asked_at, Some(TimestampMicros(3)));
    }

    #[test]
    fn an_answer_brings_back_its_tool_calls_before_it() {
        let mut answer = row("a1", "assistant", "The API fails.", 2);
        answer.tool_transcript = Some(json!([{
            "text": "Checking logs.",
            "calls": [{"id": "c1", "name": "query_logs", "arguments": {"q": "5xx"}, "result": "[3]"}]
        }]));

        let messages = to_model(&[row("u1", "user", "why?", 1), answer]);
        let roles = messages.iter().map(|m| m.role).collect::<Vec<_>>();

        assert_eq!(
            roles,
            [
                MessageRole::User,
                MessageRole::Assistant,
                MessageRole::Tool,
                MessageRole::Assistant
            ]
        );
        assert_eq!(messages[1].content, "Checking logs.");
        assert_eq!(messages[2].content, "[3]");
        assert_eq!(messages[3].content, "The API fails.");
    }

    #[test]
    fn the_model_reads_commands_expanded_and_escapes_without_the_backslash() {
        let mut command = row("user-1", "user", "/rca inc-1", 1);
        command.command_json = Some(json!({
            "name": "rca",
            "args": "inc-1",
            "source": "builtin",
            "model_text": "Find the root cause of the incident \"API 502s\".",
            "incident_id": "inc-1",
        }));

        let history = to_model(&[command, row("user-2", "user", "\\/etc/hosts is wrong", 2)]);

        assert_eq!(
            history[0].content,
            "Find the root cause of the incident \"API 502s\"."
        );
        assert_eq!(history[1].content, "/etc/hosts is wrong");
    }

    #[test]
    fn a_summary_replaces_what_it_covers() {
        let rows = [
            row("u1", "user", "old question", 1),
            row("u2", "user", "new", 5),
        ];
        let messages = assemble(&["system".into()], Some("they asked about X"), &rows[1..]);

        assert_eq!(messages.len(), 3);
        assert!(messages[1].content.contains("they asked about X"));
        assert!(estimate(&messages) > 0);
    }
}
