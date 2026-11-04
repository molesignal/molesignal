// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Teams 卡片：频道 Webhook 的卡片，与 Bot 发的卡片（模板卡片、带操作按钮的告警卡片）。

use serde_json::{Value, json};

use super::MAX_TEXT_CHARS;
use crate::{
    domain::notify::{
        buttons::{AlertAction, AlertButtons},
        connector::NotifyMessage,
    },
    infra::notify::adapters::{
        card::{CardColor, card_color, card_title, severity_color},
        common::{body, truncate_chars},
    },
};

/// 告警卡片上的操作按钮（`Action.Execute`）的 `verb`；按钮的值放在 `data` 的这个键下，点了以后
/// 由它们认出来。
pub(crate) const TEAMS_ALERT_VERB: &str = "molesignal_alert";
pub(crate) const TEAMS_ALERT_VALUE_KEY: &str = "molesignal_alert";

/// 频道 Webhook 收的消息：一张 Adaptive Card，标题加粗，正文按 Markdown 显示。
pub(super) fn adaptive_card(title: &str, text: &str) -> Value {
    let mut blocks = Vec::new();

    if !title.trim().is_empty() {
        blocks.push(json!({
            "type": "TextBlock",
            "text": title.trim(),
            "weight": "Bolder",
            "size": "Medium",
            "wrap": true,
        }));
    }

    blocks.push(json!({
        "type": "TextBlock",
        "text": truncate_chars(text.trim(), MAX_TEXT_CHARS),
        "wrap": true,
    }));

    json!({
        "type": "message",
        "attachments": [{
            "contentType": "application/vnd.microsoft.card.adaptive",
            "contentUrl": null,
            "content": {
                "$schema": "http://adaptivecards.io/schemas/adaptive-card.json",
                "type": "AdaptiveCard",
                "version": "1.4",
                "body": blocks,
            },
        }],
    })
}

/// 一个操作按钮：`Action.Execute`，点了 Teams 把 `adaptiveCard/action` 发给 Bot；只认得
/// `Action.Submit` 的旧客户端退回它，带着同样的值。
fn execute(label: &str, value: Value, primary: bool) -> Value {
    let data = json!({ TEAMS_ALERT_VALUE_KEY: value });

    json!({
        "type": "Action.Execute",
        "title": label,
        "verb": TEAMS_ALERT_VERB,
        "data": data,
        "style": if primary { "positive" } else { "default" },
        "fallback": { "type": "Action.Submit", "title": label, "data": data },
    })
}

/// 卡片本身（Adaptive Card）。标题放在撑满卡片两边、按颜色上色的容器里，成一条色带；正文按 Markdown
/// 显示，字段是一组事实，`note` 是一句谁做了什么，然后是按钮（告警的操作与链接），最后是分隔线下模板卡片
/// 的页脚。没有模板卡片时标题用通知的标题、按严重度上色。
pub(crate) fn teams_card(
    message: &NotifyMessage,
    buttons: &AlertButtons,
    note: Option<&str>,
) -> Value {
    let card = message.card.as_ref();

    let (title, color) = match card {
        Some(card) => (card_title(card, message), card_color(card, message)),
        None => (message.title.as_str(), severity_color(message)),
    };

    // 容器的样式给色带上色，标题的字与它同色；灰色是强调样式配默认的字色。
    let (style, ink) = match color {
        CardColor::Red => ("attention", "Attention"),
        CardColor::Orange | CardColor::Yellow => ("warning", "Warning"),
        CardColor::Green => ("good", "Good"),
        CardColor::Blue => ("accent", "Accent"),
        CardColor::Grey => ("emphasis", "Default"),
    };

    let mut blocks = vec![
        json!({
            "type": "Container",
            "style": style,
            "bleed": true,
            "items": [{
                "type": "TextBlock",
                "text": title,
                "weight": "Bolder",
                "size": "Medium",
                "color": ink,
                "wrap": true,
            }],
        }),
        json!({
            "type": "TextBlock",
            "text": truncate_chars(body(message).trim(), MAX_TEXT_CHARS),
            "wrap": true,
        }),
    ];

    if let Some(card) = card
        && !card.fields.is_empty()
    {
        let facts = card
            .fields
            .iter()
            .map(|field| json!({ "title": field.label, "value": field.value }))
            .collect::<Vec<_>>();

        blocks.push(json!({ "type": "FactSet", "facts": facts }));
    }

    if let Some(note) = note {
        blocks.push(json!({
            "type": "TextBlock",
            "text": note,
            "isSubtle": true,
            "size": "Small",
            "wrap": true,
        }));
    }

    let mut actions = buttons
        .actions
        .iter()
        .map(|(action, label)| {
            let primary = matches!(
                action,
                AlertAction::Acknowledge | AlertAction::AcknowledgeAll
            );
            execute(label, json!(buttons.value(*action)), primary)
        })
        .collect::<Vec<_>>();

    let links = buttons
        .open
        .iter()
        .map(|(label, url)| (label.as_str(), url.as_str()))
        .chain(
            card.into_iter()
                .flat_map(|card| &card.links)
                .map(|link| (link.label.as_str(), link.url.as_str())),
        );

    for (label, url) in links {
        actions.push(json!({ "type": "Action.OpenUrl", "title": label, "url": url }));
    }

    // 操作按钮放进正文里的 ActionSet：旧客户端只在这里认 `fallback`。只有链接时放在卡片底部。
    let operated = !buttons.actions.is_empty();

    if operated {
        blocks.push(json!({ "type": "ActionSet", "actions": actions }));
    }

    if let Some(card) = card
        && !card.note.is_empty()
    {
        blocks.push(json!({
            "type": "TextBlock",
            "text": card.note,
            "isSubtle": true,
            "size": "Small",
            "separator": true,
            "spacing": "Medium",
            "wrap": true,
        }));
    }

    let mut content = json!({
        "$schema": "http://adaptivecards.io/schemas/adaptive-card.json",
        "type": "AdaptiveCard",
        "version": "1.4",
        "body": blocks,
    });

    if !operated {
        content["actions"] = json!(actions);
    }

    content
}

