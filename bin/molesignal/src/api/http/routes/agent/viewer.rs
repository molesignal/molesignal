// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 发起请求的人在 Agent 作用域里是谁：所在的团队、是否持有 `agent.manage`，以及他对一个
//! 会话的访问程度。规则本身在 `agent::scope`。

use crate::{
    agent::scope::{ChatAccess, ChatScope, Scope, Viewer, chat_access},
    api::AppState,
    app::iam::IamContext,
    infra::persistence::repositories::agent::chats::Chat,
    shared::{Error, Result, ids::Id},
};

pub(crate) async fn viewer(state: &AppState, ctx: &IamContext) -> Result<Viewer> {
    let teams = state
        .iam
        .teams
        .list_for_member(&ctx.org_id, &ctx.user_id)
        .await?;

    Ok(Viewer {
        user_id: ctx.user_id.clone(),
        manager: ctx.has_permission("agent.manage"),
        teams: teams.into_iter().map(|team| team.id).collect(),
    })
}

pub(crate) fn chat_scope(chat: &Chat) -> ChatScope {
    match (chat.scope.as_str(), chat.team_id.as_deref()) {
        ("team", Some(team_id)) => ChatScope::Team {
            team_id: Id::from_string(team_id),
        },
        _ => ChatScope::Personal {
            owner_id: chat.user_id.clone(),
            mounted_teams: chat
                .mounted_team_ids
                .iter()
                .map(|id| Id::from_string(id.as_str()))
                .collect(),
        },
    }
}

pub(crate) fn access(viewer: &Viewer, chat: &Chat) -> ChatAccess {
    chat_access(viewer, &chat_scope(chat), &chat.user_id)
}

/// 一个能读、能续聊的会话。看不到的会话与不存在一样回 404，不暴露它的存在。
pub(crate) async fn readable_chat(
    state: &AppState,
    ctx: &IamContext,
    chat_id: &Id,
) -> Result<(Chat, Viewer)> {
    let chat = state.agent.chats.get_chat(&ctx.org_id, chat_id).await?;
    let viewer = viewer(state, ctx).await?;

    match access(&viewer, &chat) {
        ChatAccess::Full => Ok((chat, viewer)),
        ChatAccess::Metadata => Err(Error::forbidden(
            "a personal chat can only be read by the person who started it",
        )),
        ChatAccess::None => Err(Error::not_found(format!("chat `{}` not found", chat_id.0))),
    }
}

/// 请求里的作用域：`org`、`team`（带 `team_id`）或 `personal`（就是发请求的人）。团队要在本组织。
pub(crate) async fn requested_scope(
    state: &AppState,
    ctx: &IamContext,
    kind: &str,
    team_id: Option<&str>,
) -> Result<Scope> {
    match kind {
        "org" => Ok(Scope::Org),
        "personal" => Ok(Scope::Personal {
            owner_id: ctx.user_id.clone(),
        }),
        "team" => {
            let team_id = team_id
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .ok_or_else(|| Error::invalid("a team scope needs team_id"))?;

            let team = state.iam.teams.get(&Id::from_string(team_id)).await?;

            if team.org_id != ctx.org_id {
                return Err(Error::not_found(format!("team `{team_id}` not found")));
            }

            Ok(Scope::Team { team_id: team.id })
        }
        other => Err(Error::invalid(format!("unknown scope `{other}`"))),
    }
}
