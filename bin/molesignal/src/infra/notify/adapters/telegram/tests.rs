// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use domain::notify::{
    buttons::AlertAction,
    connector::validate_addresses,
    template::{NotifyCard, NotifyCardColor, NotifyCardField, NotifyCardLink},
};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_partial_json, method, path},
};

use super::*;
use crate::infra::im::telegram::{callback, html};

const TOKEN: &str = "123456:TEST-token_x";

fn config(server: &MockServer) -> Value {
    json!({ "botToken": TOKEN, "apiBaseUrl": server.uri() })
}

fn send_path() -> String {
    format!("/bot{TOKEN}/sendMessage")
}

fn target(target_type: NotifyTargetType, value: &str) -> NotifyTarget {
    NotifyTarget {
        target_type,
        value: value.into(),
        metadata: BTreeMap::new(),
    }
}

fn alert() -> NotifyMessage {
    NotifyMessage {
        title: "CRITICAL · p99 > 2s & rising".into(),
        text: "Alert is open.".into(),
        markdown: Some("**API** is slow on `checkout`".into()),
        html: None,
        metadata: BTreeMap::from([("severity".to_string(), "critical".to_string())]),
        provider_templates: Default::default(),
        event: None,
        card: None,
    }
}

fn buttons() -> AlertButtons {
    AlertButtons {
        alert_id: "inc-1".into(),
        event_type: "alert.triggered".into(),
        policy_id: Some("policy-1".into()),
        actions: vec![
            (AlertAction::Acknowledge, "Acknowledge".into()),
            (AlertAction::Resolve, "Resolve".into()),
        ],
        open: Some((
            "Open in MoleSignal".into(),
            "https://ms.example/alerts/incidents/inc-1".into(),
        )),
        ..AlertButtons::default()
    }
}

fn delivered_response() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "ok": true,
        "result": { "message_id": 42, "chat": { "id": 123_456_789 }, "date": 1_700_000_000 },
    }))
}

fn refusal(code: u16, description: &str) -> ResponseTemplate {
    ResponseTemplate::new(code).set_body_json(json!({
        "ok": false,
        "error_code": code,
        "description": description,
    }))
}

#[test]
fn a_config_needs_a_token_shaped_like_botfathers() {
    let adapter = TelegramBotConnectorAdapter::new();

    adapter
        .validate_config(&json!({ "botToken": "123456789:AAE-x_y" }))
        .unwrap();
    adapter
        .validate_config(&json!({ "botToken": " 1:a ", "timeoutSecs": 60 }))
        .unwrap();

    for bad in [
        json!({}),
        json!({ "botToken": "" }),
        json!({ "botToken": "abc" }),
        json!({ "botToken": "123:" }),
        json!({ "botToken": ":abc" }),
        json!({ "botToken": "12a:abc" }),
        json!({ "botToken": "1:a b" }),
        json!({ "botToken": 5 }),
        json!({ "botToken": "1:a", "timeoutSecs": 0 }),
        json!({ "botToken": "1:a", "timeoutSecs": 61 }),
        json!({ "botToken": "1:a", "apiBaseUrl": "ftp://api.telegram.org" }),
    ] {
        let error = adapter.validate_config(&bad).unwrap_err();

        assert!(matches!(error, Error::InvalidArgument(_)), "{bad}");
        assert!(error.to_string().contains("telegram_bot"), "{error}");
    }
}

#[test]
fn the_bot_api_is_the_only_address_and_the_bot_does_what_an_app_does() {
    let adapter = TelegramBotConnectorAdapter::new();
    let addresses = adapter.addresses();
    let admitted = |config: Value| {
        validate_addresses(TELEGRAM_BOT_CONNECTOR_TYPE, &addresses, &config).is_ok()
    };

    assert!(admitted(json!({ "botToken": "1:a" })));
    assert!(admitted(
        json!({ "apiBaseUrl": "https://api.telegram.org/" })
    ));
    assert!(!admitted(
        json!({ "apiBaseUrl": "https://attacker.example" })
    ));
    assert!(!admitted(
        json!({ "apiBaseUrl": "http://169.254.169.254/" })
    ));

    let can = adapter.capabilities();
    assert!(can.direct_user && can.group && can.rich_text && can.interactive);
    assert!(!can.attachments);
    assert_eq!(adapter.channel(), NotifyChannel::Im);
}

