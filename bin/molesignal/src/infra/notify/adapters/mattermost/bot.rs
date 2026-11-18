// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The `mattermost_bot` connector: posts as a bot account through Mattermost's REST API, to a
//! person's direct channel with the bot or to a channel, with the alert buttons when the engine
//! gave them the address presses go to.

use std::time::Instant;

use async_trait::async_trait;
use domain::notify::{
    buttons::AlertButtons,
    connector::{
        ConnectorAdapter, ConnectorCapabilities, ConnectorDeliveryResult, NotifyChannel,
        NotifyMessage, NotifyTarget,
    },
};
use kernel::Result;
use serde_json::Value;

use super::{
    attachment::mattermost_post,
    client::{Bot, BotConfig, Target},
};
use crate::infra::notify::adapters::common::delivered;

pub const MATTERMOST_BOT_CONNECTOR_TYPE: &str = "mattermost_bot";

#[derive(Default)]
pub struct MattermostBotConnectorAdapter;

impl MattermostBotConnectorAdapter {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ConnectorAdapter for MattermostBotConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        MATTERMOST_BOT_CONNECTOR_TYPE
    }

    fn capabilities(&self) -> ConnectorCapabilities {
        ConnectorCapabilities {
            direct_user: true,
            group: true,
            rich_text: true,
            interactive: true,
            acknowledgement: true,
            attachments: false,
        }
    }

    fn channel(&self) -> NotifyChannel {
        NotifyChannel::Im
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        BotConfig::parse(config).map(drop)
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        Target::parse(target).map(drop)
    }

    async fn send(
        &self,
        config: &Value,
        target: &NotifyTarget,
        message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        let config = BotConfig::parse(config)?;
        let target = Target::parse(target)?;
        let bot = Bot::connect(&config).await?;
        let started = Instant::now();

        let channel_id = bot.channel(&target).await?;
        let post = mattermost_post(message, AlertButtons::read(message).as_ref(), None);
        let post_id = bot.create_post(&channel_id, &post).await?;

        Ok(delivered(post_id, started))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use domain::notify::{
        buttons::{AlertAction, AlertButtonLabels},
        connector::NotifyTargetType,
    };
    use kernel::Error;
    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_json, header, method, path},
    };

    use super::*;

    const BOT_ID: &str = "b0tb0tb0tb0tb0tb0tb0tb0tb0";
    const ALICE_ID: &str = "a11cea11cea11cea11cea11cea";
    const CHANNEL_ID: &str = "c4anne1c4anne1c4anne1c4ann";
    const CALLBACK: &str =
        "https://ms.example.com/api/v1/public/notify/conn-1/mattermost?token=t0k";

    fn config(server: &MockServer) -> Value {
        json!({ "serverUrl": server.uri(), "botToken": "bot-token" })
    }

    fn target(target_type: NotifyTargetType, value: &str) -> NotifyTarget {
        NotifyTarget {
            target_type,
            value: value.into(),
            metadata: BTreeMap::new(),
        }
    }

    fn message() -> NotifyMessage {
        NotifyMessage {
            title: "Weekly report".into(),
            text: "All quiet.".into(),
            markdown: None,
            html: None,
            metadata: BTreeMap::new(),
            provider_templates: Default::default(),
            event: None,
            card: None,
        }
    }

    /// An open alert whose buttons post their presses to [`CALLBACK`].
    fn alert() -> NotifyMessage {
        let mut message = message();
        message.title = "CRITICAL · Checkout errors".into();
        message
            .metadata
            .insert("severity".into(), "critical".into());

        let labels = AlertButtonLabels {
            acknowledge: "Acknowledge".into(),
            resolve: "Resolve".into(),
            snooze: "Snooze 1h".into(),
            acknowledge_all: "Acknowledge all".into(),
            approve: "Approve".into(),
            reject: "Reject".into(),
            open: Some(("Open in MoleSignal".into(), "https://ms.example.com".into())),
            callback: Some(CALLBACK.into()),
        };

        AlertButtons::new("inc-1", "open", "alert.triggered", None, &labels).write(&mut message);
        message
    }

    async fn mount_post(server: &MockServer, body: Value) {
        Mock::given(method("POST"))
            .and(path("/api/v4/posts"))
            .and(header("authorization", "Bearer bot-token"))
            .and(body_json(body))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "id": "post-1" })))
            .expect(1)
            .mount(server)
            .await;
    }

    #[test]
    fn it_reaches_people_and_channels_on_a_server_people_enter() {
        let adapter = MattermostBotConnectorAdapter::new();

        assert!(adapter.addresses().is_empty());
        assert!(adapter.capabilities().interactive);
        assert_eq!(adapter.channel(), NotifyChannel::Im);
        adapter
            .validate_config(&json!({ "serverUrl": "https://chat.example.com", "botToken": "t" }))
            .unwrap();
        assert!(
            adapter
                .validate_config(&json!({ "botToken": "t" }))
                .is_err()
        );
        adapter
            .validate_target(&target(NotifyTargetType::DirectUser, "@alice"))
            .unwrap();
        adapter
            .validate_target(&target(NotifyTargetType::FixedGroup, "ops/alerts"))
            .unwrap();
        assert!(
            adapter
                .validate_target(&target(NotifyTargetType::Webhook, "https://x.example.com"))
                .is_err()
        );
    }

    #[tokio::test]
    async fn a_person_gets_the_alert_in_their_direct_channel_with_the_bot() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/v4/users/username/alice"))
            .and(header("authorization", "Bearer bot-token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "id": ALICE_ID, "username": "alice" })),
            )
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/api/v4/users/me"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": BOT_ID })))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/api/v4/channels/direct"))
            .and(body_json(json!([BOT_ID, ALICE_ID])))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "id": CHANNEL_ID })))
            .expect(1)
            .mount(&server)
            .await;

        let alert = alert();
        let expected = mattermost_post(&alert, AlertButtons::read(&alert).as_ref(), None);
        let button = &expected.attachments[0]["actions"][0];
        assert_eq!(button["integration"]["url"], CALLBACK);
        assert_eq!(button["integration"]["context"]["alert_id"], "inc-1");

        mount_post(
            &server,
            json!({ "channel_id": CHANNEL_ID, "message": "", "props": expected.props() }),
        )
        .await;

        let result = MattermostBotConnectorAdapter::new()
            .send(
                &config(&server),
                &target(NotifyTargetType::DirectUser, "@alice"),
                &alert,
            )
            .await
            .unwrap();

        assert!(result.delivered);
        assert_eq!(result.provider_message_id.as_deref(), Some("post-1"));
    }

    #[tokio::test]
    async fn a_user_id_is_tried_as_an_id_before_a_username() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path(format!("/api/v4/users/{ALICE_ID}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": ALICE_ID })))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/api/v4/users/me"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": BOT_ID })))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/api/v4/channels/direct"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "id": CHANNEL_ID })))
            .mount(&server)
            .await;

        mount_post(
            &server,
            json!({ "channel_id": CHANNEL_ID, "message": "**Weekly report**\nAll quiet." }),
        )
        .await;

        MattermostBotConnectorAdapter::new()
            .send(
                &config(&server),
                &target(NotifyTargetType::DirectUser, ALICE_ID),
                &message(),
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_channel_is_found_by_its_team_and_name_or_taken_by_id() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/v4/teams/name/ops/channels/name/alerts"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": CHANNEL_ID })))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/api/v4/posts"))
            .and(body_json(json!({
                "channel_id": CHANNEL_ID,
                "message": "**Weekly report**\nAll quiet.",
            })))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "id": "post-1" })))
            .expect(2)
            .mount(&server)
            .await;

        let adapter = MattermostBotConnectorAdapter::new();

        for value in ["ops/alerts", CHANNEL_ID] {
            adapter
                .send(
                    &config(&server),
                    &target(NotifyTargetType::FixedGroup, value),
                    &message(),
                )
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn someone_the_server_does_not_know_is_a_target_problem() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/v4/users/username/bob"))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({
                "id": "app.user.missing_account.const",
                "message": "Unable to find the user.",
                "status_code": 404,
            })))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/api/v4/teams/name/ops/channels/name/secret"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let adapter = MattermostBotConnectorAdapter::new();

        let person = adapter
            .send(
                &config(&server),
                &target(NotifyTargetType::DirectUser, "bob"),
                &message(),
            )
            .await;
        assert!(
            matches!(&person, Err(Error::InvalidArgument(text)) if text.contains("@bob")),
            "{person:?}"
        );

        let channel = adapter
            .send(
                &config(&server),
                &target(NotifyTargetType::FixedGroup, "ops/secret"),
                &message(),
            )
            .await;
        assert!(
            matches!(&channel, Err(Error::InvalidArgument(text)) if text.contains("ops/secret")),
            "{channel:?}"
        );
    }

    #[tokio::test]
    async fn the_servers_refusal_and_rate_limit_are_told_in_its_words() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/api/v4/posts"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({
                "id": "api.context.permissions.app_error",
                "message": "You do not have the appropriate permissions.",
                "status_code": 403,
            })))
            .up_to_n_times(1)
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/api/v4/posts"))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("X-Ratelimit-Reset", "3")
                    .set_body_string("limit exceeded"),
            )
            .mount(&server)
            .await;

        let adapter = MattermostBotConnectorAdapter::new();
        let channel = target(NotifyTargetType::FixedGroup, CHANNEL_ID);

        let refused = adapter
            .send(&config(&server), &channel, &message())
            .await
            .unwrap_err()
            .to_string();
        assert!(refused.contains("HTTP 403"), "{refused}");
        assert!(
            refused.contains("You do not have the appropriate permissions."),
            "{refused}"
        );

        let limited = adapter
            .send(&config(&server), &channel, &message())
            .await
            .unwrap_err();
        assert!(matches!(limited, Error::Unavailable(_)), "{limited:?}");
        assert!(limited.to_string().contains("retry in 3 s"), "{limited}");
        assert!(limited.to_string().contains("limit exceeded"), "{limited}");
    }

    #[tokio::test]
    async fn a_bundle_offers_to_acknowledge_every_alert_in_it() {
        let mut bundle = message();
        let labels = AlertButtonLabels {
            acknowledge: "Acknowledge".into(),
            resolve: "Resolve".into(),
            snooze: "Snooze 1h".into(),
            acknowledge_all: "Acknowledge all".into(),
            approve: "Approve".into(),
            reject: "Reject".into(),
            open: None,
            callback: Some(CALLBACK.into()),
        };
        let ids = vec!["inc-1".to_string(), "inc-2".to_string()];

        AlertButtons::bundle(&ids, "alert.triggered", Some("policy-1"), &labels).write(&mut bundle);

        let post = mattermost_post(&bundle, AlertButtons::read(&bundle).as_ref(), None);
        let buttons = post.attachments[0]["actions"].as_array().unwrap();

        assert_eq!(buttons.len(), 1);
        assert_eq!(buttons[0]["name"], "Acknowledge all");
        assert_eq!(
            buttons[0]["integration"]["context"],
            json!({
                "action": AlertAction::AcknowledgeAll.as_str(),
                "alert_id": "inc-1",
                "event_type": "alert.triggered",
                "policy_id": "policy-1",
                "alert_ids": ["inc-1", "inc-2"],
            })
        );
    }
}
