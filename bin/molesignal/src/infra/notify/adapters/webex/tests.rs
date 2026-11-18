// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use domain::notify::{
    buttons::{AlertAction, AlertButtons},
    template::{NotifyCard, NotifyCardColor, NotifyCardField, NotifyCardLink},
};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_json, header, method, path},
};

use super::*;

const OPEN_URL: &str = "https://molesignal.example.com/alerts/incidents/inc-1";
const ACK_URL: &str = "https://molesignal.example.com/api/v1/public/notify/actions/t1";

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
        markdown: Some("**db-1** is at 97%. <@all>".into()),
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

fn link(label: &str, url: &str) -> NotifyCardLink {
    NotifyCardLink {
        label: label.into(),
        url: url.into(),
    }
}

fn card() -> NotifyCard {
    NotifyCard {
        title: "Disk full on db-1".into(),
        color: NotifyCardColor::Orange,
        fields: vec![
            NotifyCardField {
                label: "Host".into(),
                value: "db-1".into(),
            },
            NotifyCardField {
                label: "Usage".into(),
                value: "97%".into(),
            },
        ],
        links: vec![
            link("Runbook", "https://runbooks.example.com/disk"),
            link("Acknowledge", ACK_URL),
        ],
        note: "Sent by MoleSignal".into(),
    }
}

fn bot_config(server: &MockServer) -> Value {
    json!({ "botToken": "bot-token", "apiBaseUrl": server.uri() })
}

#[test]
fn configs_are_checked() {
    let bot = WebexBotConnectorAdapter::new();
    let webhook = WebexWebhookConnectorAdapter::new();

    bot.validate_config(&json!({ "botToken": "t" })).unwrap();
    bot.validate_config(&json!({ "botToken": "t", "apiBaseUrl": "" }))
        .unwrap();
    assert!(bot.validate_config(&json!({})).is_err());
    assert!(
        bot.validate_config(&json!({ "botToken": "t", "apiBaseUrl": "ftp://webexapis.com" }))
            .is_err()
    );
    assert!(
        bot.validate_config(&json!({ "botToken": "t", "timeoutSecs": 61 }))
            .is_err()
    );
    assert_eq!(
        bot.addresses(),
        vec![ConnectorAddress::fixed(
            "apiBaseUrl",
            "https://webexapis.com/v1"
        )]
    );

    for url in [
        "https://webexapis.com/v1/webhooks/incoming/Y2lzY29zcGFyazovL3Vz",
        "https://api.ciscospark.com/v1/webhooks/incoming/Y2lzY29zcGFyazovL3Vz",
    ] {
        webhook
            .validate_config(&json!({ "webhookUrl": url }))
            .unwrap_or_else(|error| panic!("{url}: {error}"));
    }

    for url in [
        "",
        "http://webexapis.com/v1/webhooks/incoming/abc",
        "https://webexapis.com/v1/messages",
        "https://webexapis.com/v1/webhooks/incoming/",
        "https://webexapis.com/v1/webhooks/incoming/abc/def",
        "https://hooks.slack.com/services/T0/B0/x",
        "https://webexapis.com.attacker.example/v1/webhooks/incoming/abc",
    ] {
        let error = webhook
            .validate_config(&json!({ "webhookUrl": url }))
            .unwrap_err();
        assert!(matches!(error, Error::InvalidArgument(_)), "{url}");
        assert!(error.to_string().contains("webhookUrl"), "{error}");
    }
}

#[test]
fn a_bot_writes_to_a_person_by_email_or_id_and_to_a_space_by_its_id() {
    let room = "Y2lzY29zcGFyazovL3VzL1JPT00vYmJjZWIxYWQ";
    let person = "Y2lzY29zcGFyazovL3VzL1BFT1BMRS9mNWIzNjE4Ny";

    assert_eq!(
        recipient(&target(NotifyTargetType::DirectUser, " alice@example.com ")).unwrap(),
        ("toPersonEmail", "alice@example.com")
    );
    assert_eq!(
        recipient(&target(NotifyTargetType::DirectUser, person)).unwrap(),
        ("toPersonId", person)
    );
    assert_eq!(
        recipient(&target(NotifyTargetType::FixedGroup, room)).unwrap(),
        ("roomId", room)
    );

    for (target_type, value) in [
        (NotifyTargetType::DirectUser, "@alice"),
        (NotifyTargetType::DirectUser, "alice@localhost"),
        (NotifyTargetType::DirectUser, "alice smith"),
        (NotifyTargetType::FixedGroup, ""),
        (NotifyTargetType::FixedGroup, "ops@example.com"),
        (NotifyTargetType::FixedAddress, "alice@example.com"),
        (
            NotifyTargetType::Webhook,
            "https://webexapis.com/v1/webhooks/incoming/abc",
        ),
    ] {
        assert!(
            WebexBotConnectorAdapter::new()
                .validate_target(&target(target_type, value))
                .is_err(),
            "{value}"
        );
    }

    let webhook = WebexWebhookConnectorAdapter::new();
    webhook
        .validate_target(&target(NotifyTargetType::FixedGroup, "ops"))
        .unwrap();
    webhook
        .validate_target(&target(
            NotifyTargetType::Webhook,
            "https://webexapis.com/v1/webhooks/incoming/other",
        ))
        .unwrap();
    assert!(
        webhook
            .validate_target(&target(NotifyTargetType::Webhook, "https://example.com/x"))
            .is_err()
    );
    assert!(
        webhook
            .validate_target(&target(NotifyTargetType::DirectUser, "alice@example.com"))
            .is_err()
    );
}

