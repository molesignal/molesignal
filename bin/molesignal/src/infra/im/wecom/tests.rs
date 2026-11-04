// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::net::TcpListener;

use super::*;
use crate::{
    agent::im::{AlertCard, CardAction, CardAnswer, NoticeKind},
    domain::notify::{
        buttons::{AlertAction, AlertButtonValue, AlertButtons},
        connector::{NotifyConnector, NotifyMessage},
    },
    shared::time::TimestampMicros,
};

fn callback(body: Value) -> Value {
    json!({ "cmd": "aibot_msg_callback", "headers": { "req_id": "req-1" }, "body": body })
}

fn group_text(content: &str) -> Value {
    callback(json!({
        "msgid": "msg-1",
        "aibotid": "aib1",
        "chatid": "wrkGroup",
        "chattype": "group",
        "from": { "userid": "zhangsan" },
        "msgtype": "text",
        "text": { "content": content },
    }))
}

#[test]
fn a_group_message_drops_the_mention_and_is_the_senders_own_thread() {
    let message = inbound(&group_text("@Mole Agent\u{2005}why is checkout slow?"), 7).unwrap();

    assert_eq!(message.kind, ConversationKind::Group);
    assert_eq!(message.text, "why is checkout slow?");
    assert_eq!(message.conversation_id, "wrkGroup");
    assert_eq!(message.thread_key, "user:zhangsan");
    assert_eq!(message.sender.ids, ["zhangsan"]);
    assert!(message.addressed);
    assert_eq!(
        message.reply,
        json!({ "req_id": "req-1", "chat_id": "wrkGroup", "chat_type": 2, "received_at": 7 })
    );

    let typed = inbound(&group_text("@Mole disk full?"), 7).unwrap();
    assert_eq!(typed.text, "disk full?");

    let mentioned_later = inbound(&group_text("is @Mole up?"), 7).unwrap();
    assert_eq!(mentioned_later.text, "is @Mole up?");
}

#[test]
fn a_direct_message_has_no_thread_and_voice_and_mixed_messages_read_as_text() {
    let voice = callback(json!({
        "msgid": "msg-2",
        "chattype": "single",
        "from": { "userid": "lisi" },
        "msgtype": "voice",
        "voice": { "content": " 查一下 5xx " },
    }));
    let direct = inbound(&voice, 1).unwrap();

    assert_eq!(direct.kind, ConversationKind::Direct);
    assert_eq!(direct.conversation_id, "lisi");
    assert_eq!(direct.thread_key, "");
    assert_eq!(direct.text, "查一下 5xx");
    assert_eq!(direct.reply["chat_type"], 1);

    let mixed = callback(json!({
        "msgid": "msg-3",
        "chattype": "single",
        "from": { "userid": "lisi" },
        "msgtype": "mixed",
        "mixed": { "msg_item": [
            { "msgtype": "text", "text": { "content": "this graph" } },
            { "msgtype": "image", "image": { "url": "https://example.com/i" } },
            { "msgtype": "text", "text": { "content": "looks odd" } },
        ] },
    }));
    assert_eq!(inbound(&mixed, 1).unwrap().text, "this graph\nlooks odd");

    let picture = callback(json!({
        "msgid": "msg-4",
        "chattype": "single",
        "from": { "userid": "lisi" },
        "msgtype": "image",
        "image": { "url": "https://example.com/i" },
    }));
    assert!(inbound(&picture, 1).is_none());
}

#[test]
fn a_working_stream_carries_its_start() {
    assert_eq!(working_stream("2abc@1700"), Some(("2abc", 1700)));
    assert_eq!(working_stream("no-start"), None);
}

/// 点了一张模板卡片上的按钮，`event` 是那一帧的 `body.event`。
fn press(event: Value) -> Value {
    json!({
        "cmd": "aibot_event_callback",
        "headers": { "req_id": "req-7" },
        "body": {
            "msgid": "msg-7",
            "aibotid": "aib1",
            "chatid": "wrkGroup",
            "chattype": "group",
            "from": { "userid": "zhangsan" },
            "msgtype": "event",
            "event": event,
        },
    })
}