#[test]
fn people_are_user_ids_and_groups_chat_ids_or_channel_names() {
    let adapter = TelegramBotConnectorAdapter::new();
    let ok =
        |kind: NotifyTargetType, value: &str| adapter.validate_target(&target(kind, value)).is_ok();

    assert!(ok(NotifyTargetType::DirectUser, "123456789"));
    assert!(ok(NotifyTargetType::DirectUser, " 42 "));
    assert!(!ok(NotifyTargetType::DirectUser, "-100123"));
    assert!(!ok(NotifyTargetType::DirectUser, "+42"));
    assert!(!ok(NotifyTargetType::DirectUser, "@alice"));
    assert!(ok(NotifyTargetType::FixedGroup, "-1001234567890"));
    assert!(ok(NotifyTargetType::FixedGroup, "-4242"));
    assert!(ok(NotifyTargetType::FixedGroup, "@ops_alerts"));
    assert!(!ok(NotifyTargetType::FixedGroup, "@ops"));
    assert!(!ok(NotifyTargetType::FixedGroup, "@1ops_alerts"));
    assert!(!ok(NotifyTargetType::FixedGroup, "4242"));
    assert!(!ok(NotifyTargetType::FixedGroup, "-"));
    assert!(!ok(NotifyTargetType::FixedAddress, "42"));
    assert!(!ok(NotifyTargetType::Webhook, "https://example.com/hook"));

    assert_eq!(
        chat(&target(NotifyTargetType::FixedGroup, "@ops_alerts")).unwrap(),
        json!("@ops_alerts")
    );
    assert_eq!(
        chat(&target(NotifyTargetType::FixedGroup, "-100123")).unwrap(),
        json!(-100_123)
    );
}

#[tokio::test]
async fn an_alert_goes_out_as_html_with_its_buttons() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(send_path()))
        .respond_with(delivered_response())
        .mount(&server)
        .await;

    let mut message = alert();
    buttons().write(&mut message);

    let result = TelegramBotConnectorAdapter::new()
        .send(
            &config(&server),
            &target(NotifyTargetType::DirectUser, "123456789"),
            &message,
        )
        .await
        .unwrap();

    assert!(result.delivered);
    assert_eq!(result.provider_message_id.as_deref(), Some("123456789:42"));

    let requests = server.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&requests[0].body).unwrap();

    assert_eq!(body["chat_id"], 123_456_789);
    assert_eq!(body["parse_mode"], "HTML");
    assert_eq!(body["link_preview_options"]["is_disabled"], true);
    assert_eq!(
        body["text"],
        "<b>CRITICAL · p99 &gt; 2s &amp; rising</b>\n<b>API</b> is slow on <code>checkout</code>"
    );

    let keyboard = &body["reply_markup"]["inline_keyboard"];
    assert_eq!(keyboard[0][0]["text"], "Acknowledge");
    assert_eq!(keyboard[0][0]["style"], "primary");

    for (index, action) in [AlertAction::Acknowledge, AlertAction::Resolve]
        .into_iter()
        .enumerate()
    {
        let data = keyboard[0][index]["callback_data"].as_str().unwrap();

        assert!(data.len() <= callback::MAX_DATA_BYTES);
        assert_eq!(callback::decode(data), Some(buttons().value(action)));
    }

    assert_eq!(
        keyboard[1],
        json!([{
            "text": "Open in MoleSignal",
            "url": "https://ms.example/alerts/incidents/inc-1",
        }])
    );
}

#[test]
fn a_template_card_reads_as_a_coloured_title_fields_and_notes() {
    let field = |label: &str, value: &str| NotifyCardField {
        label: label.into(),
        value: value.into(),
    };
    let link = |label: &str, url: &str| NotifyCardLink {
        label: label.into(),
        url: url.into(),
    };

    let message = NotifyMessage {
        card: Some(NotifyCard {
            title: String::new(),
            color: NotifyCardColor::Severity,
            fields: vec![field("Host", "db-1 <primary>"), field("Service", "`api`")],
            links: vec![
                link("Runbook", "https://runbooks.example.com/disk"),
                link("Script", "javascript:alert(1)"),
            ],
            note: "MoleSignal".into(),
        }),
        ..alert()
    };

    let fields = card::render(&message, None, Some("Alice acknowledged this alert."), true);

    assert_eq!(
        fields["text"],
        "🔴 <b>CRITICAL · p99 &gt; 2s &amp; rising</b>\n<b>API</b> is slow on <code>checkout</code>\n\n\
         <b>Host:</b> db-1 &lt;primary&gt;\n<b>Service:</b> <code>api</code>\n\n\
         <i>Alice acknowledged this alert.</i>\n<i>MoleSignal</i>"
    );
    assert_eq!(
        fields["reply_markup"]["inline_keyboard"],
        json!([[{ "text": "Runbook", "url": "https://runbooks.example.com/disk" }]])
    );

    // Without link buttons the links are lines of the text, above the notes.
    let lines = card::render(&message, None, None, false);
    let text = lines["text"].as_str().unwrap();

    assert!(lines.get("reply_markup").is_none());
    assert!(text.ends_with("Runbook: https://runbooks.example.com/disk\n\n<i>MoleSignal</i>"));
    assert!(!text.contains("javascript"));
}

#[test]
fn acknowledge_all_keeps_only_its_link() {
    let bundle = AlertButtons {
        actions: vec![(AlertAction::AcknowledgeAll, "Acknowledge all".into())],
        alert_ids: vec!["inc-1".into(), "inc-2".into()],
        open: Some(("Open".into(), "https://ms.example/alerts/incidents".into())),
        ..buttons()
    };

    let fields = card::render(&alert(), Some(&bundle), None, true);

    assert_eq!(
        fields["reply_markup"]["inline_keyboard"],
        json!([[{ "text": "Open", "url": "https://ms.example/alerts/incidents" }]])
    );
}

