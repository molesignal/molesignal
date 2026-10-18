// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 给对话起名：标题还是对话页取自第一个问题的临时标题时，和回答同时请同一个模型起名（规则在
//! `agent::titles`），起好后写进会话，告诉正在看的页面。用量照常记进组织的每日用量；起不出名
//! （模型出错、问题还没有主题）时标题不变，下一个问题再起。

use std::{sync::Arc, time::Duration};

use serde_json::json;
use tokio::task::JoinHandle;

use super::{super::provider::ResolvedProvider, RunControl, history::system_message};
use crate::{
    agent::{
        chat::{
            ChatMessage as ExternalChatMessage, CompletionRequest, MessageRole, ModelCallObserver,
            ToolChoice, observed_complete,
        },
        runs::RunEventKind,
        titles::{TITLE_INSTRUCTION, clean_title, question_for_title},
        usage::UsageOrigin,
    },
    app::{agent_usage::UsageScope, iam::IamContext},
    infra::persistence::repositories::agent::chats::Chat,
    shared::ids::Id,
};

/// 回答写完后最多再等起名这么久，好让页面拿到回答时也拿到名字。
const WAIT: Duration = Duration::from_secs(10);

/// 名字要的 token：够思考型模型想一下再写。
const MAX_TOKENS: i32 = 256;

/// 标题还是临时的就开始起名，和回答同时进行。
pub(super) fn start(
    control: &Arc<RunControl>,
    ctx: &IamContext,
    chat: &Chat,
    resolved: &ResolvedProvider,
    question: &str,
) -> Option<JoinHandle<()>> {
    let question = question_for_title(question);

    if !chat.title_provisional || question.is_empty() {
        return None;
    }

    let control = control.clone();
    let chat_id = chat.id.clone();
    let adapter = resolved.adapter.clone();
    let model = resolved.model.clone();

    let scope = UsageScope {
        user_id: Some(ctx.user_id.clone()),
        provider_id: resolved.provider_id.clone().map(Id),
        prices: resolved.prices,
        chat_id: Some(chat.id.clone()),
        ..UsageScope::new(ctx.org_id.clone(), UsageOrigin::Chat)
    };

    Some(tokio::spawn(async move {
        let state = &control.state;
        let recorder = state.agent.usage.recorder(scope);
        let observer: Arc<dyn ModelCallObserver> = recorder;

        let request = CompletionRequest {
            model,
            messages: vec![
                system_message(TITLE_INSTRUCTION.into()),
                ExternalChatMessage {
                    role: MessageRole::User,
                    ..system_message(question)
                },
            ],
            tools: None,
            tool_choice: ToolChoice::None,
            max_tokens: Some(MAX_TOKENS),
            temperature: Some(0.2),
        };

        let title = match observed_complete(adapter.as_ref(), request, Some(&observer)).await {
            Ok(response) => response.content.as_deref().and_then(clean_title),
            Err(error) => {
                tracing::debug!(chat_id = %chat_id.0, %error, "a chat could not be named");
                None
            }
        };

        let Some(title) = title else {
            return;
        };

        match state.agent.chats.name_chat(&chat_id, &title).await {
            Ok(true) => {
                control
                    .emit(
                        RunEventKind::Notice,
                        json!({ "kind": "title", "title": title }),
                    )
                    .await;
            }
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(chat_id = %chat_id.0, %error, "a chat's name could not be saved")
            }
        }
    }))
}

/// 等起名写完再结束这一轮，最多等 [`WAIT`]；被停下时不等，名字照样会写下。
pub(super) async fn settle(control: &RunControl, naming: Option<JoinHandle<()>>) {
    let Some(naming) = naming else {
        return;
    };

    if control.cancel.is_cancelled() {
        return;
    }

    let _ = tokio::time::timeout(WAIT, naming).await;
}
