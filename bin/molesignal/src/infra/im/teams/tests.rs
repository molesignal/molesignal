// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_partial_json, header, method, path},
};

use super::*;

const BOT: &str = "28:00000000-0000-0000-0000-00000000abcd";

fn activity(kind: &str, conversation: &str, text: &str) -> Value {
    json!({
        "type": "message",
        "id": "1700000000123",
        "channelId": "msteams",
        "serviceUrl": "https://smba.trafficmanager.net/emea/",
        "from": { "id": "29:alice", "aadObjectId": "aad-alice", "name": "Alice" },
        "recipient": { "id": BOT, "name": "Mole" },
        "conversation": { "id": conversation, "conversationType": kind, "tenantId": "t1" },
        "text": text,
        "entities": [
            { "type": "mention", "text": "<at>Mole</at>", "mentioned": { "id": BOT, "name": "Mole" } },
            { "type": "mention", "text": "<at>Bob</at>", "mentioned": { "id": "29:bob", "name": "Bob" } },
        ],
    })
}

#[test]
fn a_mention_in_a_channel_answers_under_its_post() {
    let post = activity(
        "channel",
        "19:ops@thread.tacv2;messageid=1700000000000",
        "<at>Mole</at> why did <at>Bob</at>&nbsp;get paged?<br>checkout &amp; cart",
    );
    let mention = inbound(&post).unwrap();

    assert_eq!(mention.kind, ConversationKind::Group);
    assert_eq!(mention.text, "why did @Bob get paged?\ncheckout & cart");
    assert_eq!(mention.conversation_id, "19:ops@thread.tacv2");
    assert_eq!(mention.thread_key, "1700000000000");
    assert_eq!(mention.message_id, "19:ops@thread.tacv2:1700000000123");
    assert_eq!(mention.sender.ids, ["29:alice", "aad-alice"]);
    assert_eq!(
        mention.reply,
        json!({
            "service_url": "https://smba.trafficmanager.net/emea/",
            "conversation": "19:ops@thread.tacv2;messageid=1700000000000",
            "activity_id": "1700000000123",
            "tenant": "t1",
        })
    );
}

#[test]
fn a_group_chat_is_per_person_and_a_personal_chat_has_no_thread() {
    let group = inbound(&activity(
        "groupChat",
        "19:group@thread.v2",
        "<at>Mole</at> hi",
    ))
    .unwrap();
    assert_eq!(group.thread_key, "user:29:alice");
    assert_eq!(group.text, "hi");

    let personal = inbound(&activity("personal", "a:1xyz", "disk full?")).unwrap();
    assert_eq!(personal.kind, ConversationKind::Direct);
    assert_eq!(personal.thread_key, "");
    assert_eq!(personal.conversation_id, "a:1xyz");

    let mut update = activity("personal", "a:1xyz", "x");
    update["type"] = json!("conversationUpdate");
    assert!(inbound(&update).is_none());

    let mut submit = activity("personal", "a:1xyz", "x");
    submit.as_object_mut().unwrap().remove("text");
    submit["value"] = json!({ "action": "approve" });
    assert!(inbound(&submit).is_none());
}

fn config(server: &MockServer) -> Value {
    json!({
        "appId": "00000000-0000-0000-0000-00000000abcd",
        "clientSecret": "s",
        "tenantId": "t1",
        "serviceUrl": server.uri(),
        "loginBaseUrl": server.uri(),
    })
}

