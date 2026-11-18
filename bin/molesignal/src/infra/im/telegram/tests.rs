// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::Mutex as StdMutex;

use agent::im::{Inbound, Sender};
use async_trait::async_trait;
use domain::notify::{
    buttons::{AlertAction, AlertButtonValue, AlertButtons},
    connector::NotifyMessage,
};
use wiremock::{
    Mock, MockServer, Request, ResponseTemplate,
    matchers::{body_partial_json, method, path},
};

use super::{
    updates::{card_action, inbound, without_mention},
    *,
};

const TOKEN: &str = "777000:TEST-token_x";

fn me() -> Me {
    Me {
        id: 777_000,
        username: "MoleBot".into(),
    }
}

fn config(server: &MockServer) -> Value {
    json!({ "botToken": TOKEN, "apiBaseUrl": server.uri() })
}

fn at(name: &str) -> String {
    format!("/bot{TOKEN}/{name}")
}

fn ok(result: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({ "ok": true, "result": result }))
}

fn refused(code: u16, description: &str) -> ResponseTemplate {
    ResponseTemplate::new(code).set_body_json(json!({
        "ok": false,
        "error_code": code,
        "description": description,
    }))
}

/// The bodies of the calls to one method, in order.
fn bodies(requests: &[Request], name: &str) -> Vec<Value> {
    let suffix = format!("/{name}");

    requests
        .iter()
        .filter(|request| request.url.path().ends_with(&suffix))
        .map(|request| serde_json::from_slice(&request.body).unwrap())
        .collect()
}

fn alice() -> Value {
    json!({
        "id": 42,
        "is_bot": false,
        "first_name": "Alice",
        "last_name": "Liddell",
        "username": "alice",
    })
}

fn private() -> Value {
    json!({ "id": 42, "type": "private", "first_name": "Alice" })
}

fn group() -> Value {
    json!({ "id": -100_123, "type": "supergroup", "title": "Ops" })
}

fn message(chat: Value, text: &str) -> Value {
    json!({ "message_id": 7, "from": alice(), "chat": chat, "date": 1_700_000_000, "text": text })
}

fn acknowledge() -> AlertButtonValue {
    AlertButtonValue {
        action: AlertAction::Acknowledge,
        alert_id: "inc-1".into(),
        event_type: "alert.triggered".into(),
        policy_id: Some("policy-1".into()),
        alert_ids: Vec::new(),
    }
}

fn press(data: &str, chat: Value) -> Value {
    json!({
        "id": "cbq-1",
        "from": alice(),
        "chat_instance": "-9",
        "data": data,
        "message": {
            "message_id": 9,
            "date": 1_700_000_000,
            "chat": chat,
            "from": { "id": 777_000, "is_bot": true, "first_name": "Mole", "username": "MoleBot" },
            "text": "CRITICAL · API unavailable",
        },
    })
}

fn pressed() -> CardAction {
    let data = callback::encode(&acknowledge()).unwrap();
    card_action(&press(&data, group()), &me()).unwrap()
}

#[test]
fn a_private_message_is_for_the_bot() {
    let asked = inbound(&message(private(), "  why is checkout slow? "), &me()).unwrap();

    assert_eq!(asked.kind, ConversationKind::Direct);
    assert!(asked.addressed);
    assert_eq!(asked.text, "why is checkout slow?");
    assert_eq!(asked.thread_key, "");
    assert_eq!(asked.message_id, "42:7");
    assert_eq!(asked.conversation_id, "42");
    assert_eq!(
        asked.sender,
        Sender {
            ids: vec!["42".into(), "@alice".into()],
            name: Some("Alice Liddell".into()),
        }
    );
    assert_eq!(asked.reply, json!({ "chat_id": 42, "message_id": 7 }));

    // The first message of a private chat, and what a link into it sends.
    let started = inbound(&message(private(), "/start link"), &me()).unwrap();
    assert_eq!(started.text, "/help");
}

