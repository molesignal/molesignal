// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use domain::notify::{
    buttons::{AlertAction, AlertButtons},
    template::{NotifyCard, NotifyCardColor, NotifyCardField, NotifyCardLink},
};
use serde_json::json;
use wiremock::{
    Mock, MockServer, Request, ResponseTemplate,
    matchers::{body_json, header, method, path, path_regex, query_param},
};

use super::*;

const OPEN_URL: &str = "https://molesignal.example.com/alerts/incidents/inc-1";
const ACK_URL: &str = "https://molesignal.example.com/api/v1/public/notify/actions/t1";
const API: &str = "/_matrix/client/v3";

fn config(server: &MockServer) -> Value {
    json!({ "homeserverUrl": server.uri(), "accessToken": "syt_token" })
}

fn target(target_type: NotifyTargetType, value: &str) -> NotifyTarget {
    NotifyTarget {
        target_type,
        value: value.into(),
        metadata: BTreeMap::new(),
    }
}

fn message(card: Option<NotifyCard>) -> NotifyMessage {
    NotifyMessage {
        title: "CRITICAL · Disk full".into(),
        text: "Disk is at 97%.".into(),
        markdown: Some("**db-1** is at 97%. @room".into()),
        html: None,
        metadata: BTreeMap::from([("severity".to_string(), "critical".to_string())]),
        provider_templates: Default::default(),
        event: None,
        card,
    }
}

/// The alert's buttons as the engine writes them; only their Open link is drawn here.
fn with_open_link(mut message: NotifyMessage) -> NotifyMessage {
    AlertButtons {
        alert_id: "inc-1".into(),
        event_type: "alert.triggered".into(),
        policy_id: None,
        alert_ids: Vec::new(),
        actions: vec![(AlertAction::Acknowledge, "Acknowledge".into())],
        open: Some(("Open in MoleSignal".into(), OPEN_URL.into())),
        callback: None,
    }
    .write(&mut message);

    message
}

fn card() -> NotifyCard {
    NotifyCard {
        title: "Disk <full> on db-1".into(),
        color: NotifyCardColor::Orange,
        fields: vec![
            NotifyCardField {
                label: "Host".into(),
                value: "db-1".into(),
            },
            NotifyCardField {
                label: "Usage <%>".into(),
                value: "97% & rising".into(),
            },
        ],
        links: vec![
            NotifyCardLink {
                label: "Runbook".into(),
                url: "https://runbooks.example.com/disk?a=\"b\"".into(),
            },
            NotifyCardLink {
                label: "Acknowledge".into(),
                url: ACK_URL.into(),
            },
        ],
        note: "Sent by MoleSignal".into(),
    }
}

/// `PUT …/rooms/<room>/send/m.room.message/<txn>` for an encoded room ID.
fn send_path(room: &str) -> String {
    format!("^{API}/rooms/{room}/send/m\\.room\\.message/molesignal-[0-9a-f]{{32}}$")
}

fn sent(event_id: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({ "event_id": event_id }))
}

fn refused(status: u16, errcode: &str, error: &str) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_json(json!({ "errcode": errcode, "error": error }))
}

async fn whoami(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path(format!("{API}/account/whoami")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "user_id": "@bot:example.org" })),
        )
        .mount(server)
        .await;
}

async fn requests(server: &MockServer, verb: &str) -> Vec<Request> {
    server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|request| request.method.as_str() == verb)
        .collect()
}

#[test]
fn configs_are_checked() {
    let adapter = MatrixBotConnectorAdapter::new();
    let config = |url: &str| json!({ "homeserverUrl": url, "accessToken": "syt_token" });

    adapter
        .validate_config(&config("https://matrix.example.org"))
        .unwrap();
    adapter
        .validate_config(&config("https://example.org/matrix/"))
        .unwrap();

    for url in [
        "",
        "matrix.example.org",
        "http://matrix.example.org",
        "https://matrix.example.org/?user=bot",
    ] {
        let error = adapter.validate_config(&config(url)).unwrap_err();
        assert!(
            error.to_string().contains("homeserverUrl"),
            "{url}: {error}"
        );
    }

    assert!(
        adapter
            .validate_config(&json!({ "homeserverUrl": "https://matrix.example.org" }))
            .is_err()
    );
    assert!(
        adapter
            .validate_config(&json!({
                "homeserverUrl": "https://matrix.example.org",
                "accessToken": "syt_token",
                "timeoutSecs": 0,
            }))
            .is_err()
    );
    assert!(adapter.addresses().is_empty());
}

