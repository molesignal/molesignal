// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_partial_json, header, method, path, query_param},
};

use super::*;
use crate::infra::notify::adapters::google_chat_test_key;

fn message_event(space_type: &str, text: &str, argument: Option<&str>) -> Value {
    let mut message = json!({
        "name": "spaces/AAA/messages/M1",
        "sender": {
            "name": "users/1001",
            "displayName": "Alice",
            "email": "alice@example.com",
            "type": "HUMAN",
        },
        "text": text,
        "thread": { "name": "spaces/AAA/threads/T1" },
    });

    if let Some(argument) = argument {
        message["argumentText"] = json!(argument);
    }

    json!({
        "type": "MESSAGE",
        "message": message,
        "space": { "name": "spaces/AAA", "spaceType": space_type },
    })
}

#[test]
fn a_mention_in_a_space_answers_in_its_thread() {
    let event = message_event(
        "SPACE",
        "@Mole why is checkout slow?",
        Some(" why is checkout slow?"),
    );
    let mention = inbound(&event).unwrap();

    assert_eq!(mention.kind, ConversationKind::Group);
    assert_eq!(mention.text, "why is checkout slow?");
    assert_eq!(mention.message_id, "spaces/AAA/messages/M1");
    assert_eq!(mention.conversation_id, "spaces/AAA");
    assert_eq!(mention.thread_key, "spaces/AAA/threads/T1");
    assert_eq!(
        mention.sender.ids,
        ["users/1001", "1001", "alice@example.com"]
    );
    assert_eq!(mention.sender.name.as_deref(), Some("Alice"));
    assert_eq!(
        mention.reply,
        json!({ "space": "spaces/AAA", "thread": "spaces/AAA/threads/T1", "direct": false })
    );
}

#[test]
fn a_group_chat_is_per_person_and_a_direct_message_has_no_thread() {
    let group = inbound(&message_event("GROUP_CHAT", "@Mole hi", Some("hi"))).unwrap();
    assert_eq!(group.thread_key, "user:users/1001");

    let direct = inbound(&message_event("DIRECT_MESSAGE", "disk full?", None)).unwrap();
    assert_eq!(direct.kind, ConversationKind::Direct);
    assert_eq!(direct.thread_key, "");
    assert_eq!(direct.text, "disk full?");
    assert_eq!(direct.reply["direct"], true);
}

#[test]
fn an_add_on_event_reads_the_same_and_bots_and_other_events_are_skipped() {
    let classic = message_event("SPACE", "@Mole status", Some("status"));
    let add_on = json!({
        "commonEventObject": { "hostApp": "CHAT" },
        "chat": {
            "user": classic["message"]["sender"],
            "messagePayload": { "message": classic["message"], "space": classic["space"] },
        },
    });
    assert_eq!(inbound(&add_on), inbound(&classic));

    let mut from_bot = classic.clone();
    from_bot["message"]["sender"]["type"] = json!("BOT");
    assert!(inbound(&from_bot).is_none());

    let added = json!({ "type": "ADDED_TO_SPACE", "space": { "name": "spaces/AAA" } });
    assert!(inbound(&added).is_none());
}

#[test]
fn a_pubsub_message_carries_the_event_as_base64_json() {
    let data = STANDARD.encode(br#"{"type":"MESSAGE"}"#);
    assert_eq!(
        event(&json!({ "data": data })),
        Some(json!({ "type": "MESSAGE" }))
    );
    assert_eq!(event(&json!({ "data": "%%%" })), None);
    assert_eq!(
        short_name("projects/ops/subscriptions/mole-chat"),
        "mole-chat"
    );
}

#[derive(Default)]
struct Collect {
    connected: Mutex<Vec<String>>,
    messages: Mutex<Vec<Inbound>>,
}

#[async_trait]
impl Listener for Collect {
    async fn connected(&self, detail: &str) {
        self.connected.lock().unwrap().push(detail.to_string());
    }

    async fn message(&self, inbound: Inbound) {
        self.messages.lock().unwrap().push(inbound);
    }
}

async fn google(api: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "access_token": "ya29.t", "expires_in": 3600 })),
        )
        .mount(api)
        .await;
}