#[test]
fn in_a_group_only_mentions_commands_and_replies_to_answers_are_for_the_bot() {
    let me = me();

    let mention = inbound(&message(group(), "@molebot why is checkout slow?"), &me).unwrap();
    assert_eq!(mention.kind, ConversationKind::Group);
    assert_eq!(mention.text, "why is checkout slow?");
    assert_eq!(mention.thread_key, "user:42");
    assert_eq!(mention.message_id, "-100123:7");
    assert_eq!(mention.conversation_id, "-100123");

    let text = |text: &str| inbound(&message(group(), text), &me).map(|asked| asked.text);

    assert_eq!(text("lunch?"), None);
    assert_eq!(text("/var/log is full"), None);
    assert_eq!(text("/help@OtherBot"), None);
    assert_eq!(text("/help@MoleBot").as_deref(), Some("/help"));
    assert_eq!(text("/scope payments").as_deref(), Some("/scope payments"));
    assert_eq!(text("/start@MoleBot").as_deref(), Some("/help"));

    let mut follow_up = message(group(), "and the database?");
    follow_up["reply_to_message"] = json!({
        "message_id": 6,
        "from": { "id": 777_000, "is_bot": true, "first_name": "Mole" },
        "chat": group(),
        "text": "The connection pool is full.",
    });
    assert_eq!(
        inbound(&follow_up, &me).map(|asked| asked.text).as_deref(),
        Some("and the database?")
    );

    // A notification has buttons: a reply to it is about the alert, unless it mentions the bot.
    follow_up["reply_to_message"]["reply_markup"] = json!({
        "inline_keyboard": [[{ "text": "Acknowledge", "callback_data": "a:t:inc-1" }]],
    });
    assert!(inbound(&follow_up, &me).is_none());

    follow_up["text"] = json!("@MoleBot what does this mean?");
    assert_eq!(
        inbound(&follow_up, &me).map(|asked| asked.text).as_deref(),
        Some("what does this mean?")
    );
}

#[test]
fn topics_have_threads_of_their_own_and_bots_are_not_heard() {
    let me = me();

    let mut topic = message(group(), "@MoleBot status?");
    topic["is_topic_message"] = json!(true);
    topic["message_thread_id"] = json!(5);

    let asked = inbound(&topic, &me).unwrap();
    assert_eq!(asked.thread_key, "topic:5:user:42");
    assert_eq!(
        asked.reply,
        json!({ "chat_id": -100_123, "message_id": 7, "thread_id": 5 })
    );

    // A reply in a supergroup carries a thread ID too, without being in a topic.
    let mut threaded = message(group(), "@MoleBot status?");
    threaded["message_thread_id"] = json!(3);
    assert_eq!(inbound(&threaded, &me).unwrap().thread_key, "user:42");

    let mut bot = message(private(), "hi");
    bot["from"]["is_bot"] = json!(true);
    assert!(inbound(&bot, &me).is_none());

    let channel = message(json!({ "id": -100_999, "type": "channel" }), "@MoleBot hi");
    assert!(inbound(&channel, &me).is_none());

    let mut sticker = message(private(), "");
    sticker.as_object_mut().unwrap().remove("text");
    assert!(inbound(&sticker, &me).is_none());

    let mut photo = message(group(), "");
    photo.as_object_mut().unwrap().remove("text");
    photo["caption"] = json!("@MoleBot is this graph bad?");
    assert_eq!(inbound(&photo, &me).unwrap().text, "is this graph bad?");

    let mut ephemeral = message(group(), "/help@MoleBot");
    ephemeral["message_id"] = json!(0);
    ephemeral["ephemeral_message_id"] = json!(3);
    assert!(inbound(&ephemeral, &me).is_none());
}

