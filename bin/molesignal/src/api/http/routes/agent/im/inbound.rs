// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 收到一条 IM 消息之后：去重，对上提问人，处理 IM 命令，找到或开一个会话，加上「处理中」的
//! 表情，排一轮回答。回答在 Run 结束时由 [`super::reply`] 发回原处。
//!
//! 对不上人时私下发绑定链接；对上多个人、账号不能用 Mole Agent、接不了这个会话时直接说明，
//! 不排回答。

use super::{
    commands, links,
    texts::Texts,
    threads::{self, Refusal},
};
use crate::{
    agent::{
        im::{ImBot, ImMessage, ImPlatform, Inbound, MessageStatus, Sender},
        runs::RunPrincipal,
    },
    api::{
        AppState,
        http::routes::agent::{
            chat::{ImPost, prepare_im_post, run::identity::resolve_principal},
            viewer::viewer,
        },
    },
    domain::{iam::IamScope, notify::connector::NotifyConnector},
    infra::im,
    shared::{Error, Result, ids::Id},
};

/// 一轮 IM 回答最多跑多久。
pub(super) const DEADLINE_SECS: u64 = 15 * 60;

/// 交给模型的说明：问题来自 IM，没有人能在中途回答或确认。
fn instruction(platform: ImPlatform) -> String {
    format!(
        "This question was asked in {}. Answer for a chat window: lead with the conclusion and \
         keep it short. Nobody can answer questions or confirm changes while you work: make \
         reasonable assumptions and say which, and do not propose changes that need someone to \
         confirm them; say what the person can do in MoleSignal instead.",
        platform.label()
    )
}

/// 一条消息所在的地方：回复都发到这里。
pub(super) struct Place<'a> {
    pub(super) state: &'a AppState,
    pub(super) bot: &'a ImBot,
    pub(super) connector: &'a NotifyConnector,
    pub(super) platform: ImPlatform,
    pub(super) inbound: &'a Inbound,
}

impl Place<'_> {
    /// 回复到消息的原处；发不出去只记日志。
    pub(super) async fn say(&self, text: &str) {
        if let Err(error) = im::reply(
            self.platform,
            &self.connector.config,
            &self.inbound.reply,
            text,
        )
        .await
        {
            tracing::warn!(connector = %self.bot.connector_id.0, %error, "an IM reply was not sent");
        }
    }

    /// 把「处理中」换成 `text`；发不出去只记日志。
    async fn finish(&self, working: Option<&str>, text: &str) {
        if let Err(error) = im::finish(
            self.platform,
            &self.connector.config,
            &self.inbound.reply,
            working,
            text,
        )
        .await
        {
            tracing::warn!(connector = %self.bot.connector_id.0, %error, "an IM reply was not sent");
        }
    }
}

/// 对上提问人的结果。
pub(super) enum Identity {
    User(Id),
    Unknown,
    Ambiguous,
}

/// 只认这个连接器上启用、已验证的通知身份；对上不止一个人时不答。
pub(super) async fn identify(state: &AppState, bot: &ImBot, sender: &Sender) -> Result<Identity> {
    identify_on(state, &bot.org_id, &bot.connector_id, sender).await
}

/// 按连接器上验证过的通知方式认人：发消息、点按钮的那个账号对应哪个 MoleSignal 用户。
pub(super) async fn identify_on(
    state: &AppState,
    org_id: &Id,
    connector_id: &Id,
    sender: &Sender,
) -> Result<Identity> {
    let mut users = state
        .alerting
        .notify
        .list_organization_endpoints(org_id)
        .await?
        .into_iter()
        .filter(|endpoint| {
            endpoint.connector_id == *connector_id
                && endpoint.enabled
                && endpoint.verified
                && sender
                    .ids
                    .iter()
                    .any(|id| id.as_str() == endpoint.external_identity.trim())
        })
        .map(|endpoint| endpoint.user_id)
        .collect::<Vec<_>>();

    users.sort_by(|a, b| a.0.cmp(&b.0));
    users.dedup();

    Ok(match users.as_slice() {
        [] => Identity::Unknown,
        [user] => Identity::User(user.clone()),
        _ => Identity::Ambiguous,
    })
}

pub(super) fn principal(user_id: &Id) -> RunPrincipal {
    RunPrincipal {
        user_id: user_id.clone(),
        scope: IamScope::Organization,
        credential_token_id: None,
        credential_application_id: None,
        credential_service_account_id: None,
    }
}

pub(super) fn refusal_text(texts: &Texts, refusal: &Refusal) -> String {
    match refusal {
        Refusal::NotYours => texts.get("not_yours", &[]),
        Refusal::NotMember(team) => texts.get("not_member", &[("team", team)]),
    }
}

/// 斜杠命令被拒绝时的说法。
fn command_refused(texts: &Texts, code: &str, command: &str) -> String {
    if code == "unknown_command" {
        texts.get("command_unknown", &[("command", command)])
    } else {
        texts.get("command_refused", &[("command", command), ("code", code)])
    }
}

async fn settle(
    state: &AppState,
    message: &mut ImMessage,
    status: MessageStatus,
    detail: &str,
) -> Result<()> {
    message.status = status;
    message.detail = Some(detail.to_string());
    state.agent.im.update_message(message).await
}

/// 机器人收到一条消息。出错只记日志：平台那边已经确认过了。
pub(crate) async fn handle(
    state: &AppState,
    bot: &ImBot,
    connector: &NotifyConnector,
    inbound: Inbound,
) {
    if let Err(error) = process(state, bot, connector, &inbound).await {
        tracing::warn!(connector = %bot.connector_id.0, %error, "an IM message could not be handled");
    }
}