/// 带着 [`teams_card`] 的一条消息。
pub(crate) fn card_message(
    message: &NotifyMessage,
    buttons: &AlertButtons,
    note: Option<&str>,
) -> Value {
    let summary = match &message.card {
        Some(card) => card_title(card, message),
        None => message.title.as_str(),
    };

    json!({
        "type": "message",
        "summary": summary,
        "attachments": [{
            "contentType": "application/vnd.microsoft.card.adaptive",
            "content": teams_card(message, buttons, note),
        }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::notify::{
        buttons::{AlertButtonValue, AlertButtons},
        template::{NotifyCard, NotifyCardColor, NotifyCardField, NotifyCardLink},
    };

    fn message(card: Option<NotifyCard>) -> NotifyMessage {
        NotifyMessage {
            title: "CRITICAL · Disk full".into(),
            text: "Disk is at **97%**.".into(),
            markdown: None,
            html: None,
            metadata: Default::default(),
            provider_templates: Default::default(),
            event: None,
            card,
        }
    }

    #[test]
    fn a_webhook_message_is_an_adaptive_card() {
        let card = adaptive_card("Checkout 5xx", "Rate is **8%**.");
        let content = &card["attachments"][0]["content"];

        assert_eq!(card["type"], "message");
        assert_eq!(content["type"], "AdaptiveCard");
        assert_eq!(content["body"][0]["text"], "Checkout 5xx");
        assert_eq!(content["body"][1]["text"], "Rate is **8%**.");
    }

    #[test]
    fn a_card_is_an_adaptive_card_with_facts_and_links() {
        let card = NotifyCard {
            title: "Disk full on db-1".into(),
            color: NotifyCardColor::Green,
            fields: vec![NotifyCardField {
                label: "Host".into(),
                value: "db-1".into(),
            }],
            links: vec![NotifyCardLink {
                label: "Runbook".into(),
                url: "https://runbooks.example.com/disk".into(),
            }],
            note: "MoleSignal".into(),
        };

        let activity = card_message(&message(Some(card)), &AlertButtons::default(), None);
        let content = &activity["attachments"][0]["content"];

        assert_eq!(activity["summary"], "Disk full on db-1");

        // 标题在撑满两边、按颜色上色的容器里。
        let header = &content["body"][0];
        assert_eq!(header["type"], "Container");
        assert_eq!(header["style"], "good");
        assert_eq!(header["bleed"], true);
        assert_eq!(header["items"][0]["text"], "Disk full on db-1");
        assert_eq!(header["items"][0]["color"], "Good");

        assert_eq!(content["body"][1]["text"], "Disk is at **97%**.");
        assert_eq!(content["body"][2]["facts"][0]["title"], "Host");
        assert_eq!(content["body"][3]["text"], "MoleSignal");
        assert_eq!(content["body"][3]["separator"], true);
        assert_eq!(
            content["actions"][0]["url"],
            "https://runbooks.example.com/disk"
        );
    }

    #[test]
    fn an_alert_card_carries_its_actions_in_an_action_set() {
        let buttons = AlertButtons {
            alert_id: "inc-1".into(),
            event_type: "alert.triggered".into(),
            policy_id: Some("policy-1".into()),
            alert_ids: Vec::new(),
            actions: vec![
                (AlertAction::Acknowledge, "Acknowledge".into()),
                (AlertAction::Resolve, "Resolve".into()),
            ],
            open: Some((
                "Open in MoleSignal".into(),
                "https://molesignal.example.com".into(),
            )),
        };

        let content = teams_card(&message(None), &buttons, Some("Acknowledged by Ada"));
        let blocks = content["body"].as_array().unwrap();

        // 没有模板卡片：通知的标题，严重度的颜色。
        assert_eq!(blocks[0]["items"][0]["text"], "CRITICAL · Disk full");
        assert_eq!(blocks[2]["text"], "Acknowledged by Ada");

        let actions = blocks[3]["actions"].as_array().unwrap();
        assert_eq!(blocks[3]["type"], "ActionSet");
        assert!(content.get("actions").is_none());

        let acknowledge = &actions[0];
        assert_eq!(acknowledge["type"], "Action.Execute");
        assert_eq!(acknowledge["verb"], TEAMS_ALERT_VERB);
        assert_eq!(acknowledge["style"], "positive");
        assert_eq!(acknowledge["fallback"]["type"], "Action.Submit");
        assert_eq!(acknowledge["fallback"]["data"], acknowledge["data"]);

        let value: AlertButtonValue =
            serde_json::from_value(acknowledge["data"][TEAMS_ALERT_VALUE_KEY].clone()).unwrap();
        assert_eq!(value.action, AlertAction::Acknowledge);
        assert_eq!(value.alert_id, "inc-1");
        assert_eq!(value.policy_id.as_deref(), Some("policy-1"));

        assert_eq!(actions[1]["style"], "default");
        assert_eq!(actions[2]["type"], "Action.OpenUrl");
        assert_eq!(actions[2]["url"], "https://molesignal.example.com");
    }
}