#[test]
fn only_the_bots_own_mention_is_taken_out() {
    let stripped = |text: &str| without_mention(text, "MoleBot");

    assert_eq!(
        stripped("hey @MoleBot what now"),
        ("hey what now".into(), true)
    );
    assert_eq!(stripped("@molebot\nline two"), ("line two".into(), true));
    assert_eq!(
        stripped("mail ops@molebot.example"),
        ("mail ops@molebot.example".into(), false)
    );
    assert_eq!(
        stripped("ask @MoleBot_two"),
        ("ask @MoleBot_two".into(), false)
    );
    assert_eq!(without_mention(" x ", ""), ("x".into(), false));
}

#[test]
fn a_pressed_alert_button_reads_as_a_card_action() {
    let data = callback::encode(&acknowledge()).unwrap();
    let action = card_action(&press(&data, group()), &me()).unwrap();

    assert_eq!(action.value, acknowledge());
    assert_eq!(action.sender.ids, ["42", "@alice"]);
    assert_eq!(action.kind, ConversationKind::Group);
    assert_eq!(action.conversation_id, "-100123");
    assert_eq!(
        action.reply,
        json!({ "chat_id": -100_123, "message_id": 9 })
    );
    assert_eq!(
        action.card,
        json!({
            "callback_query_id": "cbq-1",
            "chat_id": -100_123,
            "message_id": 9,
            "bot": "MoleBot",
        })
    );

    let direct = card_action(&press(&data, private()), &me()).unwrap();
    assert_eq!(direct.kind, ConversationKind::Direct);

    // An old card is out of the bot's reach, but still says where it is.
    let mut old = press(&data, group());
    old["message"] = json!({ "message_id": 9, "date": 0, "chat": group() });
    assert!(card_action(&old, &me()).is_some());

    assert!(card_action(&press("something-else", group()), &me()).is_none());

    let mut inline = press(&data, group());
    inline.as_object_mut().unwrap().remove("message");
    inline["inline_message_id"] = json!("im-1");
    assert!(card_action(&inline, &me()).is_none());
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
            alert_id: "inc-1".into(),
            event_type: "alert.triggered".into(),
            actions: vec![(AlertAction::Resolve, "Resolve".into())],
            ..AlertButtons::default()
        },
        note: Some("Alice acknowledged this alert.".into()),
    }
}

#[tokio::test]
async fn an_answer_rewrites_the_card_and_tells_the_presser() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(at("editMessageText")))
        .respond_with(ok(json!({ "message_id": 9 })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(at("answerCallbackQuery")))
        .respond_with(ok(json!(true)))
        .mount(&server)
        .await;

    answer(
        &config(&server),
        &pressed(),
        &CardAnswer {
            notice: Some((NoticeKind::Success, "**Acknowledged.**".into())),
            card: Some(acknowledged_card()),
        },
    )
    .await
    .unwrap();

    let requests = server.received_requests().await.unwrap();
    let edit = &bodies(&requests, "editMessageText")[0];

    assert_eq!(edit["chat_id"], -100_123);
    assert_eq!(edit["message_id"], 9);
    assert_eq!(edit["parse_mode"], "HTML");
    assert_eq!(
        edit["text"],
        "<b>CRITICAL · API unavailable</b>\nAlert API unavailable is acknowledged.\n\n\
         <i>Alice acknowledged this alert.</i>"
    );

    let resolve = edit["reply_markup"]["inline_keyboard"][0][0]["callback_data"]
        .as_str()
        .and_then(callback::decode)
        .unwrap();
    assert_eq!(resolve.action, AlertAction::Resolve);

    assert_eq!(
        bodies(&requests, "answerCallbackQuery"),
        [json!({ "callback_query_id": "cbq-1", "text": "Acknowledged." })]
    );
}

