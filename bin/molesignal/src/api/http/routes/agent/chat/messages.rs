// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 会话与消息的列出、新建、删除。
//!
//! 会话分个人与团队两种：个人会话只有发起人能读，管理员只看到元数据；团队会话由团队成员和
//! 管理员共用。新建时选定，之后不再改变。

use std::collections::HashMap;

use axum::{
    Extension, Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{
    super::{
        investigate::{SubjectReq, start as start_investigation},
        viewer::{self, readable_chat},
    },
    archive::archive_chat,
};
use crate::{
    agent::{
        commands::{CommandRecord, CommandSource},
        scope::{ChatAccess, Scope, Viewer, can_delete_chat},
        usage::Cost,
    },
    api::{AppState, http::interface_language::interface_language},
    app::iam::IamContext,
    domain::iam::permission,
    infra::persistence::repositories::agent::chats::{Chat, ChatMessage},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateChatReq {
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub title: String,
    /// 标题是对话页取自第一个问题的临时标题：回答时由模型起名。
    #[serde(default)]
    pub title_provisional: bool,
    /// 可选：绑定 PG provider 行 id（区别于 provider 类型串）。
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub analysis_mode: Option<String>,
    #[serde(default)]
    pub capability: Option<String>,
    /// 团队会话所属的团队；不填是个人会话。
    #[serde(default)]
    pub team_id: Option<String>,
    /// 会话调查的对象：从事件页发起时是那个事件。
    #[serde(default)]
    pub subject: Option<SubjectReq>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatResp {
    pub id: String,
    pub provider: String,
    pub model: String,
    pub title: String,
    pub provider_id: Option<String>,
    pub analysis_mode: Option<String>,
    pub capability: Option<String>,
    pub time_range_start: Option<i64>,
    pub time_range_end: Option<i64>,
    pub archive_object_key: Option<String>,
    /// `personal`、`team` 或 `org`（组织作用域的自动化）。
    pub scope: String,
    pub team_id: Option<String>,
    pub mounted_team_ids: Vec<String>,
    /// 发起会话的人。
    pub owner_id: String,
    /// 当前用户能读、能续聊；为否时只有元数据。
    pub readable: bool,
    /// 会话调查的对象（`incident` 与事件 ID）与它的调查记录。
    pub subject_kind: Option<String>,
    pub subject_id: Option<String>,
    pub investigation_id: Option<String>,
    /// 从哪个会话分叉出来的。
    pub parent_chat_id: Option<String>,
    /// 自动化的一次运行：会话只读，页面说明出自哪条规则。
    pub automation_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// 用户消息是命令时的命令：界面据此画命令胶囊。交给模型的展开问题不返回。
#[derive(Debug, Serialize)]
pub struct MessageCommandResp {
    pub name: String,
    pub args: String,
    pub source: CommandSource,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessageResp {
    pub id: String,
    pub chat_id: String,
    pub org_id: String,
    pub role: String,
    pub content: String,
    pub tool_calls_json: Option<Value>,
    pub tool_result_for: Option<String>,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub cost: Option<Cost>,
    pub prompt_template_id: Option<String>,
    pub prompt_builtin_key: Option<String>,
    pub prompt_version: Option<i32>,
    pub prompt_hash: Option<String>,
    pub evidence_json: Option<Value>,
    pub command: Option<MessageCommandResp>,
    /// 用户消息里用 @ 插入的引用。
    pub references: Option<Value>,
    /// 回答出自哪个 Run：界面据此找到这一轮的状态（停止、中断）。
    pub run_id: Option<String>,
    /// 用户消息的发送人与其显示名：团队会话里问题可能来自不同的人。
    pub sender_id: Option<String>,
    pub sender_name: Option<String>,
    pub created_at: i64,
}

pub(super) fn to_chat_resp(s: Chat, viewer: &Viewer) -> ChatResp {
    let capability = (s.analysis_mode.as_deref() == Some("dashboard"))
        .then(|| "dashboard_authoring".to_string());

    let readable = viewer::access(viewer, &s) == ChatAccess::Full;

    ChatResp {
        id: s.id.0,
        provider: s.provider,
        model: s.model,
        title: s.title,
        provider_id: s.provider_id,
        analysis_mode: s.analysis_mode,
        capability,
        time_range_start: s.time_range_start,
        time_range_end: s.time_range_end,
        archive_object_key: s.archive_object_key,
        scope: s.scope,
        team_id: s.team_id,
        mounted_team_ids: s.mounted_team_ids,
        owner_id: s.user_id.0,
        readable,
        subject_kind: s.subject_kind,
        subject_id: s.subject_id,
        investigation_id: s.investigation_id,
        parent_chat_id: s.parent_chat_id,
        automation_id: s.automation_id,
        created_at: s.created_at.0,
        updated_at: s.updated_at.0,
    }
}

/// 消息上记下的命令；读不出时当作没有。
pub(super) fn command_of(message: &ChatMessage) -> Option<CommandRecord> {
    message
        .command_json
        .clone()
        .and_then(|value| serde_json::from_value(value).ok())
}

#[cfg(test)]
fn to_message_resp(message: ChatMessage) -> ChatMessageResp {
    to_message_resp_named(message, &HashMap::new())
}

/// `names` 是发送人的显示名。
fn to_message_resp_named(message: ChatMessage, names: &HashMap<String, String>) -> ChatMessageResp {
    let sender_id = message.user_id.as_ref().map(|id| id.0.clone());
    let sender_name = sender_id.as_ref().and_then(|id| names.get(id).cloned());

    let command = command_of(&message).map(|record| MessageCommandResp {
        name: record.name,
        args: record.args,
        source: record.source,
    });

    ChatMessageResp {
        id: message.id.0,
        chat_id: message.chat_id.0,
        org_id: message.org_id.0,
        role: message.role,
        content: message.content,
        tool_calls_json: message.tool_calls_json,
        tool_result_for: message.tool_result_for,
        prompt_tokens: message.prompt_tokens,
        completion_tokens: message.completion_tokens,
        cost: message.cost,
        prompt_template_id: message.prompt_template_id,
        prompt_builtin_key: message.prompt_builtin_key,
        prompt_version: message.prompt_version,
        prompt_hash: message.prompt_hash,
        evidence_json: message.evidence_json,
        command,
        references: message.references_json,
        run_id: message.run_id.map(|id| id.0),
        sender_id,
        sender_name,
        created_at: message.created_at.0,
    }
}

/// 本人的个人会话，加上本人所在团队的团队会话，最近的在前。
#[permission("agent.use")]
pub(super) async fn list_chats(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<ChatResp>>> {
    let viewer = viewer::viewer(&state, &ctx).await?;

    let team_ids = viewer
        .teams
        .iter()
        .map(|id| id.0.clone())
        .collect::<Vec<_>>();

    let mut chats = state
        .agent
        .chats
        .list_chats(&ctx.org_id, &ctx.user_id)
        .await?;

    chats.extend(
        state
            .agent
            .chats
            .list_team_chats(&ctx.org_id, &team_ids)
            .await?,
    );

    chats.sort_by_key(|chat| std::cmp::Reverse(chat.updated_at.0));

    Ok(Json(
        chats
            .into_iter()
            .map(|chat| to_chat_resp(chat, &viewer))
            .collect(),
    ))
}

/// 一个能读的会话。不在对话列表里的（自动化的运行）也从这里读。
#[permission("agent.use")]
pub(super) async fn get_chat(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<ChatResp>> {
    let (chat, viewer) = readable_chat(&state, &ctx, &Id(id)).await?;
    Ok(Json(to_chat_resp(chat, &viewer)))
}

#[permission("agent.use")]
pub(super) async fn create_chat(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<CreateChatReq>,
) -> Result<Json<ChatResp>> {
    let now = TimestampMicros::now();

    let mut s = Chat::minimal(
        Id::new(),
        ctx.org_id.clone(),
        ctx.user_id.clone(),
        req.provider,
        req.model,
        req.title,
        now,
    );

    s.provider_id = req.provider_id;
    s.title_provisional = req.title_provisional;

    // 回答用发起人此刻的界面语言，之后不变。
    s.reply_language = interface_language(&state, &ctx)
        .await
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "the interface language of a new chat could not be read");
            String::new()
        });

    let viewer = viewer::viewer(&state, &ctx).await?;

    if let Some(team_id) = req
        .team_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        let team = state.iam.teams.get(&Id::from_string(team_id)).await?;

        if team.org_id != ctx.org_id {
            return Err(Error::not_found(format!("team `{team_id}` not found")));
        }

        if !viewer.can_edit(&Scope::Team {
            team_id: team.id.clone(),
        }) {
            return Err(Error::forbidden(
                "only the team's members can start a chat for the team",
            ));
        }

        s.scope = "team".into();
        s.team_id = Some(team.id.0);
    }

    // 从事件发起：核对能读它，建调查记录。
    if let Some(subject) = &req.subject {
        start_investigation(&state, &ctx, &mut s, subject).await?;
    }

    s.analysis_mode = match req.capability.as_deref() {
        Some("dashboard_authoring") => Some("dashboard".into()),
        Some(other) => {
            return Err(Error::invalid(format!(
                "unknown Mole Agent capability `{other}`"
            )));
        }
        None => req.analysis_mode,
    };

    Ok(Json(to_chat_resp(
        state.agent.chats.create_chat(s).await?,
        &viewer,
    )))
}

#[permission("agent.use")]
pub(super) async fn delete_chat(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let chat = state
        .agent
        .chats
        .get_chat(&ctx.org_id, &Id(id.clone()))
        .await?;
    let viewer = viewer::viewer(&state, &ctx).await?;

    if !can_delete_chat(&viewer, &viewer::chat_scope(&chat), &chat.user_id) {
        return Err(Error::not_found(format!("chat `{id}` not found")));
    }

    // 软删前先尽力归档，保证删除的会话仍有 transcript 留档（best-effort）。
    let _ = archive_chat(&state, &ctx, &chat).await;

    state.agent.chats.delete_chat(&ctx.org_id, &chat.id).await?;
    Ok(Json(json!({ "deleted": true })))
}

/// 会话名最长这么多字符。
const MAX_TITLE_CHARS: usize = 255;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenameChatReq {
    pub title: String,
}

/// 改会话的名字，之后不再由模型起名。和删除同一条规则：会话的发起人与管理员能改。
#[permission("agent.use")]
pub(super) async fn rename_chat(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<RenameChatReq>,
) -> Result<Json<ChatResp>> {
    let title = req.title.trim();

    if title.is_empty() || title.chars().count() > MAX_TITLE_CHARS {
        return Err(Error::invalid("a chat's name is 1 to 255 characters"));
    }

    let chat = state
        .agent
        .chats
        .get_chat(&ctx.org_id, &Id(id.clone()))
        .await?;
    let viewer = viewer::viewer(&state, &ctx).await?;
    let scope = viewer::chat_scope(&chat);

    if !can_delete_chat(&viewer, &scope, &chat.user_id) {
        if viewer::access(&viewer, &chat) == ChatAccess::Full {
            return Err(Error::forbidden(
                "only the person who started this chat and administrators can rename it",
            ));
        }

        return Err(Error::not_found(format!("chat `{id}` not found")));
    }

    state.agent.chats.rename_chat(&chat.id, title).await?;

    let renamed = Chat {
        title: title.to_string(),
        title_provisional: false,
        ..chat
    };

    Ok(Json(to_chat_resp(renamed, &viewer)))
}

#[permission("agent.use")]
pub(super) async fn list_messages(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let (chat, _) = readable_chat(&state, &ctx, &Id(id)).await?;
    let msgs = state.agent.chats.list_messages(&chat.id).await?;
    let names = sender_names(&state, &msgs).await;

    Ok(Json(json!({
        "messages": msgs
            .into_iter()
            .map(|message| to_message_resp_named(message, &names))
            .collect::<Vec<_>>()
    })))
}

/// 发送人的显示名；读不到时界面只显示头像。
async fn sender_names(state: &AppState, messages: &[ChatMessage]) -> HashMap<String, String> {
    let mut ids = messages
        .iter()
        .filter_map(|message| message.user_id.clone())
        .collect::<Vec<_>>();

    ids.sort_by(|left, right| left.0.cmp(&right.0));
    ids.dedup();

    if ids.is_empty() {
        return HashMap::new();
    }

    match state.iam.service.users.get_many(&ids).await {
        Ok(users) => users
            .into_iter()
            .map(|user| {
                let name = if user.display_name.trim().is_empty() {
                    user.email
                } else {
                    user.display_name
                };

                (user.id.0, name)
            })
            .collect(),
        Err(error) => {
            tracing::warn!(%error, "chat sender names could not be read");
            HashMap::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(content: &str) -> ChatMessage {
        ChatMessage::minimal(
            Id("message-1".into()),
            Id("chat-1".into()),
            Id("org-1".into()),
            "user",
            content,
            TimestampMicros(42),
        )
    }

    #[test]
    fn message_response_uses_frontend_timestamp_contract() {
        let value = serde_json::to_value(to_message_resp(message("回答"))).unwrap();

        assert_eq!(value["createdAt"], 42);
        assert!(value.get("createdAtMicros").is_none());
        assert!(value["command"].is_null());
    }

    #[test]
    fn a_command_is_returned_without_the_question_given_to_the_model() {
        let mut command = message("/query errors by service");
        command.command_json = Some(json!({
            "name": "query",
            "args": "errors by service",
            "source": "builtin",
            "modelText": "errors by service",
        }));

        let value = serde_json::to_value(to_message_resp(command)).unwrap();

        assert_eq!(value["content"], "/query errors by service");
        assert_eq!(value["command"]["name"], "query");
        assert_eq!(value["command"]["source"], "builtin");
        assert!(value["command"].get("model_text").is_none());
    }
}
