// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::net::TcpListener;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header, method, path},
};

use super::*;
use crate::{
    agent::im::AlertCard,
    domain::notify::{
        buttons::{AlertAction, AlertButtons},
        connector::NotifyMessage,
    },
};

#[test]
fn a_mention_in_a_channel_opens_a_thread_at_itself() {
    let event = json!({
        "type": "app_mention",
        "user": "U1",
        "text": "<@UBOT> is checkout &lt;ok&gt;?",
        "ts": "1700.1",
        "channel": "C1",
    });
    let mention = inbound(&event, "UBOT").unwrap();

    assert!(mention.addressed);
    assert_eq!(mention.kind, ConversationKind::Group);
    assert_eq!(mention.text, "is checkout <ok>?");
    assert_eq!(mention.thread_key, "1700.1");
    assert_eq!(mention.message_id, "C1:1700.1");
    assert_eq!(mention.reply["thread_ts"], "1700.1");
}

#[test]
fn the_same_mention_as_a_message_event_is_the_same_message() {
    let mention = json!({
        "type": "app_mention", "user": "U1", "text": "<@UBOT> hi", "ts": "1.2", "channel": "C1",
    });
    let message = json!({
        "type": "message", "user": "U1", "text": "<@UBOT> hi", "ts": "1.2", "channel": "C1",
        "channel_type": "channel",
    });

    assert_eq!(inbound(&mention, "UBOT"), inbound(&message, "UBOT"));
}

#[test]
fn channel_chatter_bots_and_edits_are_not_for_the_bot() {
    let chatter = json!({
        "type": "message", "user": "U1", "text": "lunch?", "ts": "1.3", "channel": "C1",
        "channel_type": "channel",
    });
    assert!(inbound(&chatter, "UBOT").is_none());

    let in_thread = json!({
        "type": "message", "user": "U1", "text": "and now?", "ts": "1.4", "thread_ts": "1.2",
        "channel": "C1", "channel_type": "channel",
    });
    let reply = inbound(&in_thread, "UBOT").unwrap();
    assert!(!reply.addressed);
    assert_eq!(reply.thread_key, "1.2");

    let own = json!({"type": "message", "user": "UBOT", "text": "x", "ts": "1.5", "channel": "D1"});
    let bot = json!({"type": "message", "bot_id": "B1", "user": "U2", "text": "x", "ts": "1.6", "channel": "D1"});
    let edit =
        json!({"type": "message", "subtype": "message_changed", "ts": "1.7", "channel": "D1"});
    assert!(inbound(&own, "UBOT").is_none());
    assert!(inbound(&bot, "UBOT").is_none());
    assert!(inbound(&edit, "UBOT").is_none());

    let direct = json!({
        "type": "message", "user": "U1", "text": "hello", "ts": "1.8", "channel": "D1",
        "channel_type": "im",
    });
    let direct = inbound(&direct, "UBOT").unwrap();
    assert_eq!(direct.kind, ConversationKind::Direct);
    assert_eq!(direct.thread_key, "");
    assert!(direct.reply["thread_ts"].is_null());
}

#[test]
fn markdown_falls_back_to_mrkdwn() {
    assert_eq!(
        mrkdwn("## Cause\n**pool** is full"),
        "*Cause*\n*pool* is full"
    );
}

fn alert_value(action: &str) -> String {
    json!({
        "action": action,
        "alert_id": "incident-1",
        "event_type": "alert.triggered",
        "policy_id": "policy-1",
    })
    .to_string()
}

fn press(action_id: &str, value: Option<&str>, channel: &str) -> Value {
    let mut pressed = json!({
        "type": "button", "action_id": action_id, "block_id": "molesignal_alert", "action_ts": "9.9",
    });

    if let Some(value) = value {
        pressed["value"] = json!(value);
    }

    json!({
        "type": "block_actions",
        "user": { "id": "U1", "username": "alice", "name": "alice", "team_id": "T1" },
        "container": {
            "type": "message", "message_ts": "1700.5", "channel_id": channel, "is_ephemeral": false,
        },
        "channel": { "id": channel, "name": "ops" },
        "message": { "type": "message", "ts": "1700.5", "text": "CRITICAL · API unavailable" },
        "response_url": "https://hooks.slack.com/actions/T1/1/abc",
        "actions": [pressed],
    })
}

#[test]
fn a_pressed_alert_button_reads_as_a_card_action() {
    let value = alert_value("acknowledge");
    let action = card_action(&press("molesignal_alert_acknowledge", Some(&value), "C1")).unwrap();

    assert_eq!(action.value.action, AlertAction::Acknowledge);
    assert_eq!(action.value.alert_id, "incident-1");
    assert_eq!(action.value.policy_id.as_deref(), Some("policy-1"));
    assert_eq!(action.sender.ids, ["U1"]);
    assert_eq!(action.kind, ConversationKind::Group);
    assert_eq!(
        action.reply,
        json!({ "channel": "C1", "ts": "1700.5", "thread_ts": null })
    );
    assert_eq!(
        action.card["response_url"],
        "https://hooks.slack.com/actions/T1/1/abc"
    );

    let direct = card_action(&press("molesignal_alert_resolve", Some(&value), "D1")).unwrap();
    assert_eq!(direct.kind, ConversationKind::Direct);

    // 链接按钮没有值；别的块、别的交互都不是告警按钮。
    assert!(card_action(&press("molesignal_alert_open", None, "C1")).is_none());

    let mut other = press("approve", Some(&value), "C1");
    other["actions"][0]["block_id"] = json!("other");
    assert!(card_action(&other).is_none());

    let mut shortcut = press("molesignal_alert_acknowledge", Some(&value), "C1");
    shortcut["type"] = json!("shortcut");
    assert!(card_action(&shortcut).is_none());
}