#[tokio::test]
async fn a_refusal_is_an_alert_and_an_unchanged_card_is_fine() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(at("editMessageText")))
        .respond_with(refused(
            400,
            "Bad Request: message is not modified: specified new message content and reply \
             markup are exactly the same as a current content and reply markup of the message",
        ))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(at("answerCallbackQuery")))
        .respond_with(ok(json!(true)))
        .mount(&server)
        .await;

    let warning = CardAnswer {
        notice: Some((
            NoticeKind::Warning,
            "Your MoleSignal account cannot acknowledge or resolve this alert.".into(),
        )),
        card: None,
    };

    answer(&config(&server), &pressed(), &warning)
        .await
        .unwrap();

    let requests = server.received_requests().await.unwrap();
    let toast = &bodies(&requests, "answerCallbackQuery")[0];

    assert!(bodies(&requests, "editMessageText").is_empty());
    assert_eq!(toast["show_alert"], true);
    assert!(toast.get("url").is_none());

    let unchanged = CardAnswer {
        notice: None,
        card: Some(acknowledged_card()),
    };

    answer(&config(&server), &pressed(), &unchanged)
        .await
        .unwrap();
}

#[tokio::test]
async fn someone_the_bot_cannot_reach_is_taken_to_its_private_chat() {
    let token = "888000:TEST-token_y";
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/bot{token}/answerCallbackQuery")))
        .respond_with(ok(json!(true)))
        .mount(&server)
        .await;

    remember_unreached(888_000, 42);

    let refusal = CardAnswer {
        notice: Some((
            NoticeKind::Warning,
            "Send me a direct message, and I will send you a link there.".into(),
        )),
        card: None,
    };

    answer(
        &json!({ "botToken": token, "apiBaseUrl": server.uri() }),
        &pressed(),
        &refusal,
    )
    .await
    .unwrap();

    let requests = server.received_requests().await.unwrap();
    let toast = &bodies(&requests, "answerCallbackQuery")[0];

    assert_eq!(toast["url"], "https://t.me/MoleBot?start=link");
    assert_eq!(toast["show_alert"], false);
}

#[tokio::test]
async fn telling_someone_who_never_started_the_bot_whispers_in_the_group() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(at("sendMessage")))
        .and(body_partial_json(json!({ "chat_id": 42 })))
        .respond_with(refused(
            403,
            "Forbidden: bot can't initiate conversation with a user",
        ))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(at("sendMessage")))
        .and(body_partial_json(json!({ "chat_id": -100_123 })))
        .respond_with(ok(json!({
            "message_id": 0,
            "ephemeral_message_id": 3,
            "receiver_user": alice(),
            "chat": group(),
            "date": 1_700_000_000,
            "text": "Open this link",
        })))
        .mount(&server)
        .await;

    let reply = json!({ "chat_id": -100_123, "message_id": 7, "thread_id": 5 });

    tell(
        &config(&server),
        &reply,
        "42",
        "Open this link: https://ms.example/agent/im/link?token=t",
    )
    .await
    .unwrap();

    let requests = server.received_requests().await.unwrap();
    let sent = bodies(&requests, "sendMessage");

    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0]["chat_id"], 42);
    assert_eq!(
        sent[1]["ephemeral_message_parameters"],
        json!({ "receiver_user_id": 42 })
    );
    assert_eq!(sent[1]["message_thread_id"], 5);
    assert!(sent[1].get("reply_parameters").is_none());
}

