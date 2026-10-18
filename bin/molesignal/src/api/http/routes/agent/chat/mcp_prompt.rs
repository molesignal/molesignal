// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 调用 MCP 服务器提供的 prompt：`/<handle>:<prompt> 参数`。
//!
//! 只能用这个会话里能用的服务器的 prompt；个人会话里调用本人所在团队的服务器的 prompt 时，
//! 先取到 prompt，再把团队挂进会话。参数按 `agent::mcp::bind_arguments` 填，`prompts/get` 用
//! 发消息的人自己的凭据。prompt 的消息合成一段文字，作为这一轮交给模型的问题。

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

use super::{
    super::viewer::chat_scope,
    commands::{
        COMMAND_ARGUMENT_MISSING, COMMAND_FAILED, COMMAND_NEEDS_CREDENTIAL, Turn, UNKNOWN_COMMAND,
        refuse, refuse_with,
    },
};
use crate::{
    agent::{
        commands::{CommandRecord, CommandSource, Invocation},
        mcp::{bind_arguments, prompt_text},
        scope::Viewer,
    },
    api::AppState,
    app::{iam::IamContext, mcp::safe_runtime_error},
    infra::persistence::repositories::agent::chats::Chat,
    shared::Error,
};

/// prompt 展开后的文字上限。
const MAX_PROMPT_BYTES: usize = 64 * 1024;

pub(super) async fn invoke_mcp_prompt(
    state: &AppState,
    ctx: &IamContext,
    viewer: &Viewer,
    chat: &Chat,
    invocation: &Invocation,
) -> std::result::Result<Turn, Response> {
    let shown = invocation.name.clone();

    if invocation.team.is_some() {
        return Err(refuse(StatusCode::BAD_REQUEST, UNKNOWN_COMMAND, &shown));
    }

    let prompts = match state
        .agent
        .mcp
        .session_prompts(&ctx.org_id, viewer, &chat_scope(chat))
        .await
    {
        Ok(prompts) => prompts,
        Err(error) => return Err(error.into_response()),
    };

    let Some(found) = prompts
        .into_iter()
        .find(|prompt| prompt.command == invocation.name)
    else {
        return Err(refuse(StatusCode::BAD_REQUEST, UNKNOWN_COMMAND, &shown));
    };

    if found.needs_credential {
        return Err(refuse(
            StatusCode::FORBIDDEN,
            COMMAND_NEEDS_CREDENTIAL,
            &shown,
        ));
    }

    let arguments = match bind_arguments(&found.prompt.arguments, &invocation.args) {
        Ok(arguments) => arguments,
        Err(missing) => {
            return Err(refuse_with(
                StatusCode::BAD_REQUEST,
                COMMAND_ARGUMENT_MISSING,
                &shown,
                vec![missing],
            ));
        }
    };

    let result = match state
        .agent
        .mcp
        .get_prompt(&found.server, &ctx.user_id, &found.prompt.name, arguments)
        .await
    {
        Ok(result) => result,
        Err(Error::Unauthorized(_)) => {
            return Err(refuse(
                StatusCode::FORBIDDEN,
                COMMAND_NEEDS_CREDENTIAL,
                &shown,
            ));
        }
        Err(error) => {
            tracing::warn!(
                server = %found.server.id.0,
                prompt = %found.prompt.name,
                error = %safe_runtime_error(&error),
                "an MCP prompt could not be fetched"
            );

            return Err(refuse(StatusCode::BAD_GATEWAY, COMMAND_FAILED, &shown));
        }
    };

    let text = prompt_text(&result, MAX_PROMPT_BYTES);

    if text.trim().is_empty() {
        return Err(refuse(StatusCode::BAD_GATEWAY, COMMAND_FAILED, &shown));
    }

    if let Some(team_id) = &found.mounts
        && !chat.mounted_team_ids.contains(&team_id.0)
        && let Err(error) = state.agent.chats.mount_team(&chat.id, &team_id.0).await
    {
        return Err(error.into_response());
    }

    Ok(Turn {
        model_text: text.clone(),
        record: Some(CommandRecord {
            name: shown,
            args: invocation.args.clone(),
            source: CommandSource::Mcp,
            model_text: text,
            mcp_server_id: Some(found.server.id.0.clone()),
            mcp_prompt: Some(found.prompt.name.clone()),
            ..CommandRecord::default()
        }),
        ..Turn::default()
    })
}
