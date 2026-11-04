// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 在 IM 里：飞书长连接、Slack Socket Mode、钉钉 Stream、企业微信智能机器人的长连接
//! 与 Google Chat 的 Pub/Sub 订阅收消息；Microsoft Teams 的消息 POST 到公网回调（见 [`teams`]）。
//! 各平台的接口回复。
//!
//! 一次 [`listen`] 是一条长连接（Google Chat 是一轮轮的拉取，Teams 只确认凭据）：连上、确认收到
//! 的每条消息并交给 [`Listener`]，直到平台要求重连（`Ok`）或出错。回复、「处理中」与私下告诉
//! 发消息的人大多走平台的 HTTP 接口，执行 Run 的节点直接调；企业微信的都走长连接，只有连着的
//! 节点发得出去（[`holds`]）。
//!
//! 告警卡片上按钮的点击由长连接交出（[`Listener::action`]）：飞书在确认里带回结果，Slack 处理完用
//! `response_url` 换掉卡片，Google Chat 改掉卡片所在的消息，钉钉按卡片的 ID 换上新的变量，企业微信
//! 智能机器人用点击那一帧的 `req_id` 换掉模板卡片（[`answer`]）。Teams 的点击和消息一样 POST 到回调，
//! 由回调的路由处理。

pub(crate) mod dingtalk;
pub(crate) mod feishu;
pub(crate) mod google_chat;
pub(crate) mod slack;
pub(crate) mod teams;
pub(crate) mod wecom;
mod ws;

use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::{
    agent::im::{CardAction, CardAnswer, ImPlatform, Inbound},
    infra::notify::adapters::DingTalkApp,
    shared::Result,
};

/// 在确认里带回点击结果的平台最多等这么久（飞书等 3 秒）。
pub(crate) const ANSWER_WITHIN: Duration = Duration::from_millis(2_500);

/// 长连接交出来的东西。
#[async_trait]
pub(crate) trait Listener: Send + Sync {
    /// 连上了；`detail` 说连上了谁。
    async fn connected(&self, detail: &str);
    /// 收到一条消息。要尽快返回：平台在等确认。
    async fn message(&self, inbound: Inbound);
    /// 点了告警卡片上的按钮。在确认里带回结果的平台（[`answers_in_acknowledgement`]）最多等
    /// [`ANSWER_WITHIN`]，返回结果；其余的、来不及的，处理完自己换上（[`answer`]）。
    async fn action(&self, action: CardAction) -> Option<CardAnswer> {
        let _ = action;
        None
    }
}

/// 一条长连接，直到断开或 `cancel`。
pub(crate) async fn listen(
    platform: ImPlatform,
    config: &Value,
    listener: &dyn Listener,
    cancel: &CancellationToken,
) -> Result<()> {
    match platform {
        ImPlatform::Feishu => feishu::listen(config, listener, cancel).await,
        ImPlatform::Slack => slack::listen(config, listener, cancel).await,
        ImPlatform::DingTalk => dingtalk::listen(config, listener, cancel).await,
        ImPlatform::WeCom => wecom::listen(config, listener, cancel).await,
        ImPlatform::GoogleChat => google_chat::listen(config, listener, cancel).await,
        ImPlatform::Teams => teams::listen(config, listener, cancel).await,
    }
}

/// 告警通知在这个平台上能带按钮：长连接或回调收得到点击。钉钉要连接器里填了互动卡片模板。
pub(crate) fn takes_card_actions(platform: ImPlatform, config: &Value) -> bool {
    match platform {
        ImPlatform::Feishu
        | ImPlatform::Slack
        | ImPlatform::Teams
        | ImPlatform::GoogleChat
        | ImPlatform::WeCom => true,
        ImPlatform::DingTalk => {
            DingTalkApp::parse(config).is_ok_and(|app| app.card_template_id().is_some())
        }
    }
}

/// 点击的结果在长连接的确认里带回去（飞书）。
pub(crate) fn answers_in_acknowledgement(platform: ImPlatform) -> bool {
    platform == ImPlatform::Feishu
}

/// 没在确认里带回去的点击结果：换上卡片，那句话只告诉点的人。
pub(crate) async fn answer(
    platform: ImPlatform,
    config: &Value,
    action: &CardAction,
    answer: &CardAnswer,
) -> Result<()> {
    match platform {
        ImPlatform::Feishu => feishu::answer(config, action, answer).await,
        ImPlatform::Slack => slack::answer(config, action, answer).await,
        ImPlatform::Teams => teams::answer(config, action, answer).await,
        ImPlatform::GoogleChat => google_chat::answer(config, action, answer).await,
        ImPlatform::DingTalk => dingtalk::answer(config, action, answer).await,
        ImPlatform::WeCom => wecom::answer(config, action, answer).await,
    }
}

