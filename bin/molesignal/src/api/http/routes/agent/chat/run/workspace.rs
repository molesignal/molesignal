// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一轮回答开始时会话自动带上的东西：知识的入口文件、记忆目录，以及知识、记忆与调查工具要用
//! 的会话位置（[`ChatWorkspace`]）。
//!
//! 团队会话加载组织与本队的 `MOLE.md`，个人会话加载组织的与已挂进来的团队的；记忆目录是这个
//! 会话写记忆的地方的：团队会话是本队的，个人会话是发起人的。

use crate::{
    agent::{
        knowledge::{ENTRY_FILE, entry_prompt},
        memory::index_prompt,
        scope::{ChatScope, Scope},
    },
    api::{
        AppState,
        http::routes::agent::{
            tool_dispatcher::ChatWorkspace,
            viewer::{chat_scope, viewer},
        },
    },
    app::iam::IamContext,
    infra::persistence::repositories::agent::chats::Chat,
    shared::ids::Id,
};

pub(super) struct Loaded {
    pub(super) workspace: ChatWorkspace,
    /// 交给模型的知识与记忆。
    pub(super) system_texts: Vec<String>,
    /// 这一轮另外直接交给模型的工具。
    pub(super) tools: Vec<String>,
}

pub(super) async fn load(state: &AppState, ctx: &IamContext, chat: &Chat) -> Loaded {
    let viewer = viewer(state, ctx).await.unwrap_or_else(|error| {
        tracing::warn!(%error, "the teams of the person who asked could not be read");
        Default::default()
    });

    let teams = state
        .iam
        .teams
        .list(&ctx.org_id)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|team| (team.id, team.name))
        .collect::<Vec<_>>();

    let workspace = ChatWorkspace {
        chat_id: chat.id.clone(),
        scope: chat_scope(chat),
        viewer,
        teams,
        mounted: Default::default(),
    };

    let mut system_texts = Vec::new();
    system_texts.extend(knowledge_entries(state, ctx, &workspace).await);
    system_texts.extend(memory_index(state, ctx, &workspace).await);

    // 能写这个会话的知识时直接给出改知识的工具；有调查时给出记录调查的工具。
    let mut tools = Vec::new();
    let write_scope = workspace
        .scope
        .team_id()
        .map_or(Scope::Org, |team_id| Scope::Team {
            team_id: team_id.clone(),
        });

    if workspace.viewer.can_edit(&write_scope) {
        tools.push("edit_knowledge".to_string());
    }

    if chat.investigation_id.is_some() {
        tools.push("update_investigation".to_string());
    }

    Loaded {
        workspace,
        system_texts,
        tools,
    }
}

fn team_name(workspace: &ChatWorkspace, team_id: &Id) -> String {
    workspace
        .teams
        .iter()
        .find(|(id, _)| id == team_id)
        .map_or_else(|| team_id.0.clone(), |(_, name)| name.clone())
}

async fn knowledge_entries(
    state: &AppState,
    ctx: &IamContext,
    workspace: &ChatWorkspace,
) -> Option<String> {
    let teams = match &workspace.scope {
        ChatScope::Team { team_id } => vec![team_id.clone()],
        ChatScope::Personal { mounted_teams, .. } => mounted_teams.clone(),
    };

    let mut sources = vec![("organization".to_string(), Scope::Org)];

    sources.extend(teams.into_iter().map(|team_id| {
        (
            format!("team {}", team_name(workspace, &team_id)),
            Scope::Team { team_id },
        )
    }));

    let mut entries = Vec::new();

    for (source, scope) in sources {
        match state
            .agent
            .knowledge
            .get(&ctx.org_id, &scope, ENTRY_FILE)
            .await
        {
            Ok(Some(file)) if !file.content.trim().is_empty() => {
                entries.push((source, file.content))
            }
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "a knowledge entry file could not be read"),
        }
    }

    entry_prompt(&entries)
}

async fn memory_index(
    state: &AppState,
    ctx: &IamContext,
    workspace: &ChatWorkspace,
) -> Option<String> {
    let (scope, owner) = match &workspace.scope {
        ChatScope::Team { team_id } => (
            Scope::Team {
                team_id: team_id.clone(),
            },
            format!("the team {}", team_name(workspace, team_id)),
        ),
        ChatScope::Personal { owner_id, .. } => (
            Scope::Personal {
                owner_id: owner_id.clone(),
            },
            "the person you are talking to".to_string(),
        ),
    };

    match state.agent.memory.list(&ctx.org_id, &scope).await {
        Ok(memories) => index_prompt(&memories, &owner),
        Err(error) => {
            tracing::warn!(%error, "the memories of a chat could not be read");
            None
        }
    }
}
