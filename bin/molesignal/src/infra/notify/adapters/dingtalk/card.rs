// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 钉钉卡片：即时通讯模板有卡片时，应用机器人发 `sampleActionCard` 系列，群机器人发 `actionCard`；
//! 链接是卡片下面的按钮，没有链接时是 Markdown。

use serde_json::{Value, json};

use super::MAX_TEXT_CHARS;
use crate::{
    domain::notify::{connector::NotifyMessage, template::NotifyCard},
    infra::notify::adapters::{
        card::{card_color, card_title},
        common::{body, truncate_chars},
    },
};

/// 一张卡片的标题与 Markdown 正文：标题按颜色上色，再是正文，字段一行一个，页脚是灰字。
fn card_text(message: &NotifyMessage, card: &NotifyCard) -> (String, String) {
    let title = truncate_chars(card_title(card, message), 60);
    let color = card_color(card, message).hex();

    let mut text = format!(
        "#### <font color={color}>{title}</font>\n\n{}",
        body(message).trim()
    );

    for field in &card.fields {
        text.push_str(&format!("\n\n**{}**：{}", field.label, field.value));
    }

    if !card.note.is_empty() {
        text.push_str(&format!("\n\n<font color=#999999>{}</font>", card.note));
    }

    (title, truncate_chars(&text, MAX_TEXT_CHARS))
}

/// 卡片下面的按钮最多几个：多出的链接不发。
const MAX_CARD_BUTTONS: usize = 5;

/// 应用机器人发一张卡片：没有链接时是 `sampleMarkdown`；一个链接是 `sampleActionCard`，两到五个是
/// `sampleActionCard2` 到 `sampleActionCard5`，按钮竖着排。
pub(super) fn card_message(message: &NotifyMessage, card: &NotifyCard) -> (&'static str, Value) {
    let (title, text) = card_text(message, card);
    let mut param = json!({ "title": title, "text": text });
    let links = &card.links[..card.links.len().min(MAX_CARD_BUTTONS)];

    let msg_key = match links {
        [] => "sampleMarkdown",
        [link] => {
            param["singleTitle"] = json!(link.label);
            param["singleURL"] = json!(link.url);
            "sampleActionCard"
        }
        _ => {
            for (index, link) in links.iter().enumerate() {
                param[format!("actionTitle{}", index + 1)] = json!(link.label);
                param[format!("actionURL{}", index + 1)] = json!(link.url);
            }

            match links.len() {
                2 => "sampleActionCard2",
                3 => "sampleActionCard3",
                4 => "sampleActionCard4",
                _ => "sampleActionCard5",
            }
        }
    };

    (msg_key, param)
}

/// 群机器人发一张卡片：有链接时是 `actionCard`，一个链接整张卡片可点，几个时按钮竖着排；没有链接时是
/// `markdown`。
pub(super) fn webhook_card(message: &NotifyMessage, card: &NotifyCard) -> Value {
    let (title, text) = card_text(message, card);
    let links = &card.links[..card.links.len().min(MAX_CARD_BUTTONS)];

    match links {
        [] => json!({ "msgtype": "markdown", "markdown": { "title": title, "text": text } }),
        [link] => json!({
            "msgtype": "actionCard",
            "actionCard": {
                "title": title,
                "text": text,
                "singleTitle": link.label,
                "singleURL": link.url,
            },
        }),
        _ => {
            let buttons = links
                .iter()
                .map(|link| json!({ "title": link.label, "actionURL": link.url }))
                .collect::<Vec<_>>();

            json!({
                "msgtype": "actionCard",
                "actionCard": {
                    "title": title,
                    "text": text,
                    "btnOrientation": "0",
                    "btns": buttons,
                },
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card_with_links(count: usize) -> (NotifyMessage, NotifyCard) {
        let card = NotifyCard {
            title: "Disk full on db-1".into(),
            color: crate::domain::notify::template::NotifyCardColor::Red,
            fields: vec![crate::domain::notify::template::NotifyCardField {
                label: "Host".into(),
                value: "db-1".into(),
            }],
            links: (1..=count)
                .map(|index| crate::domain::notify::template::NotifyCardLink {
                    label: format!("Link {index}"),
                    url: format!("https://example.com/{index}"),
                })
                .collect(),
            note: "MoleSignal".into(),
        };

        let message = NotifyMessage {
            title: "CRITICAL · Disk full".into(),
            text: "Disk is at **97%**.".into(),
            markdown: None,
            html: None,
            metadata: Default::default(),
            provider_templates: Default::default(),
            event: None,
            card: Some(card.clone()),
        };

        (message, card)
    }

    #[test]
    fn a_card_is_an_action_card_with_as_many_buttons_as_links() {
        let (message, card) = card_with_links(0);
        let (key, param) = card_message(&message, &card);

        assert_eq!(key, "sampleMarkdown");
        assert_eq!(param["title"], "Disk full on db-1");
        assert_eq!(
            param["text"],
            "#### <font color=#D93025>Disk full on db-1</font>\n\nDisk is at **97%**.\n\n**Host**：db-1\n\n<font color=#999999>MoleSignal</font>"
        );

        let (message, card) = card_with_links(1);
        let (key, param) = card_message(&message, &card);
        assert_eq!(key, "sampleActionCard");
        assert_eq!(param["singleTitle"], "Link 1");
        assert_eq!(param["singleURL"], "https://example.com/1");

        let (message, card) = card_with_links(3);
        let (key, param) = card_message(&message, &card);
        assert_eq!(key, "sampleActionCard3");
        assert_eq!(param["actionTitle3"], "Link 3");
        assert_eq!(param["actionURL2"], "https://example.com/2");
    }

    #[test]
    fn a_group_bot_card_is_an_action_card_with_buttons() {
        let (message, card) = card_with_links(0);
        assert_eq!(webhook_card(&message, &card)["msgtype"], "markdown");

        let (message, card) = card_with_links(1);
        let single = webhook_card(&message, &card);
        assert_eq!(single["msgtype"], "actionCard");
        assert_eq!(single["actionCard"]["singleURL"], "https://example.com/1");

        let (message, card) = card_with_links(7);
        let several = webhook_card(&message, &card);
        assert_eq!(several["actionCard"]["btns"].as_array().unwrap().len(), 5);
        assert_eq!(
            several["actionCard"]["btns"][1]["actionURL"],
            "https://example.com/2"
        );
        assert_eq!(several["actionCard"]["title"], "Disk full on db-1");
    }
}
