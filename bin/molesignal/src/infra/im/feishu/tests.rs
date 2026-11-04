// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::net::TcpListener;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

use super::*;
use crate::{
    agent::im::{AlertCard, CardAction, ConversationKind, Inbound, NoticeKind},
    domain::notify::{
        buttons::{AlertAction, AlertButtons},
        connector::NotifyMessage,
    },
};

fn header(key: &str, value: &str) -> Header {
    Header {
        key: key.into(),
        value: value.into(),
    }
}

fn data_frame(headers: Vec<Header>, payload: &[u8]) -> Frame {
    Frame {
        seq_id: 9,
        log_id: 3,
        service: 7,
        method: FRAME_DATA,
        headers,
        payload_encoding: None,
        payload_type: None,
        payload: Some(payload.to_vec()),
        log_id_new: None,
    }
}

fn receive_event(text: &str, chat_type: &str, root_id: &str) -> Value {
    json!({
        "schema": "2.0",
        "header": { "event_id": "ev1", "event_type": "im.message.receive_v1" },
        "event": {
            "sender": {
                "sender_id": { "open_id": "ou_alice", "user_id": "alice", "union_id": "on_alice" },
                "sender_type": "user"
            },
            "message": {
                "message_id": "om_2",
                "root_id": root_id,
                "chat_id": "oc_ops",
                "chat_type": chat_type,
                "message_type": "text",
                "content": json!({ "text": text }).to_string(),
                "mentions": [
                    { "key": "@_user_1", "id": { "open_id": "ou_bot" }, "name": "Mole" },
                    { "key": "@_user_2", "id": { "open_id": "ou_bob" }, "name": "Bob" }
                ]
            }
        }
    })
}

#[test]
fn a_ping_frame_carries_its_required_fields() {
    let bytes = Frame::ping(7).encode_to_vec();
    let decoded = Frame::decode(bytes.as_slice()).unwrap();

    assert_eq!(decoded.method, FRAME_CONTROL);
    assert_eq!(decoded.service, 7);
    assert_eq!(decoded.header("type"), Some("ping"));
    // 必填的 SeqID 与 LogID 是 0 也要写进去：字段 1 与 2 都在。
    assert_eq!(&bytes[..4], &[0x08, 0x00, 0x10, 0x00]);
}

#[test]
fn chunks_are_put_together_in_order() {
    let mut chunks = HashMap::new();
    let first = data_frame(
        vec![
            header("message_id", "m1"),
            header("sum", "2"),
            header("seq", "1"),
        ],
        b"world",
    );
    let second = data_frame(
        vec![
            header("message_id", "m1"),
            header("sum", "2"),
            header("seq", "0"),
        ],
        b"hello ",
    );

    assert_eq!(assemble(&mut chunks, &first), None);
    assert_eq!(
        assemble(&mut chunks, &second),
        Some(b"hello world".to_vec())
    );
    assert!(chunks.is_empty());

    let whole = data_frame(vec![header("message_id", "m2")], b"{}");
    assert_eq!(assemble(&mut chunks, &whole), Some(b"{}".to_vec()));
}

#[test]
fn the_acknowledgement_says_ok_and_how_long_it_took() {
    let frame = acknowledge(
        data_frame(vec![header("type", "event")], b"{}"),
        Duration::from_millis(12),
        None,
    );

    assert_eq!(frame.header("biz_rt"), Some("12"));
    let payload: Value = serde_json::from_slice(frame.payload.as_deref().unwrap()).unwrap();
    assert_eq!(payload["code"], 200);
    assert!(payload["data"].is_null());
    assert_eq!(frame.seq_id, 9);
}

fn card_event(value: Value) -> Value {
    json!({
        "schema": "2.0",
        "header": { "event_id": "ev2", "event_type": "card.action.trigger" },
        "event": {
            "operator": { "open_id": "ou_alice", "user_id": "alice", "union_id": "on_alice" },
            "token": "c-1",
            "action": { "tag": "button", "value": value },
            "context": { "open_message_id": "om_card", "open_chat_id": "oc_ops" }
        }
    })
}

