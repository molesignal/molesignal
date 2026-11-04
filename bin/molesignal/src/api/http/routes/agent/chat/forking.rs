// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 分叉会话：从一条回答起，把会话复制成一个新会话，归分叉的人。

use axum::{
    Extension, Json,
    extract::{Path, State},
};
use serde::Deserialize;

use super::{
    super::viewer::readable_chat,
    messages::{ChatResp, to_chat_resp},
};
use crate::{
    agent::scope::Scope,
    api::AppState,
    app::iam::IamContext,
    domain::iam::permission,
    infra::persistence::repositories::agent::chats::Chat,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForkReq {
    /// 从这条消息分叉：新会话的消息到它为止。不填时复制全部。
    #[serde(default)]
    pub message_id: Option<String>,
}

/// `POST /api/v1/agent/chat/{id}/fork`：从一条回答把会话分叉成一个新会话，归分叉的人。复制到那
/// 条为止的消息，标题、范围与团队、挂进来的团队、模型、时间范围与回答语言照旧；调查的对象不带
/// 过去，新会话是另一条线。只在复制全部时带上压缩摘要（它可能概括了那条之后的消息）。团队会话的
/// 分叉仍在团队里，要能在团队里发起会话。
#[permission("agent.use")]
pub(in super::super) async fn fork_chat(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    body: Option<Json<ForkReq>>,
) -> Result<Json<ChatResp>> {
    let request = body.map(|Json(request)| request).unwrap_or_default();
    let (source, viewer) = readable_chat(&state, &ctx, &Id(id)).await?;

    let upto = match request.message_id.as_deref() {
        Some(message_id) => Some(
            state
                .agent
                .chats
                .list_messages(&source.id)
                .await?
                .into_iter()
                .find(|message| message.id.0 == message_id)
                .ok_or_else(|| Error::not_found("the message to fork from is not in this chat"))?,
        ),
        None => None,
    };

    if let Some(team_id) = &source.team_id
        && !viewer.can_edit(&Scope::Team {
            team_id: Id(team_id.clone()),
        })
    {
        return Err(Error::forbidden(
            "only the team's members can fork the team's chats",
        ));
    }

    let mut fork = Chat::minimal(
        Id::new(),
        ctx.org_id.clone(),
        ctx.user_id.clone(),
        source.provider.clone(),
        source.model.clone(),
        source.title.clone(),
        TimestampMicros::now(),
    );
    fork.provider_id = source.provider_id;
    fork.analysis_mode = source.analysis_mode;
    fork.time_range_start = source.time_range_start;
    fork.time_range_end = source.time_range_end;
    // 组织会话只用于自动化的运行：分叉出来的是本人的会话，可以接着问。
    if source.scope == "org" {
        fork.scope = "personal".into();
    } else {
        fork.scope = source.scope;
        fork.team_id = source.team_id;
    }
    fork.mounted_team_ids = source.mounted_team_ids;
    fork.reply_language = source.reply_language;
    if upto.is_none() {
        fork.context_summary = source.context_summary;
        fork.context_summary_upto = source.context_summary_upto;
    }
    fork.parent_chat_id = Some(source.id.0);
    fork.title_provisional = source.title_provisional;

    let saved = state.agent.chats.fork_chat(fork, upto.as_ref()).await?;
    Ok(Json(to_chat_resp(saved, &viewer)))
}