/// 回复只能走机器人的长连接（企业微信）：只有连着的节点发得出去。
pub(crate) fn replies_over_connection(platform: ImPlatform) -> bool {
    platform == ImPlatform::WeCom
}

/// 本节点能不能发这个机器人的回复。
pub(crate) fn holds(platform: ImPlatform, config: &Value) -> bool {
    match platform {
        ImPlatform::WeCom => wecom::holds(config),
        ImPlatform::Feishu
        | ImPlatform::Slack
        | ImPlatform::DingTalk
        | ImPlatform::GoogleChat
        | ImPlatform::Teams => true,
    }
}

/// 回复到消息的原处：话题里、私聊里。
pub(crate) async fn reply(
    platform: ImPlatform,
    config: &Value,
    reply: &Value,
    markdown: &str,
) -> Result<()> {
    match platform {
        ImPlatform::Feishu => feishu::reply(config, reply, markdown).await,
        ImPlatform::Slack => slack::reply(config, reply, markdown).await,
        ImPlatform::DingTalk => dingtalk::reply(config, reply, markdown).await,
        ImPlatform::WeCom => wecom::reply(config, reply, markdown).await,
        ImPlatform::GoogleChat => google_chat::reply(config, reply, markdown).await,
        ImPlatform::Teams => teams::reply(config, reply, markdown).await,
    }
}

/// 表示「处理中」，返回换掉它要的 ID：飞书与 Slack 在消息上加表情，企业微信发一条写着
/// `working` 的流式消息，Google Chat 与 Teams 回复一条。钉钉什么都不加，返回 `None`。
pub(crate) async fn react(
    platform: ImPlatform,
    config: &Value,
    reply: &Value,
    working: &str,
) -> Result<Option<String>> {
    match platform {
        ImPlatform::Feishu => feishu::react(config, reply).await.map(Some),
        ImPlatform::Slack => slack::react(config, reply).await.map(Some),
        ImPlatform::DingTalk => Ok(None),
        ImPlatform::WeCom => wecom::react(config, reply, working).await.map(Some),
        ImPlatform::GoogleChat => google_chat::react(config, reply, working).await.map(Some),
        ImPlatform::Teams => teams::react(config, reply, working).await.map(Some),
    }
}

/// 回答好了：撤掉「处理中」的表情再回复，或把「处理中」的消息原地换成回答。
pub(crate) async fn finish(
    platform: ImPlatform,
    config: &Value,
    reply: &Value,
    working: Option<&str>,
    markdown: &str,
) -> Result<()> {
    let unreacted = match (platform, working) {
        (ImPlatform::Feishu, Some(reaction)) => feishu::unreact(config, reply, reaction).await,
        (ImPlatform::Slack, Some(reaction)) => slack::unreact(config, reply, reaction).await,
        (ImPlatform::WeCom, _) => return wecom::finish(config, reply, working, markdown).await,
        (ImPlatform::GoogleChat, _) => {
            return google_chat::finish(config, reply, working, markdown).await;
        }
        (ImPlatform::Teams, _) => return teams::finish(config, reply, working, markdown).await,
        _ => Ok(()),
    };

    if let Err(error) = unreacted {
        tracing::debug!(%error, "the working reaction was not removed");
    }

    self::reply(platform, config, reply, markdown).await
}

/// 只让发消息的人看到：私下发给他（Slack 是只有他看得到的消息）。企业微信要他先私聊过机器人。
pub(crate) async fn tell_sender(
    platform: ImPlatform,
    config: &Value,
    reply: &Value,
    sender_id: &str,
    markdown: &str,
) -> Result<()> {
    match platform {
        ImPlatform::Feishu => feishu::tell(config, sender_id, markdown).await,
        ImPlatform::Slack => slack::tell(config, reply, sender_id, markdown).await,
        ImPlatform::DingTalk => dingtalk::tell(config, sender_id, markdown).await,
        ImPlatform::WeCom => wecom::tell(config, sender_id, markdown).await,
        ImPlatform::GoogleChat => google_chat::tell(config, sender_id, markdown).await,
        ImPlatform::Teams => teams::tell(config, reply, sender_id, markdown).await,
    }
}