fn resolve_value() -> Value {
    json!({
        "molesignal_alert": {
            "action": "resolve",
            "alert_id": "incident-1",
            "event_type": "alert.escalated",
        }
    })
}

fn acknowledged() -> CardAnswer {
    CardAnswer {
        notice: Some((NoticeKind::Success, "已确认。".into())),
        card: Some(AlertCard {
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
        }),
    }
}

#[test]
fn a_pressed_alert_button_names_who_pressed_it_on_which_card() {
    let action = card_action(&card_event(resolve_value())["event"]).unwrap();

    assert_eq!(action.value.action, AlertAction::Resolve);
    assert_eq!(action.value.event_type, "alert.escalated");
    assert_eq!(action.value.policy_id, None);
    assert_eq!(action.sender.ids, ["ou_alice", "alice", "on_alice"]);
    assert_eq!(action.conversation_id, "oc_ops");
    assert_eq!(action.kind, ConversationKind::Group);
    assert_eq!(action.card["message_id"], "om_card");

    // 别的按钮不是告警按钮。
    assert!(card_action(&card_event(json!({ "approve": true }))["event"]).is_none());
}

#[test]
fn the_answer_rides_back_in_the_acknowledgement() {
    let frame = acknowledge(
        data_frame(vec![header("type", "event")], b"{}"),
        Duration::from_millis(5),
        Some(&acknowledged()),
    );

    let payload: Value = serde_json::from_slice(frame.payload.as_deref().unwrap()).unwrap();
    assert_eq!(payload["code"], 200);

    let data = BASE64.decode(payload["data"].as_str().unwrap()).unwrap();
    let response: Value = serde_json::from_slice(&data).unwrap();

    assert_eq!(
        response["toast"],
        json!({ "type": "success", "content": "已确认。" })
    );
    assert_eq!(response["card"]["type"], "raw");
    assert_eq!(
        response["card"]["data"]["elements"][1]["elements"][0]["content"],
        "Alice 确认了这个告警。"
    );
    assert_eq!(
        response["card"]["data"]["elements"][2]["actions"][0]["text"]["content"],
        "解决"
    );
}

#[test]
fn a_pong_may_change_the_heartbeat() {
    let mut pong = Frame::ping(7);
    pong.headers = vec![header("type", "pong")];
    pong.payload = Some(br#"{"PingInterval":30,"ReconnectCount":-1}"#.to_vec());

    assert_eq!(pong_interval(&pong), Some(Duration::from_secs(30)));
    assert_eq!(pong_interval(&Frame::ping(7)), None);
}

#[test]
fn a_group_message_that_mentions_the_bot_starts_a_thread_at_itself() {
    let event = receive_event("@_user_1 why is @_user_2 paged?", "group", "");
    let mention = inbound(&event["event"], "ou_bot").unwrap();

    assert!(mention.addressed);
    assert_eq!(mention.kind, ConversationKind::Group);
    assert_eq!(mention.text, "why is @Bob paged?");
    assert_eq!(mention.thread_key, "om_2");
    assert_eq!(mention.sender.ids, ["ou_alice", "alice", "on_alice"]);
    assert_eq!(
        mention.reply,
        json!({ "message_id": "om_2", "in_thread": true })
    );
}

#[test]
fn a_reply_in_a_thread_keeps_its_root_and_a_direct_message_has_no_thread() {
    // 话题里的回复没 @ 机器人：事件里也就没有对它的 mention。
    let mut event = receive_event("and now?", "group", "om_1");
    event["event"]["message"]["mentions"] = json!([]);
    let in_thread = inbound(&event["event"], "ou_bot").unwrap();
    assert!(!in_thread.addressed);
    assert_eq!(in_thread.thread_key, "om_1");

    let event = receive_event("hello", "p2p", "");
    let direct = inbound(&event["event"], "ou_bot").unwrap();
    assert!(direct.addressed);
    assert_eq!(direct.kind, ConversationKind::Direct);
    assert_eq!(direct.thread_key, "");
}

#[test]
fn rich_text_reads_as_lines_and_other_messages_are_skipped() {
    let content = json!({
        "zh_cn": {
            "title": "Checkout",
            "content": [
                [{ "tag": "at", "user_id": "@_user_1" }, { "tag": "text", "text": " 5xx again" }],
                [{ "tag": "a", "text": "trace", "href": "https://ms.example/t/1" }]
            ]
        }
    });
    assert_eq!(post_text(&content), "Checkout\n@_user_1 5xx again\ntrace");

    let mut event = receive_event("x", "p2p", "");
    event["event"]["message"]["message_type"] = json!("image");
    assert!(inbound(&event["event"], "ou_bot").is_none());

    let mut event = receive_event("x", "p2p", "");
    event["event"]["sender"]["sender_type"] = json!("app");
    assert!(inbound(&event["event"], "ou_bot").is_none());
}

#[derive(Default)]
struct Collect {
    connected: Mutex<Vec<String>>,
    messages: Mutex<Vec<Inbound>>,
    actions: Mutex<Vec<CardAction>>,
    /// 点了按钮时在确认里带回去的。
    answer: Option<CardAnswer>,
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
        self.answer.clone()
    }
}