#[tokio::test]
async fn the_bot_posts_markdown_and_a_card_webex_can_draw() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/messages"))
        .and(header("authorization", "Bearer bot-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "msg-1" })))
        .expect(1)
        .mount(&server)
        .await;

    let result = WebexBotConnectorAdapter::new()
        .send(
            &bot_config(&server),
            &target(NotifyTargetType::DirectUser, "alice@example.com"),
            &with_open_link(message(Some(card()))),
        )
        .await
        .unwrap();

    assert_eq!(result.provider_message_id.as_deref(), Some("msg-1"));

    let requests = server.received_requests().await.unwrap();
    let sent: Value = serde_json::from_slice(&requests[0].body).unwrap();

    assert_eq!(sent["toPersonEmail"], "alice@example.com");
    assert!(sent.get("roomId").is_none());
    assert_eq!(
        sent["markdown"],
        format!(
            "**Disk full on db-1**\n\n**db-1** is at 97%. <\u{200B}@all>\n\n\
             **Host**: db-1\n**Usage**: 97%\n\
             [Open in MoleSignal]({OPEN_URL}) · [Runbook](https://runbooks.example.com/disk) · \
             [Acknowledge]({ACK_URL})\n_Sent by MoleSignal_"
        )
    );

    let attachment = &sent["attachments"][0];
    let card = &attachment["content"];

    assert_eq!(
        attachment["contentType"],
        "application/vnd.microsoft.card.adaptive"
    );
    assert_eq!(card["type"], "AdaptiveCard");
    assert_eq!(card["version"], "1.3");
    assert_eq!(card["body"][0]["style"], "warning");
    assert_eq!(card["body"][0]["items"][0]["text"], "Disk full on db-1");
    assert_eq!(card["body"][1]["text"], "**db-1** is at 97%. <@all>");
    assert_eq!(card["body"][2]["type"], "FactSet");
    assert_eq!(card["body"][3]["text"], "Sent by MoleSignal");

    let actions = card["actions"].as_array().unwrap();
    let urls = actions
        .iter()
        .map(|action| action["url"].as_str().unwrap())
        .collect::<Vec<_>>();

    assert!(
        actions
            .iter()
            .all(|action| action["type"] == "Action.OpenUrl")
    );
    assert_eq!(
        urls,
        [OPEN_URL, "https://runbooks.example.com/disk", ACK_URL]
    );

    // Nothing on the card can be pressed.
    let written = sent.to_string();
    assert!(!written.contains("Action.Execute") && !written.contains("Action.Submit"));
}

#[tokio::test]
async fn a_space_gets_the_card_of_a_plain_message_in_the_severity_colour() {
    let server = MockServer::start().await;
    let room = "Y2lzY29zcGFyazovL3VzL1JPT00vYmJjZWIxYWQ";

    Mock::given(method("POST"))
        .and(path("/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "msg-2" })))
        .expect(1)
        .mount(&server)
        .await;

    WebexBotConnectorAdapter::new()
        .send(
            &bot_config(&server),
            &target(NotifyTargetType::FixedGroup, room),
            &message(None),
        )
        .await
        .unwrap();

    let requests = server.received_requests().await.unwrap();
    let sent: Value = serde_json::from_slice(&requests[0].body).unwrap();
    let card = &sent["attachments"][0]["content"];

    assert_eq!(sent["roomId"], room);
    assert_eq!(
        sent["markdown"],
        "**CRITICAL · Disk full**\n\n**db-1** is at 97%. <\u{200B}@all>"
    );
    assert_eq!(card["body"][0]["style"], "attention");
    assert_eq!(card["body"][0]["items"][0]["text"], "CRITICAL · Disk full");
    assert_eq!(card["actions"], json!([]));
}

#[test]
fn a_card_keeps_five_buttons_and_lists_the_rest_under_its_text() {
    let mut card = card();
    card.links = (1..=6)
        .map(|index| {
            link(
                &format!("Link {index}"),
                &format!("https://l.example.com/{index}"),
            )
        })
        .collect();

    let sent = bot_message(&with_open_link(message(Some(card))));
    let content = &sent["attachments"][0]["content"];
    let actions = content["actions"].as_array().unwrap();
    let blocks = content["body"].as_array().unwrap();

    assert_eq!(actions.len(), 5);
    assert_eq!(actions[0]["url"], OPEN_URL);
    assert_eq!(actions[4]["url"], "https://l.example.com/4");

    // The rest sit above the footer.
    let footer = blocks.len() - 1;
    assert_eq!(blocks[footer]["text"], "Sent by MoleSignal");
    assert_eq!(
        blocks[footer - 1]["text"],
        "[Link 5](https://l.example.com/5) · [Link 6](https://l.example.com/6)"
    );
}

#[test]
fn a_long_message_keeps_within_webex_limits_and_keeps_its_links() {
    let mut long = message(Some(card()));
    long.markdown = Some("é".repeat(40_000));

    let sent = bot_message(&long);
    let markdown = sent["markdown"].as_str().unwrap();
    let card_bytes = serde_json::to_vec(&sent["attachments"][0]["content"])
        .unwrap()
        .len();

    assert!(markdown.len() <= MAX_MARKDOWN_BYTES);
    assert!(markdown.ends_with(&format!("[Acknowledge]({ACK_URL})\n_Sent by MoleSignal_")));
    assert!(markdown.len() + card_bytes <= 22_000);
    assert!(
        sent["attachments"][0]["content"]["body"][1]["text"]
            .as_str()
            .unwrap()
            .ends_with('…')
    );

    // A card whose fields alone pass the limit is left out; the Markdown still goes.
    let mut crowded = card();
    crowded.fields = (0..400)
        .map(|index| NotifyCardField {
            label: format!("Field {index}"),
            value: "v".repeat(100),
        })
        .collect();

    let sent = bot_message(&message(Some(crowded)));
    assert!(sent.get("attachments").is_none());
    assert!(sent["markdown"].as_str().unwrap().len() <= MAX_MARKDOWN_BYTES);
}

#[tokio::test]
async fn a_rate_limit_says_when_to_retry() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/messages"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "30"))
        .mount(&server)
        .await;

    let error = WebexBotConnectorAdapter::new()
        .send(
            &bot_config(&server),
            &target(NotifyTargetType::DirectUser, "alice@example.com"),
            &message(None),
        )
        .await
        .unwrap_err();

    assert!(matches!(error, Error::Unavailable(_)), "{error}");
    assert!(error.to_string().contains("retry after 30 s"), "{error}");
}

#[tokio::test]
async fn a_refusal_says_why() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/messages"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "message": "Could not find a room with provided ID.",
            "errors": [{ "description": "Could not find a room with provided ID." }],
            "trackingId": "ROUTER_1234",
        })))
        .mount(&server)
        .await;

    let error = WebexBotConnectorAdapter::new()
        .send(
            &bot_config(&server),
            &target(NotifyTargetType::FixedGroup, "Y2lzY29zcGFyazovL3Vz"),
            &message(None),
        )
        .await
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("HTTP 404 Could not find a room with provided ID. (trackingId ROUTER_1234)"),
        "{error}"
    );
}

