// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent chat HTTP 路由。
//!
//! - `GET    /api/v1/agent/chat`               列出当前用户活跃会话
//! - `POST   /api/v1/agent/chat`               新建会话
//! - `GET    /api/v1/agent/chat/{id}/messages` 拉历史
//! - `POST   /api/v1/agent/chat/{id}/messages` 发用户消息，排进队列，返回这一轮的 Run
//! - `POST   /api/v1/agent/chat/{id}/compact`  把到现在为止的对话压缩成摘要（`/compact`）
//! - `POST   /api/v1/agent/chat/{id}/archive`  归档 transcript 到对象存储
//! - `DELETE /api/v1/agent/chat/{id}`          归档后软删会话
//!
//! 发消息只核对与排队：消息以 `/名称 参数` 开头时是斜杠命令（`commands`），用户消息存原文和
//! 命令；之后由后台执行这一轮（[`run`]），过程经 `/agent/runs/{id}/events` 续读。
//! - provider 从 PG `agent_model_providers` 行 + 加密 key 构造 adapter（dev 缺省 env fallback）。
//! - prompt 按 purpose 解析（user → org → builtin 默认），渲染白名单变量，持久化 id/version/key/hash。
//! - tool 调用走 `RealToolDispatcher`，org/user 身份恒取自 `IamContext`（忽略 model 传的 org_id）。
//! - tool 证据落 `agent_messages.evidence_json`，大原始结果 spill 到对象存储。
//! - transcript 归档：对象存储 JSON + PG 元数据（object_key/sha256/bytes/status）+ 审计事件。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{budget, references, runs::RunResp, toolsets, viewer::readable_chat};
use crate::{
    agent::{
        capabilities::dashboard_authoring,
        references::MessageReference,
        runs::{Run, RunPrincipal, RunSource},
        tools::AgentExecutionPolicy,
    },
    api::AppState,
    app::{agent_usage::BudgetGate, iam::IamContext},
    domain::iam::permission,
    infra::persistence::repositories::agent::chats::ChatMessage,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

mod archive;
mod commands;
mod evidence;
mod mcp_prompt;
mod messages;
mod prompts;
mod provider;
pub(crate) mod run;
mod skill_invocation;
mod tool_schema;

use archive::archive_chat_route;
use messages::{create_chat, delete_chat, fork_chat, list_chats, list_messages, rename_chat};
use run::request::{AnswerRequest, RunRequest, TurnSnapshot};

/// Agent chat 对象目录布局版本，与 transcript JSON schema 版本独立演进。
const CHAT_OBJECT_LAYOUT_VERSION: &str = "v1";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/agent/chat", get(list_chats).post(create_chat))
        .route(
            "/agent/chat/{id}/messages",
            get(list_messages).post(post_message),
        )
        .route("/agent/chat/{id}/compact", post(compact_chat))
        .route("/agent/chat/{id}/fork", post(fork_chat))
        .route("/agent/chat/{id}/archive", post(archive_chat_route))
        .route(
            "/agent/chat/{id}",
            axum::routing::delete(delete_chat).patch(rename_chat),
        )
}

// ---------------------------------------------------------------------------
// Post message (commands + queue)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRangeReq {
    pub start_micros: i64,
    pub end_micros: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostMessageReq {
    pub content: String,
    #[serde(default)]
    pub regenerate_from_message_id: Option<String>,
    #[serde(default)]
    pub investigation_id: Option<String>,
    #[serde(default)]
    pub time_range: Option<TimeRangeReq>,
    #[serde(default)]
    pub analysis_mode: Option<String>,
    #[serde(default)]
    pub capability: Option<String>,
    #[serde(default)]
    pub execution_policy: Option<AgentExecutionPolicy>,
    #[serde(default)]
    pub stream_hints: Vec<String>,
    #[serde(default)]
    pub agent_profile_id: Option<String>,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub prompt_template_id: Option<String>,
    /// 客户端传 prompt token 数（legacy cost hint）。
    #[serde(default)]
    pub prompt_tokens: Option<i64>,
    #[serde(default)]
    pub completion_tokens: Option<i64>,
    /// 输入框里用 @ 插入的引用。
    #[serde(default)]
    pub references: Vec<MessageReference>,
}