#[test]
fn targets_are_rooms_aliases_or_people() {
    let of = |target_type, value: &str| Destination::of(&target(target_type, value));

    assert_eq!(
        of(NotifyTargetType::FixedGroup, " !ops:example.org ").unwrap(),
        Destination::Room("!ops:example.org".into())
    );
    assert_eq!(
        of(
            NotifyTargetType::FixedGroup,
            "!31hneApxJ_1o-63DmFrpeqnkFfWppnzWso1JvH3ogLM"
        )
        .unwrap(),
        Destination::Room("!31hneApxJ_1o-63DmFrpeqnkFfWppnzWso1JvH3ogLM".into())
    );
    assert_eq!(
        of(NotifyTargetType::FixedGroup, "#ops:example.org").unwrap(),
        Destination::Alias("#ops:example.org".into())
    );
    assert_eq!(
        of(NotifyTargetType::DirectUser, "@alice:example.org").unwrap(),
        Destination::Person("@alice:example.org".into())
    );

    for (target_type, value) in [
        (NotifyTargetType::FixedGroup, "!"),
        (NotifyTargetType::FixedGroup, "#ops"),
        (NotifyTargetType::FixedGroup, "ops room"),
        (NotifyTargetType::FixedGroup, "@alice:example.org"),
        (NotifyTargetType::DirectUser, "alice"),
        (NotifyTargetType::DirectUser, "@alice"),
        (NotifyTargetType::DirectUser, "!ops:example.org"),
        (NotifyTargetType::FixedAddress, "@alice:example.org"),
        (NotifyTargetType::Webhook, "https://matrix.example.org"),
    ] {
        assert!(
            MatrixBotConnectorAdapter::new()
                .validate_target(&target(target_type, value))
                .is_err(),
            "{value}"
        );
    }
}

#[tokio::test]
async fn an_alias_is_looked_up_and_joined_when_the_bot_is_not_in_the_room() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(format!("{API}/directory/room/%23ops%3Aexample.org")))
        .and(header("authorization", "Bearer syt_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "room_id": "!ops:example.org",
            "servers": ["example.org"],
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("PUT"))
        .and(path_regex(send_path("%21ops%3Aexample.org")))
        .respond_with(refused(
            403,
            "M_FORBIDDEN",
            "User @bot:example.org not in room !ops:example.org",
        ))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;

    Mock::given(method("PUT"))
        .and(path_regex(send_path("%21ops%3Aexample.org")))
        .respond_with(sent("$e1"))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path(format!("{API}/join/%23ops%3Aexample.org")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "room_id": "!ops:example.org" })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let result = MatrixBotConnectorAdapter::new()
        .send(
            &config(&server),
            &target(NotifyTargetType::FixedGroup, "#ops:example.org"),
            &message(None),
        )
        .await
        .unwrap();

    assert_eq!(result.provider_message_id.as_deref(), Some("$e1"));

    // The same transaction both times, so the homeserver keeps one message.
    let posts = requests(&server, "PUT").await;
    assert_eq!(posts.len(), 2);
    assert_eq!(posts[0].url.path(), posts[1].url.path());

    let content: Value = serde_json::from_slice(&posts[1].body).unwrap();
    assert_eq!(content["msgtype"], "m.text");
    assert_eq!(content["format"], "org.matrix.custom.html");
    assert_eq!(content["m.mentions"], json!({}));
    assert_eq!(
        content["body"],
        "CRITICAL · Disk full\n\n**db-1** is at 97%. @room"
    );
    assert_eq!(
        content["formatted_body"],
        "<p><strong><span data-mx-color=\"#D93025\">CRITICAL · Disk full</span></strong></p>\
         <p><b>db-1</b> is at 97%. @room</p>"
    );
}