#[test]
fn a_long_body_is_cut_to_fit_one_message() {
    let message = NotifyMessage {
        markdown: Some(format!("{}\n", "word ".repeat(400)).repeat(10)),
        ..alert()
    };

    let fields = card::render(
        &message,
        Some(&buttons()),
        Some("Alice acknowledged."),
        true,
    );
    let text = fields["text"].as_str().unwrap();

    assert!(html::visible_len(text) <= html::MAX_TEXT);
    assert!(text.contains("…\n\n<i>Alice acknowledged.</i>"));
}

async fn refused_with(server: &MockServer, user: &str) -> Error {
    TelegramBotConnectorAdapter::new()
        .send(
            &config(server),
            &target(NotifyTargetType::DirectUser, user),
            &alert(),
        )
        .await
        .unwrap_err()
}

#[tokio::test]
async fn refusals_say_what_telegram_said() {
    let server = MockServer::start().await;
    let answers = [
        (
            1,
            refusal(
                403,
                "Forbidden: bot can't initiate conversation with a user",
            ),
        ),
        (2, {
            ResponseTemplate::new(429).set_body_json(json!({
                "ok": false,
                "error_code": 429,
                "description": "Too Many Requests: retry after 7",
                "parameters": { "retry_after": 7 },
            }))
        }),
        (3, refusal(401, "Unauthorized")),
        (
            4,
            ResponseTemplate::new(502).set_body_string("<html>Bad Gateway</html>"),
        ),
    ];

    for (chat_id, answer) in answers {
        Mock::given(method("POST"))
            .and(path(send_path()))
            .and(body_partial_json(json!({ "chat_id": chat_id })))
            .respond_with(answer)
            .mount(&server)
            .await;
    }

    let never_started = refused_with(&server, "1").await;
    assert!(matches!(never_started, Error::InvalidArgument(_)));
    assert!(
        never_started
            .to_string()
            .contains("403 Forbidden: bot can't initiate conversation with a user"),
        "{never_started}"
    );
    assert!(never_started.to_string().contains("start the bot first"));

    let limited = refused_with(&server, "2").await;
    assert!(matches!(limited, Error::Unavailable(_)));
    assert!(limited.to_string().contains("retry after 7 s"), "{limited}");

    let token = refused_with(&server, "3").await;
    assert!(token.to_string().contains("check the bot token"), "{token}");

    let outage = refused_with(&server, "4").await;
    assert!(matches!(outage, Error::Unavailable(_)));
    assert!(outage.to_string().contains("502"), "{outage}");
    assert!(!outage.to_string().contains(TOKEN));
}

#[tokio::test]
async fn links_telegram_will_not_take_go_into_the_text() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(send_path()))
        .respond_with(refusal(
            400,
            "Bad Request: inline keyboard button URL 'http://ms.internal/alerts/incidents/inc-1' \
             is invalid: Wrong HTTP URL",
        ))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(send_path()))
        .respond_with(delivered_response())
        .with_priority(2)
        .mount(&server)
        .await;

    let mut message = alert();
    AlertButtons {
        open: Some((
            "Open in MoleSignal".into(),
            "http://ms.internal/alerts/incidents/inc-1".into(),
        )),
        ..buttons()
    }
    .write(&mut message);

    TelegramBotConnectorAdapter::new()
        .send(
            &config(&server),
            &target(NotifyTargetType::FixedGroup, "-100123"),
            &message,
        )
        .await
        .unwrap();

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);

    let retried: Value = serde_json::from_slice(&requests[1].body).unwrap();
    let keyboard = retried["reply_markup"]["inline_keyboard"]
        .as_array()
        .unwrap();

    assert_eq!(retried["chat_id"], -100_123);
    assert_eq!(keyboard.len(), 1);
    assert!(keyboard[0][0]["callback_data"].is_string());
    assert!(
        retried["text"]
            .as_str()
            .unwrap()
            .ends_with("Open in MoleSignal: http://ms.internal/alerts/incidents/inc-1")
    );
}

#[tokio::test]
async fn html_telegram_cannot_read_goes_out_plain() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(send_path()))
        .and(body_partial_json(json!({ "parse_mode": "HTML" })))
        .respond_with(refusal(
            400,
            "Bad Request: can't parse entities: Can't find end tag corresponding to start tag \"b\"",
        ))
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(send_path()))
        .respond_with(delivered_response())
        .with_priority(2)
        .mount(&server)
        .await;

    TelegramBotConnectorAdapter::new()
        .send(
            &config(&server),
            &target(NotifyTargetType::FixedGroup, "@ops_alerts"),
            &alert(),
        )
        .await
        .unwrap();

    let requests = server.received_requests().await.unwrap();
    let plain: Value = serde_json::from_slice(&requests[1].body).unwrap();

    assert_eq!(plain["chat_id"], "@ops_alerts");
    assert!(plain.get("parse_mode").is_none());
    assert_eq!(
        plain["text"],
        "CRITICAL · p99 > 2s & rising\nAPI is slow on checkout"
    );
}