async fn process(
    state: &AppState,
    bot: &ImBot,
    connector: &NotifyConnector,
    inbound: &Inbound,
) -> Result<()> {
    let Some(platform) = ImPlatform::of_connector(&connector.connector_type) else {
        return Ok(());
    };

    let mut message = ImMessage::received(&bot.org_id, &bot.connector_id, inbound);

    // 平台重发的同一条消息只处理一次。
    if !state.agent.im.record_message(&message).await? {
        return Ok(());
    }

    let _ = state
        .agent
        .im
        .touch_bot(&bot.connector_id, message.created_at)
        .await;

    let thread = state
        .agent
        .im
        .thread(
            &bot.connector_id,
            &inbound.conversation_id,
            &inbound.thread_key,
        )
        .await?;

    // 群里没 @ 机器人、也不在它答过的话题里：不是问它的。
    if !inbound.addressed && thread.is_none() {
        return settle(state, &mut message, MessageStatus::Ignored, "not addressed").await;
    }

    let place = Place {
        state,
        bot,
        connector,
        platform,
        inbound,
    };

    let user_id = match identify(state, bot, &inbound.sender).await? {
        Identity::User(user_id) => user_id,
        Identity::Unknown => {
            let texts = Texts::for_user(state, &bot.org_id, None).await;
            links::offer(&place, &texts).await;
            return settle(state, &mut message, MessageStatus::Ignored, "not linked").await;
        }
        Identity::Ambiguous => {
            let texts = Texts::for_user(state, &bot.org_id, None).await;
            place
                .say(&texts.get("ambiguous", &[("platform", &texts.platform(platform))]))
                .await;
            return settle(
                state,
                &mut message,
                MessageStatus::Ignored,
                "linked to several users",
            )
            .await;
        }
    };

    message.user_id = Some(user_id.clone());
    let texts = Texts::for_user(state, &bot.org_id, Some(&user_id)).await;

    let ctx = match resolve_principal(state, &bot.org_id, &principal(&user_id)).await {
        Ok(ctx) if ctx.has_permission("agent.use") => ctx,
        Ok(_) | Err(Error::Forbidden(_) | Error::NotFound(_)) => {
            place.say(&texts.get("no_access", &[])).await;
            return settle(state, &mut message, MessageStatus::Ignored, "no access").await;
        }
        Err(error) => return Err(error),
    };

    let viewer = viewer(state, &ctx).await?;

    if let Some(command) = crate::agent::im::commands::parse(&inbound.text) {
        let answer =
            commands::answer(&place, &ctx, &viewer, thread.as_ref(), command, &texts).await?;
        place.say(&answer).await;
        return settle(state, &mut message, MessageStatus::Ignored, "command").await;
    }

    if inbound.text.is_empty() {
        place.say(&texts.get("help", &[])).await;
        return settle(state, &mut message, MessageStatus::Ignored, "empty").await;
    }

    let current = match &thread {
        Some(thread) => match threads::current(state, &ctx, &viewer, thread).await? {
            Some(Ok(chat)) => Some(chat),
            Some(Err(refusal)) => {
                place.say(&refusal_text(&texts, &refusal)).await;
                return settle(state, &mut message, MessageStatus::Ignored, "not allowed").await;
            }
            None => None,
        },
        None => None,
    };

    let chat = match current {
        Some(chat) => chat,
        None => {
            let scope = threads::default_scope(state, bot, &viewer, inbound.kind).await;
            threads::open(state, bot, &ctx, inbound, &scope, Some(&inbound.text)).await?
        }
    };

    message.chat_id = Some(chat.id.clone());

    let post = prepare_im_post(
        state,
        &ctx,
        &chat.id,
        &inbound.text,
        vec![instruction(platform)],
        DEADLINE_SECS,
    )
    .await?;

    let run = match post {
        ImPost::Ready(run) => run,
        ImPost::Refused { code, command } => {
            place.say(&command_refused(&texts, &code, &command)).await;
            return settle(
                state,
                &mut message,
                MessageStatus::Ignored,
                "command refused",
            )
            .await;
        }
        ImPost::BudgetExhausted => {
            place.say(&texts.get("budget", &[])).await;
            return settle(
                state,
                &mut message,
                MessageStatus::Ignored,
                "budget exhausted",
            )
            .await;
        }
    };

    message.reaction_id = im::react(
        platform,
        &connector.config,
        &inbound.reply,
        &texts.get("working", &[]),
    )
    .await
    .unwrap_or_else(|error| {
        tracing::debug!(%error, "the working sign was not shown");
        None
    });

    // 先记下 Run 再排队：回答得再快，结束时也找得到回复到哪里。
    message.run_id = Some(run.id.clone());
    message.status = MessageStatus::Answering;
    state.agent.im.update_message(&message).await?;

    if let Err(error) = state.agent.runs.create(*run).await {
        place
            .finish(
                message.reaction_id.as_deref(),
                &texts.get("failed", &[("reason", &error.to_string())]),
            )
            .await;
        return settle(
            state,
            &mut message,
            MessageStatus::Failed,
            &error.to_string(),
        )
        .await;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_model_is_told_where_the_question_came_from() {
        let text = instruction(ImPlatform::Feishu);

        assert!(text.contains("asked in Feishu"));
        assert!(text.contains("Nobody can answer questions or confirm changes"));
    }
}