/// 飞书的开放接口：换令牌、读机器人、长连接地址指向 `ws_port`。
async fn feishu_api(ws_port: u16) -> MockServer {
    let api = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/open-apis/auth/v3/tenant_access_token/internal"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "code": 0, "tenant_access_token": "t-1", "expire": 7200 })),
        )
        .mount(&api)
        .await;
    Mock::given(method("GET"))
        .and(path("/open-apis/bot/v3/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({ "code": 0, "bot": { "open_id": "ou_bot", "app_name": "Mole" } }),
        ))
        .mount(&api)
        .await;
    Mock::given(method("POST"))
        .and(path("/callback/ws/endpoint"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 0,
            "data": {
                "URL": format!("ws://127.0.0.1:{ws_port}/ws?device_id=d1&service_id=7"),
                "ClientConfig": { "PingInterval": 120 }
            }
        })))
        .mount(&api)
        .await;
    api
}

/// 一条长连接的服务端：发出 `payload` 这一帧，收到确认后断开，交回那一帧确认。
async fn serve_one_frame(
    socket: TcpListener,
    frame_type: &'static str,
    payload: Value,
) -> Arc<Mutex<Option<Frame>>> {
    let acknowledged = Arc::new(Mutex::new(None::<Frame>));
    let seen = acknowledged.clone();

    tokio::spawn(async move {
        let (tcp, _) = socket.accept().await.unwrap();
        let mut server = tokio_tungstenite::accept_async(tcp).await.unwrap();

        let frame = data_frame(
            vec![
                header("type", frame_type),
                header("message_id", "m1"),
                header("sum", "1"),
            ],
            payload.to_string().as_bytes(),
        );
        server
            .send(Message::Binary(frame.encode_to_vec().into()))
            .await
            .unwrap();

        while let Some(Ok(message)) = server.next().await {
            if let Message::Binary(bytes) = message {
                let frame = Frame::decode(bytes.as_ref()).unwrap();

                if frame.method == FRAME_DATA {
                    *seen.lock().unwrap() = Some(frame);
                    let _ = server.close(None).await;
                    break;
                }
            }
        }
    });

    acknowledged
}

