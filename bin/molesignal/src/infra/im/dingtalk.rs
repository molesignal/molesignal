// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 钉钉：Stream 模式收机器人消息，回复用消息里的 sessionWebhook（过期了用应用机器人接口）。
//!
//! 用 Client ID 与 Client Secret 开一条连接，订阅机器人消息的回调。系统消息里的 ping 原样回，
//! disconnect 之后重连；回调先确认再交出消息。钉钉群里没有话题：一个人在一个群里的提问是一个
//! 会话，`/new` 换新的。钉钉的机器人不能加表情。
//!
//! 连接器填了互动卡片模板时还订阅卡片回调：告警卡片上按钮的点击（[`card_action`]），处理完按卡片的
//! `outTrackId` 换上新的变量，那句话单聊告诉点的人（[`answer`]）。

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::time::{Instant, interval_at};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use super::{Listener, ws};
use crate::{
    agent::im::{CardAction, CardAnswer, ConversationKind, Inbound, NoticeKind, Sender},
    domain::notify::buttons::AlertButtonValue,
    infra::notify::adapters::{
        DINGTALK_ALERT_PARAM, DINGTALK_API_BASE_URL, DINGTALK_CARD_CALLBACK_TOPIC, DingTalkApp,
        DingTalkRecipient, dingtalk_card_params, post_dingtalk_markdown,
    },
    shared::{
        Error, Result,
        http_trace::{self, HttpTarget},
    },
};

pub(crate) const BOT_MESSAGE_TOPIC: &str = "/v1.0/im/bot/messages/get";
const PING: Duration = Duration::from_secs(60);
/// sessionWebhook 过期前留这么久的余量。
const WEBHOOK_MARGIN_MS: i64 = 60_000;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}

fn api_base(config: &Value) -> String {
    config["apiBaseUrl"]
        .as_str()
        .filter(|base| !base.trim().is_empty())
        .unwrap_or(DINGTALK_API_BASE_URL)
        .trim()
        .trim_end_matches('/')
        .to_string()
}

/// 开一条 Stream 连接：返回 `endpoint?ticket=…`。
async fn open(http: &reqwest::Client, config: &Value, app: &DingTalkApp) -> Result<String> {
    let mut subscriptions = vec![json!({ "type": "CALLBACK", "topic": BOT_MESSAGE_TOPIC })];

    if app.card_template_id().is_some() {
        subscriptions.push(json!({ "type": "CALLBACK", "topic": DINGTALK_CARD_CALLBACK_TOPIC }));
    }

    let response = http_trace::send(
        http,
        http.post(format!(
            "{}/v1.0/gateway/connections/open",
            api_base(config)
        ))
        .json(&json!({
            "clientId": app.client_id.trim(),
            "clientSecret": app.client_secret().trim(),
            "subscriptions": subscriptions,
            "ua": "molesignal",
            "localIp": "",
        })),
        HttpTarget::ThirdParty,
    )
    .await
    .map_err(|error| Error::unavailable(format!("DingTalk Stream request failed: {error}")))?;

    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);

    let (Some(endpoint), Some(ticket)) = (body["endpoint"].as_str(), body["ticket"].as_str())
    else {
        return Err(Error::unavailable(format!(
            "DingTalk refused the Stream connection: HTTP {} {}",
            status.as_u16(),
            body["message"].as_str().unwrap_or("unknown_error")
        )));
    };

    let mut url = url::Url::parse(endpoint)
        .map_err(|_| Error::unavailable("DingTalk returned an invalid Stream address"))?;
    url.query_pairs_mut().append_pair("ticket", ticket);
    Ok(url.to_string())
}

/// 回给钉钉的确认：带上原消息的 ID；`data` 是 JSON 文本。
fn acknowledge(message_id: &str, data: String) -> String {
    json!({
        "code": 200,
        "headers": { "contentType": "application/json", "messageId": message_id },
        "message": "OK",
        "data": data,
    })
    .to_string()
}