fn config(api: &MockServer) -> Value {
    json!({
        "credentials": {
            "client_email": "mole@ops.iam.gserviceaccount.com",
            "private_key": google_chat_test_key(),
            "token_uri": format!("{}/token", api.uri()),
        },
        "subscription": "projects/ops/subscriptions/mole-chat",
        "pubsubBaseUrl": format!("{}/v1", api.uri()),
        "apiBaseUrl": format!("{}/chat", api.uri()),
    })
}

#[tokio::test]
async fn messages_are_pulled_from_the_subscription_and_acknowledged() {
    let api = MockServer::start().await;
    google(&api).await;

    Mock::given(method("GET"))
        .and(path("/v1/projects/ops/subscriptions/mole-chat"))
        .and(header("authorization", "Bearer ya29.t"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "name": "mole-chat" })))
        .mount(&api)
        .await;

    let event = message_event("SPACE", "@Mole status?", Some("status?"));
    Mock::given(method("POST"))
        .and(path("/v1/projects/ops/subscriptions/mole-chat:pull"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "receivedMessages": [{
                "ackId": "ack-1",
                "message": { "data": STANDARD.encode(event.to_string()), "messageId": "p1" },
            }],
        })))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&api)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/projects/ops/subscriptions/mole-chat:pull"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .with_priority(2)
        .mount(&api)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/projects/ops/subscriptions/mole-chat:acknowledge"))
        .and(body_partial_json(json!({ "ackIds": ["ack-1"] })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&api)
        .await;

    let listener = Arc::new(Collect::default());
    let cancel = CancellationToken::new();
    let task = tokio::spawn({
        let (config, listener, cancel) = (config(&api), listener.clone(), cancel.clone());
        async move { listen(&config, listener.as_ref(), &cancel).await }
    });

    tokio::time::timeout(Duration::from_secs(5), async {
        while listener.messages.lock().unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();

    cancel.cancel();
    task.await.unwrap().unwrap();

    assert_eq!(*listener.connected.lock().unwrap(), ["mole-chat"]);
    assert_eq!(listener.messages.lock().unwrap()[0].text, "status?");
}

#[tokio::test]
async fn the_working_message_becomes_the_answer_and_links_go_to_a_direct_message() {
    let api = MockServer::start().await;
    google(&api).await;

    Mock::given(method("POST"))
        .and(path("/chat/spaces/AAA/messages"))
        .and(query_param(
            "messageReplyOption",
            "REPLY_MESSAGE_FALLBACK_TO_NEW_THREAD",
        ))
        .and(body_partial_json(
            json!({ "thread": { "name": "spaces/AAA/threads/T1" } }),
        ))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "name": "spaces/AAA/messages/W1" })),
        )
        .mount(&api)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/chat/spaces/AAA/messages/W1"))
        .and(query_param("updateMask", "text"))
        .and(body_partial_json(
            json!({ "text": "*Disk full* on <https://ms.example/c|db-1>" }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&api)
        .await;
    Mock::given(method("GET"))
        .and(path("/chat/spaces:findDirectMessage"))
        .and(query_param("name", "users/1001"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "name": "spaces/DM1" })))
        .mount(&api)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat/spaces/DM1/messages"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "name": "spaces/DM1/messages/X" })),
        )
        .expect(1)
        .mount(&api)
        .await;

    let config = config(&api);
    let reply =
        json!({ "space": "spaces/AAA", "thread": "spaces/AAA/threads/T1", "direct": false });

    let working = react(&config, &reply, "Looking into it…").await.unwrap();
    assert_eq!(working, "spaces/AAA/messages/W1");

    finish(
        &config,
        &reply,
        Some(&working),
        "**Disk full** on [db-1](https://ms.example/c)",
    )
    .await
    .unwrap();

    tell(&config, "users/1001", "your link").await.unwrap();
}

fn alert_parameter() -> String {
    json!({
        "action": "acknowledge",
        "alert_id": "inc-1",
        "event_type": "alert.triggered",
    })
    .to_string()
}