#[tokio::test]
async fn a_pressed_button_is_answered_in_its_acknowledgement() {
    let socket = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ws_port = socket.local_addr().unwrap().port();
    let seen = serve_one_frame(socket, "event", card_event(resolve_value())).await;
    let api = feishu_api(ws_port).await;

    let config = json!({
        "appId": "cli_test_feishu_card_action",
        "appSecret": "secret",
        "apiBaseUrl": format!("{}/open-apis", api.uri()),
    });
    let listener = Collect {
        answer: Some(acknowledged()),
        ..Collect::default()
    };

    listen(&config, &listener, &CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(listener.actions.lock().unwrap().len(), 1);
    assert!(listener.messages.lock().unwrap().is_empty());

    let ack = seen.lock().unwrap().clone().unwrap();
    let payload: Value = serde_json::from_slice(ack.payload.as_deref().unwrap()).unwrap();
    let data = BASE64.decode(payload["data"].as_str().unwrap()).unwrap();
    let response: Value = serde_json::from_slice(&data).unwrap();
    assert_eq!(response["toast"]["content"], "已确认。");
}

#[tokio::test]
async fn a_late_answer_updates_the_card_and_a_refusal_is_told_privately() {
    let api = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/open-apis/auth/v3/tenant_access_token/internal"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "code": 0, "tenant_access_token": "t-2", "expire": 7200 })),
        )
        .mount(&api)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/open-apis/im/v1/messages/om_card"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "code": 0 })))
        .mount(&api)
        .await;
    Mock::given(method("POST"))
        .and(path("/open-apis/im/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "code": 0 })))
        .mount(&api)
        .await;

    let config = json!({
        "appId": "cli_test_feishu_late_answer",
        "appSecret": "secret",
        "apiBaseUrl": format!("{}/open-apis", api.uri()),
    });
    let action = card_action(&card_event(resolve_value())["event"]).unwrap();

    answer(&config, &action, &acknowledged()).await.unwrap();

    answer(
        &config,
        &action,
        &CardAnswer {
            notice: Some((
                NoticeKind::Warning,
                "MoleSignal 里已经没有这个告警了。".into(),
            )),
            card: None,
        },
    )
    .await
    .unwrap();

    let requests = api.received_requests().await.unwrap();
    let patched = requests
        .iter()
        .find(|request| request.method.as_str() == "PATCH")
        .unwrap();
    let body: Value = serde_json::from_slice(&patched.body).unwrap();
    let card: Value = serde_json::from_str(body["content"].as_str().unwrap()).unwrap();
    assert_eq!(card["config"]["update_multi"], true);

    let told = requests
        .iter()
        .find(|request| request.url.path() == "/open-apis/im/v1/messages")
        .unwrap();
    let body: Value = serde_json::from_slice(&told.body).unwrap();
    assert_eq!(body["receive_id"], "ou_alice");
}

#[tokio::test]
async fn a_long_connection_hands_over_messages_and_acknowledges_them() {
    let socket = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ws_port = socket.local_addr().unwrap().port();
    let acknowledged = Arc::new(Mutex::new(None::<Frame>));
    let seen = acknowledged.clone();

    tokio::spawn(async move {
        let (tcp, _) = socket.accept().await.unwrap();
        let mut server = tokio_tungstenite::accept_async(tcp).await.unwrap();

        let event = receive_event("@_user_1 status?", "group", "").to_string();
        let frame = data_frame(
            vec![
                header("type", "event"),
                header("message_id", "m1"),
                header("sum", "1"),
            ],
            event.as_bytes(),
        );
        server
            .send(Message::Binary(frame.encode_to_vec().into()))
            .await
            .unwrap();

        while let Some(Ok(message)) = server.next().await {
            if let Message::Binary(bytes) = message {
                let frame = Frame::decode(bytes.as_ref()).unwrap();

                if frame.method == FRAME_DATA {
                    *seen.lock().unwrap() = Some(frame);
                    let _ = server.close(None).await;
                    break;
                }
            }
        }
    });

    let api = feishu_api(ws_port).await;

    let config = json!({
        "appId": "cli_test_feishu_listen",
        "appSecret": "secret",
        "apiBaseUrl": format!("{}/open-apis", api.uri()),
    });
    let listener = Collect::default();

    listen(&config, &listener, &CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(*listener.connected.lock().unwrap(), ["Mole"]);
    let messages = listener.messages.lock().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].text, "status?");

    let ack = acknowledged.lock().unwrap().clone().unwrap();
    assert!(ack.header("biz_rt").is_some());
    let payload: Value = serde_json::from_slice(ack.payload.as_deref().unwrap()).unwrap();
    assert_eq!(payload["code"], 200);
}
