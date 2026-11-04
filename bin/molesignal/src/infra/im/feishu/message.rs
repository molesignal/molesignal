// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 「接收消息」事件里的用户消息：文本与富文本，去掉对机器人的 @。「卡片回传交互」里点的告警
//! 按钮。

use serde_json::{Value, json};

use crate::{
    agent::im::{CardAction, ConversationKind, Inbound, Sender},
    domain::notify::buttons::AlertButtonValue,
    infra::notify::adapters::LARK_ALERT_VALUE_KEY,
};

/// 飞书的身份：按 open_id、user_id、union_id 的顺序，去掉空的。
fn identities(id: &Value) -> Vec<String> {
    ["open_id", "user_id", "union_id"]
        .iter()
        .filter_map(|key| id[*key].as_str())
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .collect()
}

/// 「卡片回传交互」里点的告警按钮。回调不说卡片在不在私聊里，当作群：绑定链接私下发。
pub(crate) fn card_action(event: &Value) -> Option<CardAction> {
    let value: AlertButtonValue =
        serde_json::from_value(event["action"]["value"][LARK_ALERT_VALUE_KEY].clone()).ok()?;

    let ids = identities(&event["operator"]);

    if ids.is_empty() {
        return None;
    }

    let message_id = event["context"]["open_message_id"]
        .as_str()
        .filter(|id| !id.is_empty())?
        .to_string();

    Some(CardAction {
        value,
        sender: Sender { ids, name: None },
        conversation_id: event["context"]["open_chat_id"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        kind: ConversationKind::Group,
        reply: json!({ "message_id": message_id, "in_thread": false }),
        card: json!({ "message_id": message_id }),
    })
}

/// 「接收消息」事件里的一条用户消息。只认文本与富文本。
pub(crate) fn inbound(event: &Value, bot_open_id: &str) -> Option<Inbound> {
    let message = &event["message"];
    let sender = &event["sender"];

    if sender["sender_type"].as_str() != Some("user") {
        return None;
    }

    let message_id = message["message_id"].as_str()?.to_string();
    let chat_id = message["chat_id"].as_str()?.to_string();
    let direct = message["chat_type"].as_str() == Some("p2p");
    let content: Value = serde_json::from_str(message["content"].as_str()?).ok()?;

    let raw = match message["message_type"].as_str()? {
        "text" => content["text"].as_str()?.to_string(),
        "post" => post_text(&content),
        _ => return None,
    };

    let mentions = message["mentions"].as_array().cloned().unwrap_or_default();
    let mut addressed = direct;
    let mut text = raw;

    for mention in &mentions {
        let key = mention["key"].as_str().unwrap_or_default();

        if key.is_empty() {
            continue;
        }

        if !bot_open_id.is_empty() && mention["id"]["open_id"].as_str() == Some(bot_open_id) {
            addressed = true;
            text = text.replace(key, "");
        } else {
            let name = mention["name"].as_str().unwrap_or_default();
            text = text.replace(key, &format!("@{name}"));
        }
    }

    let root = message["root_id"].as_str().filter(|root| !root.is_empty());
    let thread_key = if direct {
        String::new()
    } else {
        root.unwrap_or(&message_id).to_string()
    };

    let ids = identities(&sender["sender_id"]);

    if ids.is_empty() {
        return None;
    }

    Some(Inbound {
        message_id: message_id.clone(),
        conversation_id: chat_id,
        kind: if direct {
            ConversationKind::Direct
        } else {
            ConversationKind::Group
        },
        thread_key,
        sender: Sender { ids, name: None },
        text: text.trim().to_string(),
        addressed,
        reply: json!({ "message_id": message_id, "in_thread": !direct }),
    })
}

/// 富文本里的文字：每段的文本与链接，段间换行；@ 写成占位，与 `mentions` 对得上。
pub(super) fn post_text(content: &Value) -> String {
    let body = if content["content"].is_array() {
        content
    } else {
        content
            .as_object()
            .and_then(|languages| languages.values().next())
            .unwrap_or(content)
    };

    let mut lines = Vec::new();

    if let Some(title) = body["title"].as_str().filter(|title| !title.is_empty()) {
        lines.push(title.to_string());
    }

    for paragraph in body["content"].as_array().into_iter().flatten() {
        let line = paragraph
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|element| match element["tag"].as_str()? {
                "text" | "a" | "md" => element["text"].as_str().map(str::to_string),
                "at" => element["user_id"].as_str().map(str::to_string),
                "code_block" => element["text"].as_str().map(|code| format!("\n{code}\n")),
                _ => None,
            })
            .collect::<String>();

        lines.push(line);
    }

    lines.join("\n")
}