#[permission("agent.use")]
async fn post_message(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(chat_id): Path<String>,
    Json(mut req): Json<PostMessageReq>,
) -> Result<Response> {
    // 只有能读这个会话的人能续聊：个人会话的本人，团队会话的成员与管理员。
    let (chat, viewer) = readable_chat(&state, &ctx, &Id(chat_id)).await?;

    // 0) 斜杠命令：未知命令、缺参数、没有权限时直接拒绝，不落消息、不排队；task 命令改写
    //    这一轮的模式、时间与问题。
    let requested_mode = req.analysis_mode.clone();

    let mut turn = match commands::apply(&state, &ctx, &viewer, &chat, &mut req).await {
        Ok(turn) => turn,
        Err(refusal) => return Ok(refusal),
    };

    // bypass permissions 要这个对话的 Agent Profile 允许；不允许时不落消息。
    if req.execution_policy == Some(AgentExecutionPolicy::BypassPermissions) {
        let profile_id = req.agent_profile_id.as_deref().map(Id::from_string);

        if !toolsets::bypass_permissions_allowed(&state, &ctx.org_id, profile_id.as_ref()).await? {
            return Err(Error::forbidden(
                "this chat's Agent Profile does not allow bypass permissions; an administrator can allow it on the profile",
            ));
        }
    }

    // @ 引用：逐个核对能不能读，写成交给模型的上下文；读不了的让这条消息被拒绝。
    let (attached, reference_context) =
        references::resolve(&state, &ctx, std::mem::take(&mut req.references)).await?;

    turn.system_texts.extend(reference_context);

    // 组织今天的预算：用完且设为拦截时不落用户消息、不排队；接近上限时随回答提醒。
    if let BudgetGate::Refuse(status) = state.agent.usage.gate(&ctx.org_id, true).await {
        return Ok(budget::exhausted_response(&status));
    }

    let dashboard_activation = dashboard_authoring::resolve(
        req.capability.as_deref(),
        req.analysis_mode.as_deref(),
        chat.analysis_mode.as_deref(),
        &turn.model_text,
        req.time_range.is_some(),
    )?;

    // 记成会话的模式；`/query` 只管这一轮。
    let effective_analysis_mode = if dashboard_activation.is_some() {
        Some("dashboard")
    } else if turn.one_shot_mode {
        requested_mode.as_deref()
    } else {
        req.analysis_mode.as_deref()
    };

    // 1) 普通发送落 user 消息（原文，是命令时连同命令）；重新生成则复用指定 user 消息，
    //    执行时历史截到它，不把上一版回答当作下一轮输入。
    let (question_id, asked_at) =
        if let Some(message_id) = req.regenerate_from_message_id.as_deref() {
            regeneration_target(
                &state.agent.chats.list_messages(&chat.id).await?,
                message_id,
                &req.content,
            )?
        } else {
            let saved = state
                .agent
                .chats
                .append_message(ChatMessage {
                    user_id: Some(ctx.user_id.clone()),
                    prompt_tokens: req.prompt_tokens,
                    command_json: turn
                        .record
                        .as_ref()
                        .and_then(|record| serde_json::to_value(record).ok()),
                    references_json: (!attached.is_empty())
                        .then(|| serde_json::to_value(&attached).unwrap_or_default()),
                    ..ChatMessage::minimal(
                        Id::new(),
                        chat.id.clone(),
                        ctx.org_id.clone(),
                        "user",
                        req.content.clone(),
                        TimestampMicros::now(),
                    )
                })
                .await?;

            (saved.id, saved.created_at)
        };

    let _ = state
        .agent
        .chats
        .set_chat_context(
            &chat.id,
            effective_analysis_mode,
            req.time_range.as_ref().map(|t| t.start_micros),
            req.time_range.as_ref().map(|t| t.end_micros),
        )
        .await;

    // 2) 排进队列；同一会话的 Run 一个接一个执行。
    let regenerate = req.regenerate_from_message_id.is_some();

    let request = RunRequest::Answer(Box::new(AnswerRequest {
        message: req,
        turn: TurnSnapshot {
            system_texts: turn.system_texts,
            alert_name: turn.alert_name,
            pinned_tools: turn.pinned_tools,
            skill_id: turn.skill_id,
        },
        regenerate,
        dashboard: dashboard_activation.map(|activation| activation.input_complete),
        asked_at_micros: Some(asked_at.0),
    }));

    let run = enqueue(&state, &ctx, &chat.id, &request, Some(question_id.clone())).await?;

    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "run": RunResp::of(&run), "user_message_id": question_id.0 })),
    )
        .into_response())
}