#[tokio::test]
async fn the_webhook_posts_markdown_to_its_space() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/webhooks/incoming/other"))
        .and(body_json(json!({
            "markdown": format!(
                "**Disk full on db-1**\n\n**db-1** is at 97%. <\u{200B}@all>\n\n\
                 **Host**: db-1\n**Usage**: 97%\n\
                 [Open in MoleSignal]({OPEN_URL}) · [Runbook](https://runbooks.example.com/disk) · \
                 [Acknowledge]({ACK_URL})\n_Sent by MoleSignal_"
            ),
        })))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let config = json!({ "webhookUrl": format!("{}/v1/webhooks/incoming/s3cr3t", server.uri()) });
    let target = target(
        NotifyTargetType::Webhook,
        &format!("{}/v1/webhooks/incoming/other", server.uri()),
    );

    let result = WebexWebhookConnectorAdapter::new()
        .send(&config, &target, &with_open_link(message(Some(card()))))
        .await
        .unwrap();

    assert!(result.delivered);
    assert_eq!(result.provider_message_id, None);
}

#[tokio::test]
async fn a_refused_webhook_never_names_its_url() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/webhooks/incoming/s3cr3t"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "message": "Unable to post message to room.",
            "trackingId": "ROUTER_5678",
        })))
        .mount(&server)
        .await;

    let config = json!({ "webhookUrl": format!("{}/v1/webhooks/incoming/s3cr3t", server.uri()) });

    let error = WebexWebhookConnectorAdapter::new()
        .send(
            &config,
            &target(NotifyTargetType::FixedGroup, "ops"),
            &message(None),
        )
        .await
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("webex_webhook was refused: HTTP 400"),
        "{error}"
    );
    assert!(!error.contains("s3cr3t"), "{error}");
}

#[test]
fn mentions_in_the_text_ping_no_one() {
    let mut loud = message(None);
    loud.title = "<@all> look".into();
    loud.markdown = Some("<@personEmail:boss@example.com|Boss> and <@all>".into());

    let markdown = markdown(&loud, MAX_MARKDOWN_BYTES);

    assert!(!markdown.contains("<@"), "{markdown}");
    assert_eq!(markdown.matches('\u{200B}').count(), 3);
}