fn clicked(function: &str) -> Value {
    json!({
        "type": "CARD_CLICKED",
        "user": {
            "name": "users/1001",
            "displayName": "Alice",
            "email": "alice@example.com",
            "type": "HUMAN",
        },
        "space": { "name": "spaces/AAA", "spaceType": "SPACE" },
        "message": {
            "name": "spaces/AAA/messages/M1",
            "thread": { "name": "spaces/AAA/threads/T1" },
        },
        "common": {
            "invokedFunction": function,
            "parameters": { GOOGLE_CHAT_ALERT_FUNCTION: alert_parameter() },
        },
    })
}

#[test]
fn a_press_of_an_alert_button_is_an_action_in_either_event_form() {
    let action = card_action(&clicked(GOOGLE_CHAT_ALERT_FUNCTION)).unwrap();

    assert_eq!(action.value.alert_id, "inc-1");
    assert_eq!(
        action.sender.ids,
        ["users/1001", "1001", "alice@example.com"]
    );
    assert_eq!(action.conversation_id, "spaces/AAA");
    assert_eq!(action.kind, ConversationKind::Group);
    assert_eq!(
        action.card,
        json!({ "message": "spaces/AAA/messages/M1", "user": "users/1001" })
    );
    assert_eq!(action.reply["thread"], "spaces/AAA/threads/T1");

    // 旧的字段名：`action.actionMethodName` 与参数列表。
    let mut legacy = clicked(GOOGLE_CHAT_ALERT_FUNCTION);
    legacy.as_object_mut().unwrap().remove("common");
    legacy["action"] = json!({
        "actionMethodName": GOOGLE_CHAT_ALERT_FUNCTION,
        "parameters": [{ "key": GOOGLE_CHAT_ALERT_FUNCTION, "value": alert_parameter() }],
    });
    assert_eq!(card_action(&legacy).unwrap().value, action.value);

    // 做成 Workspace 插件的。
    let add_on = json!({
        "commonEventObject": { "parameters": { GOOGLE_CHAT_ALERT_FUNCTION: alert_parameter() } },
        "chat": {
            "user": { "name": "users/1001", "displayName": "Alice" },
            "buttonClickedPayload": {
                "space": { "name": "spaces/DDD", "spaceType": "DIRECT_MESSAGE" },
                "message": { "name": "spaces/DDD/messages/M2" },
            },
        },
    });
    let direct = card_action(&add_on).unwrap();
    assert_eq!(direct.kind, ConversationKind::Direct);
    assert_eq!(direct.card["message"], "spaces/DDD/messages/M2");

    assert!(card_action(&clicked("approve")).is_none());
    assert!(card_action(&message_event("SPACE", "@Mole hi", None)).is_none());
}

#[tokio::test]
async fn a_pressed_card_is_updated_and_the_presser_is_told_privately() {
    use crate::{
        agent::im::AlertCard,
        domain::notify::{
            buttons::{AlertAction, AlertButtons},
            connector::NotifyMessage,
        },
    };

    let api = MockServer::start().await;
    google(&api).await;

    Mock::given(method("PATCH"))
        .and(path("/chat/spaces/AAA/messages/M1"))
        .and(query_param("updateMask", "cardsV2"))
        .and(header("authorization", "Bearer ya29.t"))
        .and(body_partial_json(
            json!({ "cardsV2": [{ "cardId": "molesignal" }] }),
        ))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "name": "spaces/AAA/messages/M1" })),
        )
        .expect(1)
        .mount(&api)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat/spaces/AAA/messages"))
        .and(body_partial_json(json!({
            "text": "Already resolved",
            "privateMessageViewer": { "name": "users/1001" },
            "thread": { "name": "spaces/AAA/threads/T1" },
        })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "name": "spaces/AAA/messages/P1" })),
        )
        .expect(1)
        .mount(&api)
        .await;

    let action = card_action(&clicked(GOOGLE_CHAT_ALERT_FUNCTION)).unwrap();
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
            actions: vec![(AlertAction::Resolve, "Resolve".into())],
            open: None,
        },
        note: None,
    };

    answer(
        &config(&api),
        &action,
        &CardAnswer {
            notice: Some((NoticeKind::Info, "Already resolved".into())),
            card: Some(card),
        },
    )
    .await
    .unwrap();
}