#[tokio::test]
async fn a_whisper_shown_to_the_whole_group_is_taken_back() {
    let token = "999000:TEST-token_z";
    let server = MockServer::start().await;
    let at = |name: &str| format!("/bot{token}/{name}");

    Mock::given(method("POST"))
        .and(path(at("sendMessage")))
        .and(body_partial_json(json!({ "chat_id": 42 })))
        .respond_with(refused(403, "Forbidden: bot was blocked by the user"))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(at("sendMessage")))
        .and(body_partial_json(json!({ "chat_id": -100_123 })))
        .respond_with(ok(json!({
            "message_id": 50,
            "chat": group(),
            "date": 1_700_000_000,
            "text": "the link",
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(at("deleteMessage")))
        .respond_with(ok(json!(true)))
        .mount(&server)
        .await;

    let config = json!({ "botToken": token, "apiBaseUrl": server.uri() });
    let reply = json!({ "chat_id": -100_123, "message_id": 7 });

    let error = tell(&config, &reply, "42", "the link").await.unwrap_err();
    assert!(
        error.to_string().contains("has not started a private chat"),
        "{error}"
    );

    let requests = server.received_requests().await.unwrap();
    assert_eq!(
        bodies(&requests, "deleteMessage"),
        [json!({ "chat_id": -100_123, "message_id": 50 })]
    );
    assert!(unreached(999_000, 42));
}

#[tokio::test]
async fn a_long_answer_is_sent_in_parts_replying_to_the_question() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(at("sendMessage")))
        .respond_with(ok(json!({ "message_id": 100 })))
        .mount(&server)
        .await;

    let markdown = (0..60)
        .map(|index| format!("{index}. {}", "step ".repeat(20)))
        .collect::<Vec<_>>()
        .join("\n");
    let place = json!({ "chat_id": -100_123, "message_id": 7, "thread_id": 5 });

    reply(&config(&server), &place, &markdown).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    let sent = bodies(&requests, "sendMessage");

    assert_eq!(sent.len(), 2);

    for body in &sent {
        assert_eq!(body["chat_id"], -100_123);
        assert_eq!(body["reply_parameters"]["message_id"], 7);
        assert_eq!(body["message_thread_id"], 5);
        assert_eq!(body["parse_mode"], "HTML");
        assert!(html::visible_len(body["text"].as_str().unwrap()) <= html::MAX_TEXT);
    }

    assert!(reply(&config(&server), &json!({}), "hi").await.is_err());
}

#[tokio::test]
async fn a_short_rate_limit_is_waited_out_once() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(at("sendMessage")))
        .respond_with(ResponseTemplate::new(429).set_body_json(json!({
            "ok": false,
            "error_code": 429,
            "description": "Too Many Requests: retry after 1",
            "parameters": { "retry_after": 1 },
        })))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(at("sendMessage")))
        .respond_with(ok(json!({ "message_id": 100 })))
        .with_priority(2)
        .mount(&server)
        .await;

    reply(&config(&server), &json!({ "chat_id": 42 }), "part")
        .await
        .unwrap();

    let requests = server.received_requests().await.unwrap();
    assert_eq!(bodies(&requests, "sendMessage").len(), 2);
}

#[tokio::test]
async fn html_telegram_cannot_read_is_sent_plain() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(at("sendMessage")))
        .and(body_partial_json(json!({ "parse_mode": "HTML" })))
        .respond_with(refused(
            400,
            "Bad Request: can't parse entities: unclosed tag",
        ))
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(at("sendMessage")))
        .respond_with(ok(json!({ "message_id": 100 })))
        .with_priority(2)
        .mount(&server)
        .await;

    reply(&config(&server), &json!({ "chat_id": 42 }), "**a** & <b>")
        .await
        .unwrap();

    let requests = server.received_requests().await.unwrap();
    let sent = bodies(&requests, "sendMessage");

    assert_eq!(sent[1]["text"], "a & <b>");
    assert!(sent[1].get("parse_mode").is_none());
}

#[tokio::test]
async fn working_is_eyes_on_the_question() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(at("setMessageReaction")))
        .respond_with(ok(json!(true)))
        .mount(&server)
        .await;

    let place = json!({ "chat_id": -100_123, "message_id": 7 });

    assert_eq!(react(&config(&server), &place).await.unwrap(), "👀");
    unreact(&config(&server), &place, "👀").await.unwrap();

    let requests = server.received_requests().await.unwrap();
    let reactions = bodies(&requests, "setMessageReaction");

    assert_eq!(
        reactions,
        [
            json!({
                "chat_id": -100_123,
                "message_id": 7,
                "reaction": [{ "type": "emoji", "emoji": "👀" }],
            }),
            json!({ "chat_id": -100_123, "message_id": 7, "reaction": [] }),
        ]
    );
}

