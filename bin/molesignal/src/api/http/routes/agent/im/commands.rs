// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! IM 里自己的命令：`/help`、`/new`、`/scope`、`/feedback`。回复的说法见 [`super::texts`]。

use super::{
    inbound::{Place, refusal_text},
    texts::Texts,
    threads::{self, NewScope},
};
use crate::{
    agent::{
        im::{
            ImThread,
            commands::{ImCommand, ScopeTarget, Usage},
        },
        scope::Viewer,
    },
    api::http::routes::agent::feedback::{self, FeedbackRequest},
    app::iam::IamContext,
    shared::{Error, Result},
};

/// 一条 IM 命令的回答。
pub(super) async fn answer(
    place: &Place<'_>,
    ctx: &IamContext,
    viewer: &Viewer,
    thread: Option<&ImThread>,
    command: std::result::Result<ImCommand, Usage>,
    texts: &Texts,
) -> Result<String> {
    let state = place.state;

    match command {
        Ok(ImCommand::Help) => Ok(texts.get("help", &[])),
        Err(Usage::Feedback) => Ok(texts.get("feedback_usage", &[])),
        Ok(ImCommand::New) => {
            let scope = threads::default_scope(state, place.bot, viewer, place.inbound.kind).await;
            threads::open(state, place.bot, ctx, place.inbound, &scope, None).await?;
            Ok(texts.get("new", &[]))
        }
        Ok(ImCommand::Scope(None)) => describe(place, ctx, viewer, thread, texts).await,
        Ok(ImCommand::Scope(Some(target))) => {
            let scope = match target {
                ScopeTarget::Personal => NewScope::Personal,
                ScopeTarget::Team(name) => match threads::member_team(state, ctx, &name).await? {
                    Some(scope) => scope,
                    None => return Ok(texts.get("no_team", &[("team", &name)])),
                },
            };

            threads::open(state, place.bot, ctx, place.inbound, &scope, None).await?;

            Ok(match scope {
                NewScope::Personal => texts.get("scope_moved_personal", &[]),
                NewScope::Team { name, .. } => texts.get("scope_moved_team", &[("team", &name)]),
            })
        }
        Ok(ImCommand::Feedback { helpful, comment }) => {
            rate(place, ctx, viewer, thread, helpful, comment, texts).await
        }
    }
}

/// `/scope`：这个话题现在是谁的会话；还没有会话时说新会话会记在哪里。
async fn describe(
    place: &Place<'_>,
    ctx: &IamContext,
    viewer: &Viewer,
    thread: Option<&ImThread>,
    texts: &Texts,
) -> Result<String> {
    let state = place.state;

    if let Some(thread) = thread {
        match threads::current(state, ctx, viewer, thread).await? {
            Some(Ok(chat)) => {
                return Ok(match (chat.scope.as_str(), chat.team_id.as_deref()) {
                    ("team", Some(team_id)) => {
                        let team = threads::team_name(state, team_id).await;
                        texts.get("scope_team", &[("team", &team)])
                    }
                    _ => texts.get("scope_personal", &[]),
                });
            }
            Some(Err(refusal)) => return Ok(refusal_text(texts, &refusal)),
            None => {}
        }
    }

    Ok(
        match threads::default_scope(state, place.bot, viewer, place.inbound.kind).await {
            NewScope::Team { name, .. } => texts.get("scope_team", &[("team", &name)]),
            NewScope::Personal => texts.get("scope_personal", &[]),
        },
    )
}

/// `/feedback`：评价这个话题里最近的一个回答。
async fn rate(
    place: &Place<'_>,
    ctx: &IamContext,
    viewer: &Viewer,
    thread: Option<&ImThread>,
    helpful: bool,
    comment: String,
    texts: &Texts,
) -> Result<String> {
    let state = place.state;

    let chat = match thread {
        Some(thread) => match threads::current(state, ctx, viewer, thread).await? {
            Some(Ok(chat)) => chat,
            Some(Err(refusal)) => return Ok(refusal_text(texts, &refusal)),
            None => return Ok(texts.get("feedback_none", &[])),
        },
        None => return Ok(texts.get("feedback_none", &[])),
    };

    let Some(answer) = state
        .agent
        .chats
        .list_messages(&chat.id)
        .await?
        .into_iter()
        .rev()
        .find(|message| message.role == "assistant")
    else {
        return Ok(texts.get("feedback_none", &[]));
    };

    let request = FeedbackRequest {
        target_kind: "chat_message".into(),
        target_id: answer.id.0,
        chat_id: Some(chat.id.0),
        rating: if helpful { "up" } else { "down" }.into(),
        reasons: Vec::new(),
        comment,
    };

    match feedback::submit(state, ctx, request).await {
        Ok(_) => Ok(texts.get("feedback_saved", &[])),
        Err(Error::Forbidden(_)) => Ok(texts.get("feedback_not_yours", &[])),
        Err(error) => Err(error),
    }
}