pub(super) async fn listen(
    config: &Value,
    listener: &dyn Listener,
    cancel: &CancellationToken,
) -> Result<()> {
    let app = DingTalkApp::parse(config)?;
    let http = app.http()?;
    let url = open(&http, config, &app).await?;

    let mut socket = ws::connect(&url).await?;
    listener.connected(app.client_id.trim()).await;

    let mut ticker = interval_at(Instant::now() + PING, PING);
    let mut heard = Instant::now();

    loop {
        tokio::select! {
            () = cancel.cancelled() => {
                let _ = socket.close(None).await;
                return Ok(());
            }
            _ = ticker.tick() => {
                if heard.elapsed() > PING * 3 {
                    return Err(Error::unavailable("DingTalk stopped answering"));
                }

                socket
                    .send(Message::Ping(Vec::new().into()))
                    .await
                    .map_err(|error| Error::unavailable(format!("DingTalk connection lost: {error}")))?;
            }
            message = socket.next() => {
                let Some(message) = message else {
                    return Ok(());
                };

                let message = message
                    .map_err(|error| Error::unavailable(format!("DingTalk connection lost: {error}")))?;
                heard = Instant::now();

                let frame: Value = match message {
                    Message::Text(text) => match serde_json::from_str(&text) {
                        Ok(frame) => frame,
                        Err(_) => continue,
                    },
                    Message::Close(_) => return Ok(()),
                    _ => continue,
                };

                let kind = frame["type"].as_str().unwrap_or_default();
                let topic = frame["headers"]["topic"].as_str().unwrap_or_default();
                let id = frame["headers"]["messageId"].as_str().unwrap_or_default().to_string();
                let data = frame["data"].as_str().unwrap_or_default().to_string();

                if kind == "SYSTEM" && topic == "disconnect" {
                    return Ok(());
                }

                let answer = if kind == "CALLBACK" {
                    acknowledge(&id, json!({ "response": null }).to_string())
                } else {
                    acknowledge(&id, data.clone())
                };

                socket
                    .send(Message::Text(answer.into()))
                    .await
                    .map_err(|error| Error::unavailable(format!("DingTalk connection lost: {error}")))?;

                if kind != "CALLBACK" {
                    continue;
                }

                let Ok(data) = serde_json::from_str::<Value>(&data) else {
                    continue;
                };

                if topic == BOT_MESSAGE_TOPIC
                    && let Some(inbound) = inbound(&data)
                {
                    listener.message(inbound).await;
                } else if topic == DINGTALK_CARD_CALLBACK_TOPIC
                    && let Some(action) = card_action(&data)
                {
                    listener.action(action).await;
                }
            }
        }
    }
}

/// 机器人收到的一条消息。只认文本与富文本里的文字。
pub(crate) fn inbound(data: &Value) -> Option<Inbound> {
    let message_id = data["msgId"].as_str()?.to_string();
    let conversation = data["conversationId"].as_str()?.to_string();
    let staff = data["senderStaffId"]
        .as_str()
        .filter(|staff| !staff.is_empty())?
        .to_string();
    let direct = data["conversationType"].as_str() == Some("1");

    let text = match data["msgtype"].as_str()? {
        "text" => data["text"]["content"].as_str()?.to_string(),
        "richText" => data["content"]["richText"]
            .as_array()?
            .iter()
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join(""),
        _ => return None,
    };

    Some(Inbound {
        message_id,
        conversation_id: conversation.clone(),
        kind: if direct {
            ConversationKind::Direct
        } else {
            ConversationKind::Group
        },
        thread_key: if direct {
            String::new()
        } else {
            format!("user:{staff}")
        },
        sender: Sender {
            ids: vec![staff.clone()],
            name: data["senderNick"].as_str().map(str::to_string),
        },
        text: text.trim().to_string(),
        addressed: true,
        reply: json!({
            "session_webhook": data["sessionWebhook"],
            "expires_at": data["sessionWebhookExpiredTime"],
            "conversation_id": conversation,
            "direct": direct,
            "sender_staff_id": staff,
        }),
    })
}

/// 点了告警卡片上的按钮：卡片回调里「回传请求」带回的参数是按钮的值。别的卡片的回调不算。
pub(crate) fn card_action(data: &Value) -> Option<CardAction> {
    // 回调的内容是一段 JSON 文本。
    let content = match &data["content"] {
        Value::String(text) => serde_json::from_str::<Value>(text).ok()?,
        content => content.clone(),
    };

    let value: AlertButtonValue = match &content["cardPrivateData"]["params"][DINGTALK_ALERT_PARAM]
    {
        Value::String(text) => serde_json::from_str(text).ok()?,
        value => serde_json::from_value(value.clone()).ok()?,
    };

    let card = data["outTrackId"].as_str().filter(|id| !id.is_empty())?;
    let staff = data["userId"].as_str().filter(|staff| !staff.is_empty())?;
    let group = data["spaceType"].as_str() == Some("IM_GROUP");
    let conversation = data["spaceId"]
        .as_str()
        .filter(|space| group && !space.is_empty())
        .unwrap_or(staff);

    Some(CardAction {
        value,
        sender: Sender {
            ids: vec![staff.to_string()],
            name: None,
        },
        conversation_id: conversation.to_string(),
        kind: if group {
            ConversationKind::Group
        } else {
            ConversationKind::Direct
        },
        reply: json!({
            "conversation_id": conversation,
            "direct": !group,
            "sender_staff_id": staff,
        }),
        card: json!({ "out_track_id": card }),
    })
}

