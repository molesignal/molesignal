// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 钉钉的互动卡片：带确认、解决等按钮的告警通知。
//!
//! 卡片的样子是连接器里「互动卡片模板 ID」指的模板（在钉钉开发者后台的卡片平台建好、发布），这里只填
//! 它的变量（[`card_params`]）。按钮的点击经 Stream 连接的卡片回调回来
//! （[`DINGTALK_CARD_CALLBACK_TOPIC`]），点了以后按卡片的 `outTrackId` 换上新的变量
//! （[`DingTalkApp::update_card`]）。
//!
//! 模板里的变量都是文字：
//! - `title`、`content`（Markdown，含模板卡片的字段与链接）、`note`（一句谁做了什么）、`footer`（模板
//!   卡片的页脚）；
//! - 每个操作两个：`acknowledge_text` / `acknowledge_value`，`resolve_*`、`snooze_*`、`acknowledge_all_*`。
//!   按钮在 `*_text` 不为空时显示，点了是「回传请求」，回传参数 [`DINGTALK_ALERT_PARAM`] 绑定 `*_value`；
//! - `open_text` / `open_url`：在 MoleSignal 中查看。

use serde_json::{Map, Value, json};

use super::{DingTalkApp, MAX_TEXT_CHARS, Recipient};
use crate::{
    domain::notify::{
        buttons::{AlertAction, AlertButtons},
        connector::NotifyMessage,
    },
    infra::notify::adapters::{
        card::card_title,
        common::{TOKENS, body, provider_message, truncate_chars},
    },
    shared::{
        Error, Result,
        http_trace::{self, HttpTarget},
    },
};

/// Stream 连接里卡片回调的主题。
pub(crate) const DINGTALK_CARD_CALLBACK_TOPIC: &str = "/v1.0/card/instances/callback";
/// 回传参数里按钮的值（[`crate::domain::notify::buttons::AlertButtonValue`] 的 JSON）放在这个键下。
pub(crate) const DINGTALK_ALERT_PARAM: &str = "value";

/// 填进模板的变量。
pub(crate) fn card_params(
    message: &NotifyMessage,
    buttons: &AlertButtons,
    note: Option<&str>,
) -> Value {
    let card = message.card.as_ref();
    let title = card.map_or(message.title.as_str(), |card| card_title(card, message));

    let mut content = body(message).trim().to_string();

    for field in card.into_iter().flat_map(|card| &card.fields) {
        content.push_str(&format!("\n\n**{}**：{}", field.label, field.value));
    }

    for link in card.into_iter().flat_map(|card| &card.links) {
        content.push_str(&format!("\n\n[{}]({})", link.label, link.url));
    }

    let mut params = Map::new();
    params.insert("title".into(), json!(truncate_chars(title.trim(), 60)));
    params.insert(
        "content".into(),
        json!(truncate_chars(&content, MAX_TEXT_CHARS)),
    );
    params.insert("note".into(), json!(note.unwrap_or_default()));
    params.insert(
        "footer".into(),
        json!(card.map(|card| card.note.as_str()).unwrap_or_default()),
    );

    for action in AlertAction::ALL {
        let (text, value) = match buttons.actions.iter().find(|(shown, _)| *shown == action) {
            Some((_, label)) => (
                label.clone(),
                serde_json::to_string(&buttons.value(action)).unwrap_or_default(),
            ),
            None => (String::new(), String::new()),
        };

        params.insert(format!("{}_text", action.as_str()), json!(text));
        params.insert(format!("{}_value", action.as_str()), json!(value));
    }

    let (open_text, open_url) = buttons.open.clone().unwrap_or_default();
    params.insert("open_text".into(), json!(open_text));
    params.insert("open_url".into(), json!(open_url));

    Value::Object(params)
}

impl DingTalkApp {
    /// 连接器里填的互动卡片模板；没填时告警通知不带按钮。
    pub(crate) fn card_template_id(&self) -> Option<&str> {
        Some(self.card_template_id.trim()).filter(|template| !template.is_empty())
    }