#[tokio::test]
async fn a_room_the_bot_may_not_join_says_to_invite_it() {
    let server = MockServer::start().await;
    whoami(&server).await;

    Mock::given(method("PUT"))
        .and(path_regex(send_path("%21secret%3Aexample.org")))
        .respond_with(refused(
            403,
            "M_FORBIDDEN",
            "User @bot:example.org not in room !secret:example.org",
        ))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path(format!("{API}/join/%21secret%3Aexample.org")))
        .and(query_param("via", "example.org"))
        .respond_with(refused(
            403,
            "M_FORBIDDEN",
            "You are not invited to this room.",
        ))
        .expect(1)
        .mount(&server)
        .await;

    let error = MatrixBotConnectorAdapter::new()
        .send(
            &config(&server),
            &target(NotifyTargetType::FixedGroup, "!secret:example.org"),
            &message(None),
        )
        .await
        .unwrap_err();

    assert!(matches!(error, Error::InvalidArgument(_)), "{error}");

    let error = error.to_string();
    assert!(
        error.contains("M_FORBIDDEN You are not invited to this room."),
        "{error}"
    );
    assert!(
        error.contains("invite @bot:example.org to the room"),
        "{error}"
    );
}

#[tokio::test]
async fn an_unknown_alias_is_named() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(format!("{API}/directory/room/%23gone%3Aexample.org")))
        .respond_with(refused(
            404,
            "M_NOT_FOUND",
            "Room alias #gone:example.org not found",
        ))
        .mount(&server)
        .await;

    let error = MatrixBotConnectorAdapter::new()
        .send(
            &config(&server),
            &target(NotifyTargetType::FixedGroup, "#gone:example.org"),
            &message(None),
        )
        .await
        .unwrap_err();

    assert!(matches!(error, Error::InvalidArgument(_)), "{error}");
    assert!(
        error
            .to_string()
            .contains("#gone:example.org does not exist"),
        "{error}"
    );
}

#[tokio::test]
async fn a_person_gets_the_newest_direct_room_they_are_still_in() {
    let server = MockServer::start().await;
    whoami(&server).await;

    Mock::given(method("GET"))
        .and(path(format!(
            "{API}/user/%40bot%3Aexample.org/account_data/m.direct"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "@alice:example.org": ["!old:example.org", "!dm:example.org"],
            "@bob:example.org": ["!b:example.org"],
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path(format!(
            "{API}/rooms/%21dm%3Aexample.org/state/m.room.member/%40alice%3Aexample.org"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "membership": "join" })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path(format!(
            "{API}/rooms/%21old%3Aexample.org/state/m.room.member/%40alice%3Aexample.org"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "membership": "join" })))
        .expect(0)
        .mount(&server)
        .await;

    Mock::given(method("PUT"))
        .and(path_regex(send_path("%21dm%3Aexample.org")))
        .respond_with(sent("$e2"))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path(format!("{API}/createRoom")))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;

    let result = MatrixBotConnectorAdapter::new()
        .send(
            &config(&server),
            &target(NotifyTargetType::DirectUser, "@alice:example.org"),
            &message(None),
        )
        .await
        .unwrap();

    assert_eq!(result.provider_message_id.as_deref(), Some("$e2"));
}