/// 点击的结果：按卡片的 `outTrackId` 换上新的变量，那句话单聊告诉点的人（做成了的不用说，卡片上写着）。
pub(super) async fn answer(config: &Value, action: &CardAction, answer: &CardAnswer) -> Result<()> {
    if let Some(card) = &answer.card {
        let id = action.card["out_track_id"]
            .as_str()
            .ok_or_else(|| Error::invalid("the DingTalk card to update is unknown"))?;
        let app = DingTalkApp::parse(config)?;
        let params = dingtalk_card_params(&card.message, &card.buttons, card.note.as_deref());

        app.update_card(&app.http()?, id, &params).await?;
    }

    if let Some((kind, text)) = &answer.notice
        && (answer.card.is_none() || *kind != NoticeKind::Success)
        && let Some(staff) = action.sender.ids.first()
    {
        tell(config, staff, text).await?;
    }

    Ok(())
}

/// 回复的标题：正文的第一行（钉钉在通知里显示它）。
fn title(markdown: &str) -> String {
    markdown
        .lines()
        .map(|line| line.trim().trim_start_matches('#').trim())
        .find(|line| !line.is_empty())
        .unwrap_or("Mole Agent")
        .chars()
        .take(40)
        .collect()
}

pub(super) async fn reply(config: &Value, reply: &Value, markdown: &str) -> Result<()> {
    let app = DingTalkApp::parse(config)?;
    let http = app.http()?;
    let title = title(markdown);

    if let Some(webhook) = reply["session_webhook"].as_str()
        && reply["expires_at"].as_i64().unwrap_or_default() - WEBHOOK_MARGIN_MS > now_ms()
    {
        return post_dingtalk_markdown(&http, webhook, &title, markdown).await;
    }

    let staff = [reply["sender_staff_id"]
        .as_str()
        .unwrap_or_default()
        .to_string()];
    let to = if reply["direct"].as_bool().unwrap_or(false) {
        DingTalkRecipient::Users(&staff)
    } else {
        DingTalkRecipient::Conversation(reply["conversation_id"].as_str().unwrap_or_default())
    };

    app.send(&http, to, &title, markdown).await.map(|_| ())
}