    /// 发一张互动卡片（单聊发给一个人，或发到群里）。`out_track_id` 是它的 ID，点了以后按它换变量。
    pub(crate) async fn deliver_card(
        &self,
        http: &reqwest::Client,
        to: Recipient<'_>,
        template: &str,
        out_track_id: &str,
        params: &Value,
    ) -> Result<()> {
        let mut card = json!({
            "cardTemplateId": template,
            "outTrackId": out_track_id,
            "cardData": { "cardParamMap": params },
            "callbackType": "STREAM",
            "imGroupOpenSpaceModel": { "supportForward": true },
            "imRobotOpenSpaceModel": { "supportForward": true },
            "userIdType": 1,
        });

        match to {
            Recipient::Users(users) => {
                let user = users
                    .first()
                    .ok_or_else(|| Error::invalid("dingtalk_app card has no recipient"))?;

                card["openSpaceId"] = json!(format!("dtv1.card//IM_ROBOT.{user}"));
                card["imRobotOpenDeliverModel"] =
                    json!({ "spaceType": "IM_ROBOT", "robotCode": self.robot_code() });
            }
            Recipient::Conversation(conversation) => {
                card["openSpaceId"] = json!(format!("dtv1.card//IM_GROUP.{conversation}"));
                card["imGroupOpenDeliverModel"] = json!({ "robotCode": self.robot_code() });
            }
        }

        let delivered = self
            .card_request(
                http,
                reqwest::Method::POST,
                "/v1.0/card/instances/createAndDeliver",
                &card,
            )
            .await?;

        // 每个投放处各有成败：一处都没送到算失败。
        let results = delivered["result"]["deliverResults"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        if let Some(failed) = results
            .iter()
            .find(|result| result["success"] == false)
            .filter(|_| results.iter().all(|result| result["success"] == false))
        {
            return Err(Error::unavailable(format!(
                "dingtalk_app could not deliver the card: {}",
                provider_message(failed["errorMsg"].as_str().unwrap_or_default())
            )));
        }

        Ok(())
    }

    /// 换掉一张卡片的变量，没给的不动。
    pub(crate) async fn update_card(
        &self,
        http: &reqwest::Client,
        out_track_id: &str,
        params: &Value,
    ) -> Result<()> {
        self.card_request(
            http,
            reqwest::Method::PUT,
            "/v1.0/card/instances",
            &json!({
                "outTrackId": out_track_id,
                "cardData": { "cardParamMap": params },
                "cardUpdateOptions": { "updateCardDataByKey": true },
            }),
        )
        .await
        .map(|_| ())
    }

    async fn card_request(
        &self,
        http: &reqwest::Client,
        method: reqwest::Method,
        path: &str,
        body: &Value,
    ) -> Result<Value> {
        let token = self.access_token(http).await?;

        let response = http_trace::send(
            http,
            http.request(method, format!("{}{path}", self.base()))
                .header("x-acs-dingtalk-access-token", token)
                .json(body),
            HttpTarget::ThirdParty,
        )
        .await
        .map_err(|_| Error::unavailable("dingtalk_app card request failed"))?;

        let status = response.status();
        let answer: Value = response.json().await.unwrap_or(Value::Null);

        if status == reqwest::StatusCode::UNAUTHORIZED {
            TOKENS.forget(&self.token_key());
        }

        if !status.is_success() || answer["success"] == false {
            return Err(Error::unavailable(format!(
                "dingtalk_app rejected the card: HTTP {} {}",
                status.as_u16(),
                provider_message(answer["message"].as_str().unwrap_or_default())
            )));
        }

        Ok(answer)
    }
}

#[cfg(test)]
mod tests {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_partial_json, header, method, path},
    };

    use super::*;
    use crate::domain::notify::{
        buttons::AlertButtonValue,
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

    fn buttons() -> AlertButtons {
        AlertButtons {
            alert_id: "inc-1".into(),
            event_type: "alert.triggered".into(),
            policy_id: Some("policy-1".into()),
            alert_ids: Vec::new(),
            actions: vec![
                (AlertAction::Acknowledge, "确认".into()),
                (AlertAction::Resolve, "解决".into()),
            ],
            open: Some((
                "在 MoleSignal 中查看".into(),
                "https://ms.example.com".into(),
            )),
        }
    }

    #[test]
    fn the_template_gets_texts_and_values_for_every_button_shown() {
        let card = NotifyCard {
            title: "Disk full on db-1".into(),
            color: NotifyCardColor::Severity,
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

        let params = card_params(&message(Some(card)), &buttons(), Some("Ada 确认了"));

        assert_eq!(params["title"], "Disk full on db-1");
        assert_eq!(
            params["content"],
            "Disk is at **97%**.\n\n**Host**：db-1\n\n[Runbook](https://runbooks.example.com/disk)"
        );
        assert_eq!(params["note"], "Ada 确认了");
        assert_eq!(params["footer"], "MoleSignal");

        assert_eq!(params["acknowledge_text"], "确认");
        let value: AlertButtonValue =
            serde_json::from_str(params["acknowledge_value"].as_str().unwrap()).unwrap();
        assert_eq!(value.action, AlertAction::Acknowledge);
        assert_eq!(value.alert_id, "inc-1");

        // 没画的按钮：字是空的，模板按它藏起来。
        assert_eq!(params["snooze_text"], "");
        assert_eq!(params["snooze_value"], "");
        assert_eq!(params["open_url"], "https://ms.example.com");

        // 每个变量都是文字。
        assert!(params.as_object().unwrap().values().all(Value::is_string));
    }

    fn app(server: &MockServer) -> DingTalkApp {
        DingTalkApp::parse(&json!({
            "clientId": "ding-client",
            "clientSecret": "s",
            "apiBaseUrl": server.uri(),
            "cardTemplateId": "tpl-1.schema",
        }))
        .unwrap()
    }

    async fn token(server: &MockServer) {
        Mock::given(method("POST"))
            .and(path("/v1.0/oauth2/accessToken"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "accessToken": "dt", "expireIn": 7200 })),
            )
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn a_card_goes_to_a_group_with_stream_callbacks_and_is_updated_by_key() {
        let server = MockServer::start().await;
        token(&server).await;

        Mock::given(method("POST"))
            .and(path("/v1.0/card/instances/createAndDeliver"))
            .and(header("x-acs-dingtalk-access-token", "dt"))
            .and(body_partial_json(json!({
                "cardTemplateId": "tpl-1.schema",
                "outTrackId": "card-1",
                "callbackType": "STREAM",
                "openSpaceId": "dtv1.card//IM_GROUP.cid-ops",
                "imGroupOpenDeliverModel": { "robotCode": "ding-client" },
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "success": true,
                "result": { "deliverResults": [{ "spaceType": "IM_GROUP", "success": true }] },
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/v1.0/card/instances"))
            .and(body_partial_json(json!({
                "outTrackId": "card-1",
                "cardData": { "cardParamMap": { "note": "Ada 确认了" } },
                "cardUpdateOptions": { "updateCardDataByKey": true },
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "success": true })))
            .expect(1)
            .mount(&server)
            .await;

        let app = app(&server);
        let http = app.http().unwrap();
        assert_eq!(app.card_template_id(), Some("tpl-1.schema"));

        app.deliver_card(
            &http,
            Recipient::Conversation("cid-ops"),
            "tpl-1.schema",
            "card-1",
            &card_params(&message(None), &buttons(), None),
        )
        .await
        .unwrap();

        app.update_card(&http, "card-1", &json!({ "note": "Ada 确认了" }))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_card_no_space_took_is_a_failure() {
        let server = MockServer::start().await;
        token(&server).await;

        Mock::given(method("POST"))
            .and(path("/v1.0/card/instances/createAndDeliver"))
            .and(body_partial_json(json!({
                "openSpaceId": "dtv1.card//IM_ROBOT.staff1",
                "imRobotOpenDeliverModel": { "spaceType": "IM_ROBOT", "robotCode": "ding-client" },
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "success": true,
                "result": {
                    "deliverResults": [{ "success": false, "errorMsg": "robot not in the chat" }],
                },
            })))
            .mount(&server)
            .await;

        let app = app(&server);
        let users = ["staff1".to_string()];

        let error = app
            .deliver_card(
                &app.http().unwrap(),
                Recipient::Users(&users),
                "tpl-1.schema",
                "card-2",
                &card_params(&message(None), &buttons(), None),
            )
            .await
            .unwrap_err();

        assert!(error.to_string().contains("robot not in the chat"));
    }
}
