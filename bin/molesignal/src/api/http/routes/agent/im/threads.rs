// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一条 IM 消息接着哪个会话。
//!
//! 已有话题接着它的会话，提问人要能续聊它：个人会话只有本人，团队会话是成员。新话题开一个会话：
//! 群里、机器人设了默认团队且提问人是成员时是团队会话，否则是提问人的个人会话。`/new` 与
//! `/scope` 开新会话，这个话题之后接着新的；旧会话留在 MoleSignal 里。

use crate::{
    agent::{
        im::{ConversationKind, ImBot, ImThread, Inbound},
        scope::{ChatAccess, Viewer},
    },
    api::{
        AppState,
        http::{interface_language::interface_language, routes::agent::viewer::access},
    },
    app::iam::IamContext,
    infra::persistence::repositories::agent::chats::Chat,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 新会话记在哪里。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum NewScope {
    Personal,
    Team { id: Id, name: String },
}

/// 不能接着这个话题的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Refusal {
    /// 别人的个人会话。
    NotYours,
    /// 一个团队的会话，提问人不在这个团队。
    NotMember(String),
}

pub(super) async fn team_name(state: &AppState, team_id: &str) -> String {
    state
        .iam
        .teams
        .get(&Id::from_string(team_id))
        .await
        .map(|team| team.name)
        .unwrap_or_else(|_| team_id.to_string())
}

/// 话题现在的会话：能续聊时是它，不能时是原因；会话已经删了时是 `None`。
pub(super) async fn current(
    state: &AppState,
    ctx: &IamContext,
    viewer: &Viewer,
    thread: &ImThread,
) -> Result<Option<std::result::Result<Chat, Refusal>>> {
    let chat = match state
        .agent
        .chats
        .get_chat(&ctx.org_id, &thread.chat_id)
        .await
    {
        Ok(chat) => chat,
        Err(Error::NotFound(_)) => return Ok(None),
        Err(error) => return Err(error),
    };

    if matches!(access(viewer, &chat), ChatAccess::Full) {
        return Ok(Some(Ok(chat)));
    }

    let refusal = match (chat.scope.as_str(), chat.team_id.as_deref()) {
        ("team", Some(team_id)) => Refusal::NotMember(team_name(state, team_id).await),
        _ => Refusal::NotYours,
    };

    Ok(Some(Err(refusal)))
}

/// 新话题的会话记在哪里：群里用机器人的默认团队（提问人是成员时）。
pub(super) async fn default_scope(
    state: &AppState,
    bot: &ImBot,
    viewer: &Viewer,
    kind: ConversationKind,
) -> NewScope {
    match (&bot.default_team_id, kind) {
        (Some(team_id), ConversationKind::Group) if viewer.is_member(team_id) => NewScope::Team {
            id: team_id.clone(),
            name: team_name(state, &team_id.0).await,
        },
        _ => NewScope::Personal,
    }
}

/// 一个团队的名称或 ID 对应提问人所在的哪个团队。
pub(super) async fn member_team(
    state: &AppState,
    ctx: &IamContext,
    wanted: &str,
) -> Result<Option<NewScope>> {
    let wanted = wanted.trim().trim_start_matches('@');
    let teams = state
        .iam
        .teams
        .list_for_member(&ctx.org_id, &ctx.user_id)
        .await?;

    Ok(teams
        .into_iter()
        .find(|team| team.id.0 == wanted || team.name.eq_ignore_ascii_case(wanted))
        .map(|team| NewScope::Team {
            id: team.id,
            name: team.name,
        }))
}

/// 会话的标题：问题的开头，等模型回答时起正式的名字。
fn title(text: &str) -> String {
    let line = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    let mut title: String = line.trim().chars().take(40).collect();

    if title.is_empty() {
        title = "IM".into();
    }

    title
}

/// 给这个话题开一个新会话，话题之后接着它。`question` 是会话的第一个问题（`/new` 与
/// `/scope` 开的会话还没有）。
pub(super) async fn open(
    state: &AppState,
    bot: &ImBot,
    ctx: &IamContext,
    inbound: &Inbound,
    scope: &NewScope,
    question: Option<&str>,
) -> Result<Chat> {
    let now = TimestampMicros::now();
    let mut chat = Chat::minimal(
        Id::new(),
        ctx.org_id.clone(),
        ctx.user_id.clone(),
        String::new(),
        String::new(),
        title(question.unwrap_or_default()),
        now,
    );

    chat.title_provisional = true;
    chat.reply_language = interface_language(state, ctx).await.unwrap_or_default();

    if let NewScope::Team { id, .. } = scope {
        chat.scope = "team".into();
        chat.team_id = Some(id.0.clone());
    }

    let chat = state.agent.chats.create_chat(chat).await?;

    state
        .agent
        .im
        .put_thread(&ImThread {
            id: Id::new(),
            org_id: ctx.org_id.clone(),
            connector_id: bot.connector_id.clone(),
            conversation_id: inbound.conversation_id.clone(),
            thread_key: inbound.thread_key.clone(),
            chat_id: chat.id.clone(),
            created_by: ctx.user_id.clone(),
            created_at: now,
            updated_at: now,
        })
        .await?;

    Ok(chat)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chat_is_titled_by_the_first_line_of_its_question() {
        assert_eq!(
            title("\n  checkout is slow\nsince 10:00"),
            "checkout is slow"
        );
        assert_eq!(title(""), "IM");
        assert_eq!(title(&"x".repeat(80)).chars().count(), 40);
    }
}