/// 单聊发给一个人。
pub(super) async fn tell(config: &Value, staff_id: &str, markdown: &str) -> Result<()> {
    let app = DingTalkApp::parse(config)?;
    let http = app.http()?;
    let users = [staff_id.to_string()];

    app.send(
        &http,
        DingTalkRecipient::Users(&users),
        &title(markdown),
        markdown,
    )
    .await
    .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn robot_message(conversation_type: &str) -> Value {
        json!({
            "conversationId": "cidAbc",
            "msgId": "msg1",
            "senderNick": "Alice",
            "senderStaffId": "staff1",
            "sessionWebhookExpiredTime": 1_700_000_600_000_i64,
            "conversationType": conversation_type,
            "sessionWebhook": "https://oapi.dingtalk.com/robot/sendBySession?session=s",
            "text": { "content": "  checkout 5xx?  " },
            "msgtype": "text"
        })
    }

    #[test]
    fn a_group_message_is_its_senders_own_thread() {
        let group = inbound(&robot_message("2")).unwrap();

        assert_eq!(group.kind, ConversationKind::Group);
        assert_eq!(group.thread_key, "user:staff1");
        assert_eq!(group.text, "checkout 5xx?");
        assert_eq!(group.sender.name.as_deref(), Some("Alice"));
        assert_eq!(group.reply["conversation_id"], "cidAbc");
        assert!(group.addressed);

        let direct = inbound(&robot_message("1")).unwrap();
        assert_eq!(direct.thread_key, "");
        assert_eq!(direct.reply["direct"], true);
    }

    #[test]
    fn rich_text_is_read_and_pictures_are_not() {
        let mut message = robot_message("1");
        message["msgtype"] = json!("richText");
        message["content"] = json!({ "richText": [{ "text": "disk " }, { "text": "full" }] });
        assert_eq!(inbound(&message).unwrap().text, "disk full");

        message["msgtype"] = json!("picture");
        assert!(inbound(&message).is_none());
    }

    #[test]
    fn an_acknowledgement_carries_the_message_id() {
        let ack: Value = serde_json::from_str(&acknowledge("m-1", "{}".into())).unwrap();

        assert_eq!(ack["code"], 200);
        assert_eq!(ack["headers"]["messageId"], "m-1");
        assert_eq!(ack["headers"]["contentType"], "application/json");
        assert_eq!(ack["data"], "{}");
    }

    #[test]
    fn a_reply_is_titled_by_its_first_line() {
        assert_eq!(title("\n## Checkout is slow\nbecause"), "Checkout is slow");
        assert_eq!(title(""), "Mole Agent");
    }

    fn card_callback(space_type: &str, params: Value) -> Value {
        json!({
            "outTrackId": "molesignal-card-1",
            "corpId": "ding-corp",
            "userId": "staff1",
            "userIdType": 1,
            "spaceType": space_type,
            "spaceId": "cidOps",
            "type": "actionCallback",
            "content": json!({ "cardPrivateData": { "actionIds": ["acknowledge"], "params": params } })
                .to_string(),
        })
    }

    fn alert_value() -> String {
        json!({ "action": "acknowledge", "alert_id": "inc-1", "event_type": "alert.triggered" })
            .to_string()
    }

    #[test]
    fn a_card_callback_carries_the_buttons_value() {
        let group = card_action(&card_callback(
            "IM_GROUP",
            json!({ DINGTALK_ALERT_PARAM: alert_value() }),
        ))
        .unwrap();

        assert_eq!(group.value.alert_id, "inc-1");
        assert_eq!(group.sender.ids, ["staff1"]);
        assert_eq!(group.kind, ConversationKind::Group);
        assert_eq!(group.conversation_id, "cidOps");
        assert_eq!(group.card, json!({ "out_track_id": "molesignal-card-1" }));
        assert_eq!(group.reply["direct"], false);

        let direct = card_action(&card_callback(
            "IM_ROBOT",
            json!({ DINGTALK_ALERT_PARAM: alert_value() }),
        ))
        .unwrap();
        assert_eq!(direct.kind, ConversationKind::Direct);
        assert_eq!(direct.conversation_id, "staff1");

        // 别的卡片：回传参数里没有按钮的值。
        assert!(card_action(&card_callback("IM_GROUP", json!({ "vote": "yes" }))).is_none());
    }

    #[tokio::test]
    async fn a_pressed_card_gets_new_variables_and_the_presser_a_direct_message() {
        use wiremock::{
            Mock, MockServer, ResponseTemplate,
            matchers::{body_partial_json, method, path},
        };

        use crate::{
            agent::im::AlertCard,
            domain::notify::{
                buttons::{AlertAction, AlertButtons},
                connector::NotifyMessage,
            },
        };

        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1.0/oauth2/accessToken"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "accessToken": "dt", "expireIn": 7200 })),
            )
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/v1.0/card/instances"))
            .and(body_partial_json(json!({
                "outTrackId": "molesignal-card-1",
                "cardData": { "cardParamMap": { "acknowledge_text": "", "resolve_text": "解决" } },
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "success": true })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1.0/robot/oToMessages/batchSend"))
            .and(body_partial_json(json!({ "userIds": ["staff1"] })))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "processQueryKey": "q1" })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let config = json!({
            "clientId": "ding-card-client",
            "clientSecret": "s",
            "apiBaseUrl": server.uri(),
            "cardTemplateId": "tpl-1.schema",
        });
        let action = card_action(&card_callback(
            "IM_GROUP",
            json!({ DINGTALK_ALERT_PARAM: alert_value() }),
        ))
        .unwrap();

        let card = AlertCard {
            message: NotifyMessage {
                title: "CRITICAL · Disk full".into(),
                text: "Disk is at **97%**.".into(),
                markdown: None,
                html: None,
                metadata: Default::default(),
                provider_templates: Default::default(),
                event: None,
                card: None,
            },
            buttons: AlertButtons {
                alert_id: "inc-1".into(),
                event_type: "alert.triggered".into(),
                policy_id: None,
                alert_ids: Vec::new(),
                actions: vec![(AlertAction::Resolve, "解决".into())],
                open: None,
            },
            note: Some("staff1 确认了".into()),
        };

        answer(
            &config,
            &action,
            &CardAnswer {
                notice: Some((NoticeKind::Info, "这个告警已经确认过了".into())),
                card: Some(card),
            },
        )
        .await
        .unwrap();
    }
}