fn acknowledged_card() -> AlertCard {
    AlertCard {
        message: NotifyMessage {
            title: "CRITICAL · API unavailable".into(),
            text: "Alert API unavailable is acknowledged.".into(),
            markdown: None,
            html: None,
            metadata: Default::default(),
            provider_templates: Default::default(),
            event: None,
            card: None,
        },
        buttons: AlertButtons {
            alert_id: "incident-1".into(),
            event_type: "alert.triggered".into(),
            policy_id: None,
            alert_ids: Vec::new(),
            actions: vec![(AlertAction::Resolve, "解决".into())],
            open: None,
        },
        note: Some("Alice 确认了这个告警。".into()),
    }
}

#[tokio::test]
async fn an_answer_replaces_the_card_and_tells_only_the_presser_what_went_wrong() {
    let hooks = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/actions/1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true })))
        .mount(&hooks)
        .await;

    let value = alert_value("acknowledge");
    let mut action =
        card_action(&press("molesignal_alert_acknowledge", Some(&value), "C1")).unwrap();
    action.card = json!({ "response_url": format!("{}/actions/1", hooks.uri()) });
    let config = json!({ "botToken": "xoxb-bot" });

    // 做成了：只换卡片，卡片上写着谁确认的。
    answer(
        &config,
        &action,
        &CardAnswer {
            notice: Some((NoticeKind::Success, "已确认。".into())),
            card: Some(acknowledged_card()),
        },
    )
    .await
    .unwrap();

    let requests = hooks.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let replaced: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(replaced["replace_original"], true);
    assert_eq!(
        replaced["blocks"][1]["elements"][0]["text"],
        "Alice 确认了这个告警。"
    );
    assert_eq!(
        replaced["blocks"][2]["elements"][0]["action_id"],
        "molesignal_alert_resolve"
    );

    // 做不成：只告诉点的人。
    answer(
        &config,
        &action,
        &CardAnswer {
            notice: Some((
                NoticeKind::Warning,
                "你的 MoleSignal 账号不能确认或解决这个告警。".into(),
            )),
            card: None,
        },
    )
    .await
    .unwrap();

    let requests = hooks.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    let told: Value = serde_json::from_slice(&requests[1].body).unwrap();
    assert_eq!(told["response_type"], "ephemeral");
    assert_eq!(told["replace_original"], false);
}

#[derive(Default)]
struct Collect {
    connected: Mutex<Vec<String>>,
    messages: Mutex<Vec<Inbound>>,
    actions: Mutex<Vec<CardAction>>,
}

#[async_trait]
impl Listener for Collect {
    async fn connected(&self, detail: &str) {
        self.connected.lock().unwrap().push(detail.to_string());
    }

    async fn message(&self, inbound: Inbound) {
        self.messages.lock().unwrap().push(inbound);
    }

    async fn action(&self, action: CardAction) -> Option<CardAnswer> {
        self.actions.lock().unwrap().push(action);
        None
    }
}

#[tokio::test]
async fn socket_mode_acknowledges_each_envelope_and_reconnects_on_request() {
    let socket = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ws_port = socket.local_addr().unwrap().port();
    let acks = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen = acks.clone();

    tokio::spawn(async move {
        let (tcp, _) = socket.accept().await.unwrap();
        let mut server = tokio_tungstenite::accept_async(tcp).await.unwrap();

        server
            .send(Message::Text(json!({"type": "hello"}).to_string().into()))
            .await
            .unwrap();
        server
            .send(Message::Text(
                json!({
                    "envelope_id": "env-1",
                    "type": "events_api",
                    "payload": { "event": {
                        "type": "app_mention", "user": "U1", "text": "<@UBOT> status?",
                        "ts": "9.1", "channel": "C1"
                    }}
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
        server
            .send(Message::Text(
                json!({
                    "envelope_id": "env-2",
                    "type": "interactive",
                    "payload": press(
                        "molesignal_alert_acknowledge",
                        Some(&alert_value("acknowledge")),
                        "C1",
                    ),
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();

        while let Some(Ok(message)) = server.next().await {
            if let Message::Text(text) = message {
                seen.lock().unwrap().push(text.to_string());
                server
                    .send(Message::Text(
                        json!({"type": "disconnect", "reason": "refresh_requested"})
                            .to_string()
                            .into(),
                    ))
                    .await
                    .unwrap();
            }
        }
    });

    let api = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth.test"))
        .and(header("authorization", "Bearer xoxb-bot"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "ok": true, "user_id": "UBOT", "user": "mole" })),
        )
        .mount(&api)
        .await;
    Mock::given(method("POST"))
        .and(path("/apps.connections.open"))
        .and(header("authorization", "Bearer xapp-1-app"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                json!({ "ok": true, "url": format!("ws://127.0.0.1:{ws_port}/link") }),
            ),
        )
        .mount(&api)
        .await;

    let config = json!({
        "botToken": "xoxb-bot",
        "appToken": "xapp-1-app",
        "apiBaseUrl": api.uri(),
    });
    let listener = Collect::default();

    listen(&config, &listener, &CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(*listener.connected.lock().unwrap(), ["mole"]);
    assert_eq!(listener.messages.lock().unwrap()[0].text, "status?");
    assert_eq!(
        listener.actions.lock().unwrap()[0].value.action,
        AlertAction::Acknowledge
    );
    let acks = acks.lock().unwrap();
    assert_eq!(
        acks.first().map(String::as_str),
        Some(r#"{"envelope_id":"env-1"}"#)
    );
}

#[tokio::test]
async fn an_app_without_an_app_level_token_cannot_listen() {
    let error = listen(
        &json!({ "botToken": "xoxb-bot" }),
        &Collect::default(),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains("app-level token"));
}