#[tokio::test]
async fn the_working_reply_becomes_the_answer_and_links_go_to_a_personal_chat() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/t1/oauth2/v2.0/token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "access_token": "bf-token", "expires_in": 3600 })),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(
            "/v3/conversations/19%3Aops%40thread.tacv2%3Bmessageid%3D1700000000000/activities/1700000000123",
        ))
        .and(header("authorization", "Bearer bf-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "w1" })))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path(
            "/v3/conversations/19%3Aops%40thread.tacv2%3Bmessageid%3D1700000000000/activities/w1",
        ))
        .and(body_partial_json(
            json!({ "id": "w1", "text": "**Disk full**", "textFormat": "markdown" }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "w1" })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v3/conversations"))
        .and(body_partial_json(
            json!({ "members": [{ "id": "29:alice" }], "tenantId": "t1" }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "a:1alice" })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v3/conversations/a%3A1alice/activities"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "x1" })))
        .expect(1)
        .mount(&server)
        .await;

    let config = config(&server);
    let reply = json!({
        "service_url": server.uri(),
        "conversation": "19:ops@thread.tacv2;messageid=1700000000000",
        "activity_id": "1700000000123",
        "tenant": "t1",
    });

    let working = react(&config, &reply, "Looking into it…").await.unwrap();
    assert_eq!(working, "w1");

    finish(&config, &reply, Some(&working), "**Disk full**")
        .await
        .unwrap();
    tell(&config, &reply, "29:alice", "your link")
        .await
        .unwrap();
}

#[tokio::test]
async fn replies_never_go_to_a_service_url_outside_bot_framework() {
    let server = MockServer::start().await;
    let reply = json!({
        "service_url": "https://evil.example/teams/",
        "conversation": "a:1xyz",
        "activity_id": "1",
    });

    Mock::given(method("POST"))
        .and(path("/t1/oauth2/v2.0/token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "access_token": "bf-token-2", "expires_in": 3600 })),
        )
        .mount(&server)
        .await;

    let mut config = config(&server);
    config["clientSecret"] = json!("s2");

    let error = self::reply(&config, &reply, "hi").await.unwrap_err();
    assert!(error.to_string().contains("outside Bot Framework"));
}

fn press(kind: &str, data: Value) -> Value {
    let mut press = json!({
        "type": kind,
        "id": "1700000000999",
        "channelId": "msteams",
        "serviceUrl": "https://smba.trafficmanager.net/emea/",
        "from": { "id": "29:alice", "aadObjectId": "aad-alice", "name": "Alice" },
        "recipient": { "id": BOT, "name": "Mole" },
        "conversation": {
            "id": "19:ops@thread.tacv2;messageid=1700000000000",
            "conversationType": "channel",
            "tenantId": "t1",
        },
        "replyToId": "1700000000555",
    });

    if kind == "invoke" {
        press["name"] = json!("adaptiveCard/action");
        press["value"] = json!({
            "action": { "type": "Action.Execute", "verb": TEAMS_ALERT_VERB, "data": data },
            "trigger": "manual",
        });
    } else {
        press["value"] = data;
    }

    press
}

fn alert_value() -> Value {
    json!({
        TEAMS_ALERT_VALUE_KEY: {
            "action": "acknowledge",
            "alert_id": "inc-1",
            "event_type": "alert.triggered",
            "policy_id": "policy-1",
        }
    })
}

#[test]
fn a_card_press_is_an_action_and_so_is_an_old_clients_submit() {
    let action = card_action(&press("invoke", alert_value())).unwrap();

    assert_eq!(action.value.alert_id, "inc-1");
    assert_eq!(action.value.policy_id.as_deref(), Some("policy-1"));
    assert_eq!(action.sender.ids, ["29:alice", "aad-alice"]);
    assert_eq!(action.conversation_id, "19:ops@thread.tacv2");
    assert_eq!(action.kind, ConversationKind::Group);
    assert_eq!(
        action.card,
        json!({
            "service_url": "https://smba.trafficmanager.net/emea/",
            "conversation": "19:ops@thread.tacv2;messageid=1700000000000",
            "activity_id": "1700000000555",
        })
    );

    let submitted = card_action(&press("message", alert_value())).unwrap();
    assert_eq!(submitted.value, action.value);

    // 别的卡片的提交、别的 verb、人发的消息都不算。
    assert!(card_action(&press("message", json!({ "answer": "yes" }))).is_none());

    let mut other = press("invoke", alert_value());
    other["value"]["action"]["verb"] = json!("approve");
    assert!(card_action(&other).is_none());

    assert!(card_action(&activity("personal", "a:1xyz", "<at>Mole</at> hi")).is_none());
}

fn alert_card() -> crate::agent::im::AlertCard {
    use crate::domain::notify::{
        buttons::{AlertAction, AlertButtons},
        connector::NotifyMessage,
    };

    crate::agent::im::AlertCard {
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
        note: Some("Acknowledged by Alice".into()),
    }
}

#[test]
fn the_invoke_answer_shows_the_new_card_or_the_notice() {
    use crate::agent::im::NoticeKind;

    let card = invoke_response(Some(&CardAnswer {
        notice: Some((NoticeKind::Success, "Acknowledged".into())),
        card: Some(alert_card()),
    }));
    assert_eq!(card["statusCode"], 200);
    assert_eq!(card["type"], "application/vnd.microsoft.card.adaptive");
    assert_eq!(card["value"]["type"], "AdaptiveCard");

    let notice = invoke_response(Some(&CardAnswer {
        notice: Some((NoticeKind::Warning, "Link your account first".into())),
        card: None,
    }));
    assert_eq!(notice["type"], "application/vnd.microsoft.activity.message");
    assert_eq!(notice["value"], "Link your account first");

    // 还没有结果：什么都不换，卡片稍后由任务换上。
    assert_eq!(invoke_response(None), json!({ "statusCode": 200 }));
}

#[tokio::test]
async fn a_pressed_card_is_replaced_for_everyone_in_the_conversation() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/t1/oauth2/v2.0/token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "access_token": "bf-token", "expires_in": 3600 })),
        )
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path(
            "/v3/conversations/19%3Aops%40thread.tacv2%3Bmessageid%3D1700000000000/activities/1700000000555",
        ))
        .and(header("authorization", "Bearer bf-token"))
        .and(body_partial_json(json!({
            "id": "1700000000555",
            "attachments": [{ "contentType": "application/vnd.microsoft.card.adaptive" }],
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "1700000000555" })))
        .expect(1)
        .mount(&server)
        .await;

    let mut action = card_action(&press("invoke", alert_value())).unwrap();
    action.card["service_url"] = json!(server.uri());

    answer(
        &config(&server),
        &action,
        &CardAnswer {
            notice: None,
            card: Some(alert_card()),
        },
    )
    .await
    .unwrap();

    // 只有一句话、没有卡片时不改消息。
    answer(
        &config(&server),
        &action,
        &CardAnswer {
            notice: Some((
                crate::agent::im::NoticeKind::Info,
                "Already resolved".into(),
            )),
            card: None,
        },
    )
    .await
    .unwrap();
}
