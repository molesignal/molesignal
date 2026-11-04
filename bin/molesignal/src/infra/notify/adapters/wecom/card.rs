// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 企业微信的模板卡片（按钮交互型）：企业微信 App 与智能机器人发的带确认、解决等按钮的告警通知。
//!
//! 按钮的 `key` 是按钮的值（[`AlertButtonValue`] 的 JSON），点了以后企业微信原样带回来。模板卡片
//! 至少要有一个按钮：告警处理完、没有可做的操作时，换上一个点了没有反应的「✓」。
//!
//! [`AlertButtonValue`]: crate::domain::notify::buttons::AlertButtonValue

use serde_json::{Value, json};

use super::{TOKEN_ERRORS, WECOM_APP_CONNECTOR_TYPE, WecomAppConfig, WecomResponse};
use crate::{
    domain::notify::{
        buttons::{AlertAction, AlertButtons},
        connector::NotifyMessage,
    },
    infra::notify::adapters::{
        card::card_title,
        common::{TOKENS, client, provider_message, truncate_chars, with_query},
        phone::plain,
    },
    shared::{
        Error, Result,
        http_trace::{self, HttpTarget},
    },
};

/// 处理完以后那个点了没有反应的按钮的 `key`。
pub(crate) const WECOM_DONE_KEY: &str = "molesignal_done";

/// 企业微信建议的长度：一级标题 36 字、标题辅助信息 44 字、二级文本 160 字、按钮 10 字，二级
/// 标题加文本列表最多 6 项、按钮最多 6 个，按钮的 `key` 最长 1024 字节。
const TITLE_CHARS: usize = 36;
const DESC_CHARS: usize = 44;
const TEXT_CHARS: usize = 160;
const KEYNAME_CHARS: usize = 10;
const VALUE_CHARS: usize = 26;
const BUTTON_CHARS: usize = 10;
const MAX_ITEMS: usize = 6;
const MAX_KEY_BYTES: usize = 1024;

/// 一张按钮交互型的模板卡片。`task_id` 是它的 ID：点了以后换卡片要带着同一个。正文是通知的纯文字，
/// 模板卡片的字段是二级标题加文本，`note` 是一句谁做了什么，「在 MoleSignal 中查看」是点卡片打开的地址。
pub(crate) fn wecom_template_card(
    message: &NotifyMessage,
    buttons: &AlertButtons,
    note: Option<&str>,
    task_id: &str,
) -> Value {
    let card = message.card.as_ref();
    let title = card.map_or(message.title.as_str(), |card| card_title(card, message));

    let mut card_json = json!({
        "card_type": "button_interaction",
        "source": { "desc": "MoleSignal" },
        "main_title": { "title": truncate_chars(title.trim(), TITLE_CHARS) },
        "sub_title_text": truncate_chars(&plain(&message.text), TEXT_CHARS),
        "task_id": task_id,
    });

    if let Some(note) = note.filter(|note| !note.trim().is_empty()) {
        card_json["main_title"]["desc"] = json!(truncate_chars(note.trim(), DESC_CHARS));
    }

    let items = card
        .into_iter()
        .flat_map(|card| &card.fields)
        .take(MAX_ITEMS)
        .map(|field| {
            json!({
                "keyname": truncate_chars(&field.label, KEYNAME_CHARS),
                "value": truncate_chars(&field.value, VALUE_CHARS),
            })
        })
        .collect::<Vec<_>>();

    if !items.is_empty() {
        card_json["horizontal_content_list"] = json!(items);
    }

    if let Some((_, url)) = &buttons.open {
        card_json["card_action"] = json!({ "type": 1, "url": url });
    }

    let mut list = buttons
        .actions
        .iter()
        .filter_map(|(action, label)| {
            let key = serde_json::to_string(&buttons.value(*action)).ok()?;
            let primary = matches!(
                action,
                AlertAction::Acknowledge | AlertAction::AcknowledgeAll
            );

            (key.len() <= MAX_KEY_BYTES).then(|| {
                json!({
                    "text": truncate_chars(label, BUTTON_CHARS),
                    "style": if primary { 1 } else { 2 },
                    "key": key,
                })
            })
        })
        .take(MAX_ITEMS)
        .collect::<Vec<_>>();

    if list.is_empty() {
        list.push(json!({ "text": "✓", "style": 4, "key": WECOM_DONE_KEY }));
    }

    card_json["button_list"] = json!(list);
    card_json
}