#[derive(Default)]
struct Collect {
    connected: StdMutex<Vec<String>>,
    messages: StdMutex<Vec<Inbound>>,
    actions: StdMutex<Vec<CardAction>>,
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

async fn mount_identity(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path(at("getMe")))
        .respond_with(ok(json!({
            "id": 777_000,
            "is_bot": true,
            "first_name": "Mole",
            "username": "MoleBot",
        })))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path(at("deleteWebhook")))
        .and(body_partial_json(json!({ "drop_pending_updates": false })))
        .respond_with(ok(json!(true)))
        .mount(server)
        .await;
}

#[tokio::test]
async fn polling_hands_over_messages_and_presses_until_telegram_refuses() {
    let server = MockServer::start().await;
    mount_identity(&server).await;

    let data = callback::encode(&acknowledge()).unwrap();
    let foreign = json!({ "id": "cbq-2", "from": alice(), "chat_instance": "-9", "data": "other" });

    Mock::given(method("POST"))
        .and(path(at("getUpdates")))
        .respond_with(ok(json!([
            { "update_id": 10, "message": message(private(), "hello") },
            { "update_id": 11, "callback_query": press(&data, group()) },
            { "update_id": 12, "callback_query": foreign },
            { "update_id": 13, "message": message(group(), "chatter") },
        ])))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(at("getUpdates")))
        .and(body_partial_json(json!({ "offset": 14 })))
        .respond_with(refused(
            409,
            "Conflict: terminated by other getUpdates request; make sure that only one bot \
             instance is running",
        ))
        .with_priority(2)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(at("answerCallbackQuery")))
        .respond_with(ok(json!(true)))
        .mount(&server)
        .await;

    let listener = Collect::default();
    let error = listen(&config(&server), &listener, &CancellationToken::new())
        .await
        .unwrap_err();

    assert!(error.to_string().contains("Conflict"), "{error}");
    assert_eq!(*listener.connected.lock().unwrap(), ["@MoleBot"]);

    // Read the requests first, so no lock is held across an await.
    let requests = server.received_requests().await.unwrap();
    let polls = bodies(&requests, "getUpdates");

    let messages = listener.messages.lock().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].text, "hello");

    let actions = listener.actions.lock().unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].value, acknowledge());

    assert_eq!(polls[0]["timeout"], 30);
    assert_eq!(
        polls[0]["allowed_updates"],
        json!(["message", "callback_query"])
    );
    assert!(polls[0].get("offset").is_none());
    assert_eq!(
        bodies(&requests, "answerCallbackQuery"),
        [json!({ "callback_query_id": "cbq-2" })]
    );
}

#[tokio::test]
async fn polling_stops_when_cancelled() {
    let server = MockServer::start().await;
    mount_identity(&server).await;
    Mock::given(method("POST"))
        .and(path(at("getUpdates")))
        .respond_with(ok(json!([])).set_delay(Duration::from_secs(10)))
        .mount(&server)
        .await;

    let cancel = CancellationToken::new();
    let stop = cancel.clone();

    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        stop.cancel();
    });

    listen(&config(&server), &Collect::default(), &cancel)
        .await
        .unwrap();
}

#[tokio::test]
async fn a_refused_token_stops_the_bot_with_the_reason() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(at("getMe")))
        .respond_with(refused(401, "Unauthorized"))
        .mount(&server)
        .await;

    let error = listen(
        &config(&server),
        &Collect::default(),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();

    assert!(matches!(error, Error::InvalidArgument(_)));
    assert!(error.to_string().contains("check the bot token"), "{error}");
    assert!(!error.to_string().contains(TOKEN));
}
