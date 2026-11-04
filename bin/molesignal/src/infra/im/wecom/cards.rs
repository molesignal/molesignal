// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 企业微信智能机器人发的告警卡片。
//!
//! 智能机器人也发告警通知（`aibot_send_msg`，带按钮时是模板卡片）：本节点连着机器人时直接发，不然交给
//! 连着它的节点，记进待发的回复里（[`deliver`]），由那个节点发出（[`post_notification`]）。机器人只能
//! 发到有人给它发过消息的会话：私聊过它的人，有人 @ 过它的群。
//!
//! 点了卡片上的按钮，企业微信经长连接推来 `aibot_event_callback`（`template_card_event`，按钮的 `key`
//! 是按钮的值，[`card_action`]）。处理完 5 秒内用那一帧的 `req_id` 换掉卡片
//! （`aibot_respond_update_msg`，[`answer`]），没做成的那句话单聊告诉点的人。

use serde_json::{Value, json};

use super::{frame, holds, send, tell};
use crate::{
    agent::im::{
        BotStatus, CardAction, CardAnswer, ConversationKind, ImOutgoing, ImRepository, NoticeKind,
        Sender,
    },
    domain::notify::{buttons::AlertButtonValue, connector::NotifyConnector},
    infra::notify::adapters::wecom_template_card,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 点了机器人发的模板卡片上的一个按钮。不是告警按钮（处理完的「✓」）时不算。
pub(crate) fn card_action(frame: &Value) -> Option<CardAction> {
    let body = &frame["body"];
    let event = &body["event"];

    if frame["cmd"] != "aibot_event_callback" || event["eventtype"] != "template_card_event" {
        return None;
    }

    // 按钮的 `key` 与卡片的 `task_id` 在 `template_card_event` 里；也认直接放在 `event` 里的。
    let card = event.get("template_card_event").unwrap_or(event);
    let value: AlertButtonValue = serde_json::from_str(card["event_key"].as_str()?).ok()?;
    let req_id = frame["headers"]["req_id"].as_str()?;
    let task_id = card["task_id"].as_str()?;
    let user = body["from"]["userid"]
        .as_str()
        .filter(|user| !user.is_empty())?;
    let group = body["chattype"].as_str() == Some("group");
    let chat = if group {
        body["chatid"].as_str()?
    } else {
        user
    };

    Some(CardAction {
        value,
        sender: Sender {
            ids: vec![user.to_string()],
            name: None,
        },
        conversation_id: chat.to_string(),
        kind: if group {
            ConversationKind::Group
        } else {
            ConversationKind::Direct
        },
        reply: json!({
            "req_id": req_id,
            "chat_id": chat,
            "chat_type": if group { 2 } else { 1 },
        }),
        card: json!({ "req_id": req_id, "task_id": task_id }),
    })
}

/// 用点击那一帧的 `req_id` 换掉卡片，会话里每个人都看得到；那句话单聊告诉点的人（做成了的不用说，
/// 卡片上写着）。要在点击后 5 秒内。
pub(in crate::infra::im) async fn answer(
    config: &Value,
    action: &CardAction,
    answer: &CardAnswer,
) -> Result<()> {
    if let Some(card) = &answer.card {
        let (Some(req_id), Some(task_id)) = (
            action.card["req_id"].as_str(),
            action.card["task_id"].as_str(),
        ) else {
            return Err(Error::invalid("the WeCom card to update is unknown"));
        };

        let body = json!({
            "response_type": "update_template_card",
            "template_card": wecom_template_card(
                &card.message,
                &card.buttons,
                card.note.as_deref(),
                task_id,
            ),
        });

        send(config, frame("aibot_respond_update_msg", req_id, body)).await?;
    }

    if let Some((kind, text)) = &answer.notice
        && (answer.card.is_none() || *kind != NoticeKind::Success)
        && let Some(user) = action.sender.ids.first()
    {
        tell(config, user, text).await?;
    }

    Ok(())
}

/// 机器人主动发的一条通知：`body` 是 `aibot_send_msg` 的消息体，带着 `chatid` 与 `chat_type`。
pub(crate) async fn post_notification(config: &Value, body: &Value) -> Result<()> {
    send(config, frame("aibot_send_msg", &Id::new().0, body.clone())).await
}

/// 机器人发一条通知到 `chat`（`kind` 1 是单聊的 userid，2 是群）：本节点连着它就直接发，不然记进待发的
/// 回复，由连着它的节点发出。哪个节点都没连着时发不出去。
pub(crate) async fn deliver(
    connector: &NotifyConnector,
    chat: &str,
    kind: i64,
    mut body: Value,
    outbox: Option<&dyn ImRepository>,
) -> Result<()> {
    body["chatid"] = json!(chat);
    body["chat_type"] = json!(kind);

    if holds(&connector.config) {
        return post_notification(&connector.config, &body).await;
    }

    let not_connected = || Error::unavailable("the WeCom smart bot is not connected");
    let outbox = outbox.ok_or_else(not_connected)?;

    let connected = outbox
        .get_bot(&connector.organization_id, &connector.id)
        .await?
        .is_some_and(|bot| bot.enabled && bot.status == BotStatus::Connected);

    if !connected {
        return Err(not_connected());
    }

    outbox
        .enqueue_outgoing(&ImOutgoing {
            id: Id::new(),
            org_id: connector.organization_id.clone(),
            connector_id: connector.id.clone(),
            message_id: None,
            reply: json!({ "chat_id": chat, "chat_type": kind }),
            working: None,
            markdown: String::new(),
            notification: Some(body),
            created_at: TimestampMicros::now(),
        })
        .await
}
