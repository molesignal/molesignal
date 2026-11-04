// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 飞书卡片：App 发的告警卡片（带操作按钮）与群机器人发的卡片或文本。

use serde_json::{Value, json};

use crate::{
    domain::notify::{
        buttons::{AlertAction, AlertButtons},
        connector::NotifyMessage,
    },
    infra::notify::adapters::{
        card::{CardColor, card_color, card_title, severity_color},
        common::truncate_chars,
    },
};

/// 告警卡片上操作按钮的值放在这个键下，点了以后由它认出来。
pub(crate) const LARK_ALERT_VALUE_KEY: &str = "molesignal_alert";
/// 告警卡片的正文最多这么多字符（卡片最大 30 KB）。
const MAX_ALERT_CARD_CHARS: usize = 8_000;

/// 卡片（卡片 JSON 1.0）：标题栏按颜色上色，正文是通知的 Markdown，再是模板卡片的字段，`note` 是一句谁做了
/// 什么，然后是按钮（告警的操作与模板卡片的链接），最后是分隔线下模板卡片的页脚。没有模板卡片时标题用
/// 通知的标题、按严重度上色。共享卡片（`update_multi`）：点了以后换上的卡片群里每个人都看得到。
pub(crate) fn lark_card(
    message: &NotifyMessage,
    buttons: &AlertButtons,
    note: Option<&str>,
) -> Value {
    let card = message.card.as_ref();

    let body = message
        .markdown
        .as_deref()
        .filter(|markdown| !markdown.trim().is_empty())
        .unwrap_or(&message.text);

    let mut elements = vec![json!({
        "tag": "markdown",
        "content": truncate_chars(body, MAX_ALERT_CARD_CHARS),
    })];

    if let Some(card) = card
        && !card.fields.is_empty()
    {
        let fields = card
            .fields
            .iter()
            .map(|field| {
                json!({
                    "is_short": true,
                    "text": {
                        "tag": "lark_md",
                        "content": format!("**{}**\n{}", field.label, field.value),
                    },
                })
            })
            .collect::<Vec<_>>();

        elements.push(json!({ "tag": "div", "fields": fields }));
    }

    if let Some(note) = note {
        elements.push(json!({
            "tag": "note",
            "elements": [{ "tag": "plain_text", "content": note }],
        }));
    }

    let mut actions = buttons
        .actions
        .iter()
        .map(|(action, label)| {
            json!({
                "tag": "button",
                "text": { "tag": "plain_text", "content": label },
                "type": if matches!(action, AlertAction::Acknowledge | AlertAction::AcknowledgeAll) { "primary" } else { "default" },
                "value": { LARK_ALERT_VALUE_KEY: buttons.value(*action) },
            })
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
        actions.push(json!({
            "tag": "button",
            "text": { "tag": "plain_text", "content": label },
            "type": "default",
            "url": url,
        }));
    }

    if !actions.is_empty() {
        elements.push(json!({ "tag": "action", "actions": actions }));
    }

    // 页脚在一条分隔线下面。
    if let Some(card) = card
        && !card.note.is_empty()
    {
        elements.push(json!({ "tag": "hr" }));

        elements.push(json!({
            "tag": "note",
            "elements": [{ "tag": "plain_text", "content": card.note }],
        }));
    }

    let (title, color) = match card {
        Some(card) => (card_title(card, message), card_color(card, message)),
        None => (message.title.as_str(), severity_color(message)),
    };

    json!({
        "config": { "wide_screen_mode": true, "update_multi": true },
        "header": {
            "title": { "tag": "plain_text", "content": title },
            "template": header_color(color),
        },
        "elements": elements,
    })
}

/// 群机器人发的消息：即时通讯模板有卡片时是卡片（群机器人收不到点击，没有告警的操作按钮），其余是文本。
pub(super) fn webhook_body(message: &NotifyMessage) -> Value {
    if message.card.is_some() {
        return json!({
            "msg_type": "interactive",
            "card": lark_card(message, &AlertButtons::default(), None),
        });
    }

    let text = format!("{}\n{}", message.title, message.text)
        .trim()
        .to_string();

    json!({
        "msg_type": "text",
        "content": { "text": text },
    })
}

/// 飞书卡片标题的颜色名。
fn header_color(color: CardColor) -> &'static str {
    match color {
        CardColor::Blue => "blue",
        CardColor::Green => "green",
        CardColor::Yellow => "yellow",
        CardColor::Orange => "orange",
        CardColor::Red => "red",
        CardColor::Grey => "grey",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::domain::notify::template::{
        NotifyCard, NotifyCardColor, NotifyCardField, NotifyCardLink,
    };

    #[test]
    fn an_alert_card_carries_its_buttons_and_says_who_acted() {
        let message = NotifyMessage {
            title: "CRITICAL · API unavailable".into(),
            text: "Alert API unavailable is acknowledged.".into(),
            markdown: Some("**API unavailable**".into()),
            html: None,
            metadata: BTreeMap::from([
                ("severity".to_string(), "critical".to_string()),
                ("status".to_string(), "acknowledged".to_string()),
            ]),
            provider_templates: Default::default(),
            event: None,
            card: None,
        };
        let buttons = AlertButtons {
            alert_id: "incident-1".into(),
            event_type: "alert.triggered".into(),
            policy_id: Some("policy-1".into()),
            alert_ids: Vec::new(),
            actions: vec![(AlertAction::Resolve, "解决".into())],
            open: Some((
                "在 MoleSignal 中查看".into(),
                "https://example/alerts/incidents/incident-1".into(),
            )),
        };

        let card = lark_card(&message, &buttons, Some("Alice 确认了这个告警。"));

        assert_eq!(card["config"]["update_multi"], true);
        assert_eq!(card["header"]["template"], "red");
        assert_eq!(
            card["header"]["title"]["content"],
            "CRITICAL · API unavailable"
        );
        assert_eq!(card["elements"][0]["content"], "**API unavailable**");
        assert_eq!(
            card["elements"][1]["elements"][0]["content"],
            "Alice 确认了这个告警。"
        );

        let actions = &card["elements"][2]["actions"];
        assert_eq!(actions[0]["text"]["content"], "解决");
        assert_eq!(
            actions[0]["value"][LARK_ALERT_VALUE_KEY],
            json!({
                "action": "resolve",
                "alert_id": "incident-1",
                "event_type": "alert.triggered",
                "policy_id": "policy-1",
            })
        );
        assert_eq!(
            actions[1]["url"],
            "https://example/alerts/incidents/incident-1"
        );

        let mut resolved = message.clone();
        resolved.metadata.insert("status".into(), "resolved".into());
        let card = lark_card(&resolved, &AlertButtons::default(), None);
        assert_eq!(card["header"]["template"], "green");
        assert_eq!(card["elements"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn a_template_card_brings_its_title_colour_fields_links_and_footer() {
        let message = NotifyMessage {
            title: "CRITICAL · Disk full".into(),
            text: "Disk is at 97%.".into(),
            markdown: Some("Disk is at **97%**.".into()),
            html: None,
            metadata: BTreeMap::from([("severity".to_string(), "critical".to_string())]),
            provider_templates: Default::default(),
            event: None,
            card: Some(NotifyCard {
                title: "Disk full on db-1".into(),
                color: NotifyCardColor::Orange,
                fields: vec![NotifyCardField {
                    label: "Host".into(),
                    value: "db-1".into(),
                }],
                links: vec![NotifyCardLink {
                    label: "Runbook".into(),
                    url: "https://runbooks.example.com/disk".into(),
                }],
                note: "MoleSignal".into(),
            }),
        };

        let card = lark_card(&message, &AlertButtons::default(), None);

        assert_eq!(card["header"]["title"]["content"], "Disk full on db-1");
        assert_eq!(card["header"]["template"], "orange");
        assert_eq!(card["elements"][0]["content"], "Disk is at **97%**.");
        assert_eq!(
            card["elements"][1]["fields"][0]["text"]["content"],
            "**Host**\ndb-1"
        );
        assert_eq!(
            card["elements"][2]["actions"][0]["url"],
            "https://runbooks.example.com/disk"
        );
        assert_eq!(card["elements"][3]["tag"], "hr");
        assert_eq!(card["elements"][4]["elements"][0]["content"], "MoleSignal");
    }

    #[test]
    fn a_group_bot_sends_the_card_or_the_text() {
        let mut message = NotifyMessage {
            title: "Disk full".into(),
            text: "Disk is at 97%.".into(),
            markdown: None,
            html: None,
            metadata: BTreeMap::new(),
            provider_templates: Default::default(),
            event: None,
            card: None,
        };

        assert_eq!(
            webhook_body(&message),
            json!({ "msg_type": "text", "content": { "text": "Disk full\nDisk is at 97%." } })
        );

        message.card = Some(NotifyCard {
            title: "Disk full on db-1".into(),
            ..NotifyCard::default()
        });

        let body = webhook_body(&message);
        assert_eq!(body["msg_type"], "interactive");
        assert_eq!(
            body["card"]["header"]["title"]["content"],
            "Disk full on db-1"
        );
    }
}
