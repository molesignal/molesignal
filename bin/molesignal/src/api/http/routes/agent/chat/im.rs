// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! IM 里的一条提问：与控制台发消息一样解析斜杠命令、看预算、记下问题，准备好一轮回答。
//!
//! Run 的来源是 IM：没有人在控制台里等着回答问题或确认，照无人值守处理。准备好的 Run 交回
//! 调用方，由它先记下 Run 与 IM 消息的对应再排进队列：回答得再快，结束时也找得到回复到哪里。

use axum::{body::to_bytes, response::Response};
use serde_json::{Value, json};

use super::{
    PostMessageReq, commands,
    run::request::{AnswerRequest, RunRequest, TurnSnapshot},
};
use crate::{
    agent::runs::{Run, RunPrincipal, RunSource},
    api::{AppState, http::routes::agent::viewer::readable_chat},
    app::{agent_usage::BudgetGate, iam::IamContext},
    infra::persistence::repositories::agent::chats::{Chat, ChatMessage},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 一条 IM 提问准备得怎么样。
#[derive(Debug)]
pub(crate) enum ImPost {
    /// 问题记下了，这一轮还没排进队列。
    Ready(Box<Run>),
    /// 斜杠命令被拒绝：错误码与命令名。
    Refused { code: String, command: String },
    /// 组织今天的预算用完了。
    BudgetExhausted,
}

/// 拒绝的应答里的错误码与命令名。
async fn refusal(response: Response) -> ImPost {
    let body = to_bytes(response.into_body(), 64 * 1024)
        .await
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .unwrap_or(Value::Null);

    ImPost::Refused {
        code: body["code"]
            .as_str()
            .unwrap_or("command_failed")
            .to_string(),
        command: body["command"].as_str().unwrap_or_default().to_string(),
    }
}

/// 在会话 `chat_id` 里问 `content`：`ctx` 是提问人，`system_texts` 交给模型，这一轮最多跑
/// `deadline_secs` 秒。
pub(crate) async fn prepare_im_post(
    state: &AppState,
    ctx: &IamContext,
    chat_id: &Id,
    content: &str,
    system_texts: Vec<String>,
    deadline_secs: u64,
) -> Result<ImPost> {
    let (chat, viewer) = readable_chat(state, ctx, chat_id).await?;

    let mut req: PostMessageReq = serde_json::from_value(json!({ "content": content }))
        .map_err(|error| Error::internal(format!("an IM question: {error}")))?;

    let mut turn = match commands::apply(state, ctx, &viewer, &chat, &mut req).await {
        Ok(turn) => turn,
        Err(response) => return Ok(refusal(response).await),
    };

    turn.system_texts.extend(system_texts);

    if let BudgetGate::Refuse(_) = state.agent.usage.gate(&ctx.org_id, true).await {
        return Ok(ImPost::BudgetExhausted);
    }

    let question = append_question(state, ctx, &chat, &req, &turn).await?;

    // 命令定下的模式与时间记成会话的；`/query` 这类只管这一轮。
    let mode = (!turn.one_shot_mode)
        .then_some(req.analysis_mode.as_deref())
        .flatten();

    let _ = state
        .agent
        .chats
        .set_chat_context(
            &chat.id,
            mode,
            req.time_range.as_ref().map(|range| range.start_time),
            req.time_range.as_ref().map(|range| range.end_time),
        )
        .await;

    let request = RunRequest::Answer(Box::new(AnswerRequest {
        message: req,
        turn: TurnSnapshot {
            system_texts: turn.system_texts,
            alert_name: turn.alert_name,
            pinned_tools: turn.pinned_tools,
            skill_id: turn.skill_id,
        },
        regenerate: false,
        dashboard: None,
        asked_at: Some(question.created_at.0),
        deadline_secs: Some(deadline_secs),
    }));

    let request = serde_json::to_value(&request)
        .map_err(|error| Error::internal(format!("the request could not be queued: {error}")))?;

    Ok(ImPost::Ready(Box::new(Run::queued(
        ctx.org_id.clone(),
        chat.id.clone(),
        RunSource::Im,
        RunPrincipal::of(ctx),
        request,
        Some(question.id),
    ))))
}

async fn append_question(
    state: &AppState,
    ctx: &IamContext,
    chat: &Chat,
    req: &PostMessageReq,
    turn: &commands::Turn,
) -> Result<ChatMessage> {
    state
        .agent
        .chats
        .append_message(ChatMessage {
            user_id: Some(ctx.user_id.clone()),
            command_json: turn
                .record
                .as_ref()
                .and_then(|record| serde_json::to_value(record).ok()),
            ..ChatMessage::minimal(
                Id::new(),
                chat.id.clone(),
                ctx.org_id.clone(),
                "user",
                req.content.clone(),
                TimestampMicros::now(),
            )
        })
        .await
}
