// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 显式调用 Skill：`/名称 参数` 或 `/名称@团队 参数`。
//!
//! 个人会话里写 `@团队` 时，先确认本人是这个团队的成员（管理员也可以），再把团队挂进会话；
//! 团队会话只用本队的 Skill。Skill 正文填好参数后作为这一轮的一条 system 指令，它的
//! `allowed-tools` 这一轮直接交给模型，但仍受组织工具策略与审批约束。要在 Runner 上跑脚本的
//! Skill 还不能用。

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

use super::{
    super::viewer::chat_scope,
    commands::{
        COMMAND_AMBIGUOUS, COMMAND_NEEDS_RUNNER, COMMAND_TARGET_NOT_FOUND, COMMAND_UNAVAILABLE,
        Turn, UNKNOWN_COMMAND, refuse, refuse_with,
    },
};
use crate::{
    agent::{
        commands::{CommandRecord, CommandSource, Invocation},
        scope::{ChatScope, Viewer},
        skills::{render_invocation, wrap_skill_body},
    },
    api::{AppState, http::interface_language::interface_language_pack},
    app::{
        iam::IamContext,
        skills::{Lookup, team_handle},
    },
    domain::iam::Team,
    infra::persistence::repositories::agent::chats::Chat,
    shared::Result,
};

/// `@` 后面写的团队：去掉空白的团队名（不分大小写）或团队 ID。
async fn find_team(state: &AppState, ctx: &IamContext, handle: &str) -> Result<Option<Team>> {
    let handle = handle.to_lowercase();

    Ok(state
        .iam
        .teams
        .list(&ctx.org_id)
        .await?
        .into_iter()
        .find(|team| team_handle(&team.name).to_lowercase() == handle || team.id.0 == handle))
}

pub(super) async fn invoke_skill(
    state: &AppState,
    ctx: &IamContext,
    viewer: &Viewer,
    chat: &Chat,
    invocation: &Invocation,
) -> std::result::Result<Turn, Response> {
    let shown = match &invocation.team {
        Some(team) => format!("{}@{team}", invocation.name),
        None => invocation.name.clone(),
    };

    let mut scope = chat_scope(chat);

    if let Some(handle) = invocation.team.as_deref() {
        let team = match find_team(state, ctx, handle).await {
            Ok(Some(team)) => team,
            Ok(None) => {
                return Err(refuse(
                    StatusCode::NOT_FOUND,
                    COMMAND_TARGET_NOT_FOUND,
                    &shown,
                ));
            }
            Err(error) => return Err(error.into_response()),
        };

        match &mut scope {
            // 团队会话只用本队的 Skill。
            ChatScope::Team { team_id } if *team_id != team.id => {
                return Err(refuse(StatusCode::BAD_REQUEST, COMMAND_UNAVAILABLE, &shown));
            }
            ChatScope::Team { .. } => {}
            ChatScope::Personal { mounted_teams, .. } => {
                if !viewer.manager && !viewer.is_member(&team.id) {
                    return Err(refuse(StatusCode::FORBIDDEN, COMMAND_UNAVAILABLE, &shown));
                }

                if !mounted_teams.contains(&team.id) {
                    if let Err(error) = state.agent.chats.mount_team(&chat.id, &team.id.0).await {
                        return Err(error.into_response());
                    }

                    mounted_teams.push(team.id.clone());
                }
            }
        }
    }

    let session = match state.agent.skills.session(&ctx.org_id, &scope).await {
        Ok(session) => session,
        Err(error) => return Err(error.into_response()),
    };

    let skill = match session.find(&invocation.name, invocation.team.as_deref()) {
        Lookup::Found(skill) => skill.clone(),
        Lookup::Ambiguous(options) => {
            return Err(refuse_with(
                StatusCode::CONFLICT,
                COMMAND_AMBIGUOUS,
                &shown,
                options,
            ));
        }
        Lookup::Missing => return Err(refuse(StatusCode::BAD_REQUEST, UNKNOWN_COMMAND, &shown)),
    };

    if skill.manifest.requires_runner {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            COMMAND_NEEDS_RUNNER,
            &shown,
        ));
    }

    let bundle = match state.agent.skills.bundle(&skill.location).await {
        Ok(bundle) => bundle,
        Err(error) => return Err(error.into_response()),
    };

    let body = render_invocation(&bundle.body, &invocation.args);

    let model_text = if invocation.args.is_empty() {
        interface_language_pack(state, ctx, "agent.command.skill")
            .await
            .render(
                "agent.command.skill.run",
                &[("name", skill.invocation.as_str())],
            )
            .unwrap_or_else(|_| format!("Follow the skill {}.", skill.invocation))
    } else {
        invocation.args.clone()
    };

    Ok(Turn {
        model_text: model_text.clone(),
        record: Some(CommandRecord {
            name: shown,
            args: invocation.args.clone(),
            source: CommandSource::Skill,
            model_text,
            skill_id: Some(skill.id.clone()),
            skill_version: skill.version.clone(),
            skill_hash: Some(skill.hash.clone()),
            ..CommandRecord::default()
        }),
        system_texts: vec![wrap_skill_body(
            &skill.invocation,
            skill.version.as_deref(),
            &body,
        )],
        pinned_tools: skill.manifest.allowed_tools.clone(),
        skill_id: Some(skill.id),
        ..Turn::default()
    })
}