#[tokio::test]
async fn a_person_who_left_their_direct_room_gets_a_new_one_recorded_in_m_direct() {
    let server = MockServer::start().await;
    whoami(&server).await;

    let direct = format!("{API}/user/%40bot%3Aexample.org/account_data/m.direct");

    Mock::given(method("GET"))
        .and(path(direct.clone()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "@bob:example.org": ["!gone:example.org"],
            "@carol:example.org": ["!c:example.org"],
        })))
        .expect(2)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path(format!(
            "{API}/rooms/%21gone%3Aexample.org/state/m.room.member/%40bob%3Aexample.org"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "membership": "leave" })))
        .expect(2)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path(format!("{API}/createRoom")))
        .and(body_json(json!({
            "is_direct": true,
            "invite": ["@bob:example.org"],
            "preset": "trusted_private_chat",
        })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "room_id": "!new:example.org" })),
        )
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("PUT"))
        .and(path(direct))
        .and(body_json(json!({
            "@bob:example.org": ["!gone:example.org", "!new:example.org"],
            "@carol:example.org": ["!c:example.org"],
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("PUT"))
        .and(path_regex(send_path("%21new%3Aexample.org")))
        .respond_with(sent("$e3"))
        .expect(1)
        .mount(&server)
        .await;

    let result = MatrixBotConnectorAdapter::new()
        .send(
            &config(&server),
            &target(NotifyTargetType::DirectUser, "@bob:example.org"),
            &message(None),
        )
        .await
        .unwrap();

    assert_eq!(result.provider_message_id.as_deref(), Some("$e3"));
}

#[tokio::test]
async fn a_bot_without_m_direct_starts_it() {
    let server = MockServer::start().await;
    whoami(&server).await;

    let direct = format!("{API}/user/%40bot%3Aexample.org/account_data/m.direct");

    Mock::given(method("GET"))
        .and(path(direct.clone()))
        .respond_with(refused(404, "M_NOT_FOUND", "Account data not found"))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path(format!("{API}/createRoom")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "room_id": "!new:example.org" })),
        )
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("PUT"))
        .and(path(direct))
        .and(body_json(
            json!({ "@dave:example.org": ["!new:example.org"] }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("PUT"))
        .and(path_regex(send_path("%21new%3Aexample.org")))
        .respond_with(sent("$e4"))
        .expect(1)
        .mount(&server)
        .await;

    MatrixBotConnectorAdapter::new()
        .send(
            &config(&server),
            &target(NotifyTargetType::DirectUser, "@dave:example.org"),
            &message(None),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn a_rate_limit_says_when_to_retry() {
    let server = MockServer::start().await;

    Mock::given(method("PUT"))
        .and(path_regex(send_path("%21ops%3Aexample.org")))
        .respond_with(ResponseTemplate::new(429).set_body_json(json!({
            "errcode": "M_LIMIT_EXCEEDED",
            "error": "Too Many Requests",
            "retry_after_ms": 2_500,
        })))
        .mount(&server)
        .await;

    let error = MatrixBotConnectorAdapter::new()
        .send(
            &config(&server),
            &target(NotifyTargetType::FixedGroup, "!ops:example.org"),
            &message(None),
        )
        .await
        .unwrap_err();

    assert!(matches!(error, Error::Unavailable(_)), "{error}");
    assert!(error.to_string().contains("retry after 3 s"), "{error}");
}

#[test]
fn a_card_is_html_with_a_coloured_title_and_plain_text_beside_it() {
    let content = content(&with_open_link(message(Some(card()))));

    assert_eq!(
        content["body"],
        format!(
            "Disk <full> on db-1\n\n**db-1** is at 97%. @room\n\n\
             Host: db-1\nUsage <%>: 97% & rising\n\n\
             Open in MoleSignal: {OPEN_URL}\nRunbook: https://runbooks.example.com/disk?a=\"b\"\n\
             Acknowledge: {ACK_URL}\n\nSent by MoleSignal"
        )
    );
    assert_eq!(
        content["formatted_body"],
        format!(
            "<p><strong><span data-mx-color=\"#E8710A\">Disk &lt;full&gt; on db-1</span></strong></p>\
             <p><b>db-1</b> is at 97%. @room</p>\
             <ul><li><strong>Host</strong>: db-1</li>\
             <li><strong>Usage &lt;%&gt;</strong>: 97% &amp; rising</li></ul>\
             <p><a href=\"{OPEN_URL}\">Open in MoleSignal</a> · \
             <a href=\"https://runbooks.example.com/disk?a=&quot;b&quot;\">Runbook</a> · \
             <a href=\"{ACK_URL}\">Acknowledge</a></p>\
             <p><em>Sent by MoleSignal</em></p>"
        )
    );
    assert_eq!(content["m.mentions"], json!({}));
}

#[test]
fn a_long_text_keeps_the_event_under_the_size_limit() {
    let mut long = message(Some(card()));
    long.markdown = Some("é".repeat(100_000));

    let content = content(&long);
    let bytes = serde_json::to_vec(&content).unwrap().len();

    assert!(bytes <= 48_000, "{bytes}");
    assert!(
        content["body"]
            .as_str()
            .unwrap()
            .ends_with("Sent by MoleSignal")
    );
    assert!(content["formatted_body"].as_str().unwrap().contains('…'));
}