fn acknowledge() -> String {
    serde_json::to_string(&AlertButtonValue {
        action: AlertAction::Acknowledge,
        alert_id: "inc-1".into(),
        event_type: "alert.triggered".into(),
        policy_id: None,
        alert_ids: Vec::new(),
    })
    .unwrap()
}

#[test]
fn a_press_on_an_alert_card_is_read_and_the_done_button_is_not() {
    let action = card_action(&press(json!({
        "eventtype": "template_card_event",
        "template_card_event": {
            "card_type": "button_interaction",
            "event_key": acknowledge(),
            "task_id": "molesignal_1",
        },
    })))
    .unwrap();

    assert_eq!(action.value.action, AlertAction::Acknowledge);
    assert_eq!(action.value.alert_id, "inc-1");
    assert_eq!(action.sender.ids, ["zhangsan"]);
    assert_eq!(action.conversation_id, "wrkGroup");
    assert_eq!(action.kind, ConversationKind::Group);
    assert_eq!(action.reply["chat_type"], 2);
    assert_eq!(
        action.card,
        json!({ "req_id": "req-7", "task_id": "molesignal_1" })
    );

    // 键也可能直接放在 `event` 里。
    assert!(
        card_action(&press(json!({
            "eventtype": "template_card_event",
            "event_key": acknowledge(),
            "task_id": "molesignal_1",
        })))
        .is_some()
    );

    // 处理完的告警留下的「✓」与别的事件都不算。
    assert!(
        card_action(&press(json!({
            "eventtype": "template_card_event",
            "template_card_event": { "event_key": "molesignal_done", "task_id": "molesignal_1" },
        })))
        .is_none()
    );
    assert!(card_action(&press(json!({ "eventtype": "enter_chat" }))).is_none());
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

async fn next_frame(
    server: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
) -> Option<Value> {
    while let Some(Ok(message)) = server.next().await {
        if let Message::Text(text) = message {
            return serde_json::from_str(&text).ok();
        }
    }

    None
}

/// 本机的企业微信：答应订阅，发一条消息，答复写来的每一帧（内容是 `refuse me` 的拒绝），收到
/// 五帧之后说连接被别人挤掉了。
async fn fake_wecom(socket: TcpListener, frames: Arc<Mutex<Vec<Value>>>) {
    let (tcp, _) = socket.accept().await.unwrap();
    let mut server = tokio_tungstenite::accept_async(tcp).await.unwrap();

    let subscribe = next_frame(&mut server).await.unwrap();
    assert_eq!(subscribe["cmd"], "aibot_subscribe");
    assert_eq!(
        subscribe["body"],
        json!({ "bot_id": "aib-listen", "secret": "s3" })
    );

    let answer = |frame: &Value, errcode: i64| {
        Message::Text(
            json!({
                "headers": { "req_id": frame["headers"]["req_id"] },
                "errcode": errcode,
                "errmsg": if errcode == 0 { "ok" } else { "invalid chatid" },
            })
            .to_string()
            .into(),
        )
    };

    server.send(answer(&subscribe, 0)).await.unwrap();
    server
        .send(Message::Text(
            group_text("@Mole status?").to_string().into(),
        ))
        .await
        .unwrap();

    while let Some(frame) = next_frame(&mut server).await {
        if frame["cmd"] == "ping" {
            server.send(answer(&frame, 0)).await.unwrap();
            continue;
        }

        let refused = frame["body"]["markdown"]["content"] == "refuse me";
        server
            .send(answer(&frame, if refused { 93_000 } else { 0 }))
            .await
            .unwrap();

        let count = {
            let mut frames = frames.lock().unwrap();
            frames.push(frame);
            frames.len()
        };

        if count == 5 {
            let kicked = json!({
                "cmd": "aibot_event_callback",
                "headers": { "req_id": "req-9" },
                "body": { "msgtype": "event", "event": { "eventtype": "disconnected_event" } },
            });
            server
                .send(Message::Text(kicked.to_string().into()))
                .await
                .unwrap();
        }
    }
}

#[tokio::test]
async fn replies_go_out_over_the_connection_that_holds_the_bot() {
    let socket = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = socket.local_addr().unwrap().port();
    let frames = Arc::new(Mutex::new(Vec::new()));
    tokio::spawn(fake_wecom(socket, frames.clone()));

    let config = json!({
        "botId": "aib-listen",
        "botSecret": "s3",
        "wsUrl": format!("ws://127.0.0.1:{port}"),
    });
    assert!(!holds(&config));

    let listener = Arc::new(Collect::default());
    let task = tokio::spawn({
        let (config, listener) = (config.clone(), listener.clone());
        async move { listen(&config, listener.as_ref(), &CancellationToken::new()).await }
    });

    let reply = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(message) = listener.messages.lock().unwrap().first() {
                return message.reply.clone();
            }

            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();

    assert!(holds(&config));
    assert_eq!(*listener.connected.lock().unwrap(), ["aib-listen"]);

    let working = react(&config, &reply, "Looking into it…").await.unwrap();
    finish(&config, &reply, Some(&working), "**Disk full** on db-1")
        .await
        .unwrap();

    let refused = self::reply(&config, &reply, "refuse me").await.unwrap_err();
    assert!(refused.to_string().contains("invalid chatid (93000)"));

    tell(&config, "zhangsan", "your link").await.unwrap();
    finish(&config, &reply, Some("old@0"), "late answer")
        .await
        .unwrap();

    let ended = task.await.unwrap().unwrap_err();
    assert!(ended.to_string().contains("took it over"));
    assert!(!holds(&config));

    let frames = frames.lock().unwrap();
    let stream = frames[0]["body"]["stream"]["id"].as_str().unwrap();

    assert_eq!(frames[0]["cmd"], "aibot_respond_msg");
    assert_eq!(frames[0]["headers"]["req_id"], "req-1");
    assert_eq!(frames[0]["body"]["stream"]["finish"], false);
    assert_eq!(frames[0]["body"]["stream"]["content"], "Looking into it…");

    assert_eq!(frames[1]["body"]["stream"]["id"], stream);
    assert_eq!(frames[1]["body"]["stream"]["finish"], true);
    assert_eq!(
        frames[1]["body"]["stream"]["content"],
        "**Disk full** on db-1"
    );

    assert_eq!(frames[3]["cmd"], "aibot_send_msg");
    assert_eq!(frames[3]["body"]["chatid"], "zhangsan");
    assert_eq!(frames[3]["body"]["chat_type"], 1);

    // 流过了时限：回答另发到群里。
    assert_eq!(frames[4]["cmd"], "aibot_send_msg");
    assert_eq!(frames[4]["body"]["chatid"], "wrkGroup");
    assert_eq!(frames[4]["body"]["chat_type"], 2);
    assert_eq!(frames[4]["body"]["markdown"]["content"], "late answer");
}

#[tokio::test]
async fn a_refused_subscription_says_why() {
    let socket = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = socket.local_addr().unwrap().port();

    tokio::spawn(async move {
        let (tcp, _) = socket.accept().await.unwrap();
        let mut server = tokio_tungstenite::accept_async(tcp).await.unwrap();
        let subscribe = next_frame(&mut server).await.unwrap();
        let refusal = json!({
            "headers": { "req_id": subscribe["headers"]["req_id"] },
            "errcode": 40_001,
            "errmsg": "invalid secret",
        });

        server
            .send(Message::Text(refusal.to_string().into()))
            .await
            .unwrap();
    });

    let config = json!({
        "botId": "aib-refused",
        "botSecret": "wrong",
        "wsUrl": format!("ws://127.0.0.1:{port}"),
    });
    let error = listen(&config, &Collect::default(), &CancellationToken::new())
        .await
        .unwrap_err();

    assert!(error.to_string().contains("invalid secret (40001)"));
    assert!(!holds(&config));
}

/// 本机的企业微信：答应订阅，推来一次按钮点击，答复并记下之后写来的每一帧。
async fn fake_wecom_press(socket: TcpListener, frames: Arc<Mutex<Vec<Value>>>) {
    let (tcp, _) = socket.accept().await.unwrap();
    let mut server = tokio_tungstenite::accept_async(tcp).await.unwrap();
    let ok = |frame: &Value| {
        Message::Text(
            json!({ "headers": { "req_id": frame["headers"]["req_id"] }, "errcode": 0 })
                .to_string()
                .into(),
        )
    };

    let subscribe = next_frame(&mut server).await.unwrap();
    server.send(ok(&subscribe)).await.unwrap();

    let pressed = press(json!({
        "eventtype": "template_card_event",
        "template_card_event": { "event_key": acknowledge(), "task_id": "molesignal_1" },
    }));
    server
        .send(Message::Text(pressed.to_string().into()))
        .await
        .unwrap();

    while let Some(frame) = next_frame(&mut server).await {
        server.send(ok(&frame)).await.unwrap();

        if frame["cmd"] != "ping" {
            frames.lock().unwrap().push(frame);
        }
    }
}

#[tokio::test]
async fn a_press_is_answered_on_its_frame_and_notifications_go_out_over_the_connection() {
    let socket = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = socket.local_addr().unwrap().port();
    let frames = Arc::new(Mutex::new(Vec::new()));
    tokio::spawn(fake_wecom_press(socket, frames.clone()));

    let config = json!({
        "botId": "aib-press",
        "botSecret": "s3",
        "wsUrl": format!("ws://127.0.0.1:{port}"),
    });
    let listener = Arc::new(Collect::default());
    let cancel = CancellationToken::new();
    let task = tokio::spawn({
        let (config, listener, cancel) = (config.clone(), listener.clone(), cancel.clone());
        async move { listen(&config, listener.as_ref(), &cancel).await }
    });

    let action = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(action) = listener.actions.lock().unwrap().first() {
                return action.clone();
            }

            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();

    let message = NotifyMessage {
        title: "RESOLVED · Disk full".into(),
        text: "Disk is back at 60%.".into(),
        markdown: None,
        html: None,
        metadata: Default::default(),
        provider_templates: Default::default(),
        event: None,
        card: None,
    };
    let answered = CardAnswer {
        notice: Some((NoticeKind::Info, "It was already resolved.".into())),
        card: Some(AlertCard {
            message: message.clone(),
            buttons: AlertButtons::default(),
            note: Some("Ada resolved it".into()),
        }),
    };
    answer(&config, &action, &answered).await.unwrap();

    let connector = NotifyConnector {
        id: Id::from_string("wecom-bot"),
        organization_id: Id::from_string("org-1"),
        name: "WeCom bot".into(),
        connector_type: "wecom_bot".into(),
        config: config.clone(),
        capabilities: Default::default(),
        enabled: true,
        status: Default::default(),
        last_tested_at: None,
        last_test_status: None,
        last_test_error: None,
        created_at: TimestampMicros(1),
        updated_at: TimestampMicros(1),
    };
    let body = json!({ "msgtype": "markdown", "markdown": { "content": "**Disk full**" } });
    deliver(&connector, "wrkGroup", 2, body, None)
        .await
        .unwrap();

    cancel.cancel();
    task.await.unwrap().unwrap();

    let frames = frames.lock().unwrap();

    // 卡片在点击那一帧上换掉，同一个 task_id；没有按钮可点时留一个「✓」。
    assert_eq!(frames[0]["cmd"], "aibot_respond_update_msg");
    assert_eq!(frames[0]["headers"]["req_id"], "req-7");
    assert_eq!(frames[0]["body"]["response_type"], "update_template_card");
    let card = &frames[0]["body"]["template_card"];
    assert_eq!(card["task_id"], "molesignal_1");
    assert_eq!(card["main_title"]["desc"], "Ada resolved it");
    assert_eq!(card["button_list"][0]["text"], "✓");

    // 没做成的那句话单聊告诉点的人。
    assert_eq!(frames[1]["cmd"], "aibot_send_msg");
    assert_eq!(frames[1]["body"]["chatid"], "zhangsan");
    assert_eq!(frames[1]["body"]["chat_type"], 1);
    assert_eq!(
        frames[1]["body"]["markdown"]["content"],
        "It was already resolved."
    );

    // 本节点连着机器人：通知直接发到群里。
    assert_eq!(frames[2]["cmd"], "aibot_send_msg");
    assert_eq!(frames[2]["body"]["chatid"], "wrkGroup");
    assert_eq!(frames[2]["body"]["chat_type"], 2);
    assert_eq!(frames[2]["body"]["markdown"]["content"], "**Disk full**");
}