/// `/compact`：把到现在为止的对话压缩成摘要，之后的回答只带摘要和新消息。
#[permission("agent.use")]
async fn compact_chat(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(chat_id): Path<String>,
) -> Result<Response> {
    let (chat, _) = readable_chat(&state, &ctx, &Id(chat_id)).await?;

    if let BudgetGate::Refuse(status) = state.agent.usage.gate(&ctx.org_id, true).await {
        return Ok(budget::exhausted_response(&status));
    }

    let run = enqueue(&state, &ctx, &chat.id, &RunRequest::Compact, None).await?;

    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "run": RunResp::of(&run) })),
    )
        .into_response())
}

async fn enqueue(
    state: &AppState,
    ctx: &IamContext,
    chat_id: &Id,
    request: &RunRequest,
    question: Option<Id>,
) -> Result<Run> {
    let request = serde_json::to_value(request)
        .map_err(|error| Error::internal(format!("the request could not be queued: {error}")))?;

    state
        .agent
        .runs
        .create(Run::queued(
            ctx.org_id.clone(),
            chat_id.clone(),
            RunSource::Chat,
            RunPrincipal::of(ctx),
            request,
            question,
        ))
        .await
}

/// 重新生成的那条用户消息：必须是本会话的用户消息，内容与原文一致。
fn regeneration_target(
    rows: &[ChatMessage],
    message_id: &str,
    content: &str,
) -> Result<(Id, TimestampMicros)> {
    let original = rows
        .iter()
        .find(|message| message.id.0 == message_id && message.role == "user")
        .ok_or_else(|| Error::invalid("regenerate target must be a user message in this chat"))?;

    if original.content.trim() != content.trim() {
        return Err(Error::invalid(
            "regenerate content must match the original user message",
        ));
    }

    Ok((original.id.clone(), original.created_at))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regeneration_reuses_the_user_question() {
        let now = TimestampMicros(1);
        let chat_id = Id("chat-1".into());
        let org_id = Id("org-1".into());
        let user = ChatMessage::minimal(
            Id("user-1".into()),
            chat_id.clone(),
            org_id.clone(),
            "user",
            "谁正在负责生产环境值班？",
            now,
        );
        let answer = ChatMessage::minimal(
            Id("answer-1".into()),
            chat_id,
            org_id,
            "assistant",
            "旧回答",
            now,
        );

        let rows = vec![user.clone(), answer];
        let (id, asked_at) =
            regeneration_target(&rows, "user-1", " 谁正在负责生产环境值班？ ").unwrap();

        assert_eq!(id, user.id);
        assert_eq!(asked_at, now);
        assert!(regeneration_target(&rows, "answer-1", "旧回答").is_err());
        assert!(regeneration_target(&rows, "user-1", "别的问题").is_err());
    }
}