impl WecomAppConfig {
    /// 调自建应用的一个接口：返回企业微信的回话，`errcode` 不是 0 算失败。
    async fn call(&self, path: &str, body: &Value, what: &str) -> Result<WecomResponse> {
        let http = client(self.timeout_secs, WECOM_APP_CONNECTOR_TYPE)?;
        let token = self.token(&http).await?;
        let url = with_query(
            &format!("{}/{path}", self.base()),
            &[("access_token", token.as_str())],
        )?;

        let response = http_trace::send(&http, http.post(url).json(body), HttpTarget::ThirdParty)
            .await
            .map_err(|_| Error::unavailable(format!("wecom_app {what} request failed")))?;
        let answer: WecomResponse = response
            .json()
            .await
            .map_err(|_| Error::unavailable(format!("wecom_app {what} response was invalid")))?;

        if TOKEN_ERRORS.contains(&answer.errcode) {
            TOKENS.forget(&self.token_key());
        }

        if answer.errcode != 0 {
            return Err(Error::unavailable(format!(
                "wecom_app could not {what}: {}",
                provider_message(&answer.errmsg)
            )));
        }

        Ok(answer)
    }
}

/// 换掉点过的那张模板卡片：`response_code` 是点击事件带来的，只能用一次；只换点的人眼前的这张。
pub(crate) async fn update_wecom_card(
    config: &Value,
    user: &str,
    response_code: &str,
    card: &Value,
) -> Result<()> {
    let config = WecomAppConfig::parse(config)?;
    let body = json!({
        "userids": [user],
        "agentid": config.agent_id()?,
        "response_code": response_code,
        "template_card": card,
    });

    config
        .call("message/update_template_card", &body, "update the card")
        .await
        .map(|_| ())
}

/// 自建应用发给一个成员一段文字：点了按钮以后告诉他为什么没做成。
pub(crate) async fn tell_wecom_member(config: &Value, user: &str, text: &str) -> Result<()> {
    let config = WecomAppConfig::parse(config)?;
    let body = json!({
        "touser": user,
        "msgtype": "text",
        "agentid": config.agent_id()?,
        "text": { "content": text },
    });

    config
        .call("message/send", &body, "send the message")
        .await
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::notify::{
        buttons::AlertButtonValue,
        template::{NotifyCard, NotifyCardColor, NotifyCardField},
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
    fn buttons_carry_their_value_as_the_key() {
        let buttons = AlertButtons {
            alert_id: "inc-1".into(),
            event_type: "alert.triggered".into(),
            policy_id: None,
            alert_ids: Vec::new(),
            actions: vec![
                (AlertAction::Acknowledge, "确认".into()),
                (AlertAction::Resolve, "解决".into()),
            ],
            open: Some(("查看".into(), "https://ms.example.com".into())),
        };
        let card = NotifyCard {
            title: "Disk full on db-1".into(),
            color: NotifyCardColor::Severity,
            fields: vec![NotifyCardField {
                label: "Host".into(),
                value: "db-1".into(),
            }],
            links: Vec::new(),
            note: String::new(),
        };

        let card =
            wecom_template_card(&message(Some(card)), &buttons, Some("Ada 确认了"), "task_1");

        assert_eq!(card["card_type"], "button_interaction");
        assert_eq!(card["task_id"], "task_1");
        assert_eq!(card["main_title"]["title"], "Disk full on db-1");
        assert_eq!(card["main_title"]["desc"], "Ada 确认了");
        assert_eq!(card["sub_title_text"], "Disk is at 97%.");
        assert_eq!(card["horizontal_content_list"][0]["keyname"], "Host");
        assert_eq!(card["card_action"]["url"], "https://ms.example.com");

        let first = &card["button_list"][0];
        assert_eq!(first["style"], 1);
        let value: AlertButtonValue = serde_json::from_str(first["key"].as_str().unwrap()).unwrap();
        assert_eq!(value.action, AlertAction::Acknowledge);
        assert_eq!(value.alert_id, "inc-1");
        assert_eq!(card["button_list"][1]["style"], 2);
    }

    #[test]
    fn a_handled_alert_keeps_one_button_that_does_nothing() {
        let card = wecom_template_card(&message(None), &AlertButtons::default(), None, "task_2");

        assert_eq!(card["main_title"]["title"], "CRITICAL · Disk full");
        assert_eq!(card["button_list"].as_array().unwrap().len(), 1);
        assert_eq!(card["button_list"][0]["key"], WECOM_DONE_KEY);
        assert!(card.get("card_action").is_none());
    }
}
