// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The `mattermost_webhook` connector: posts to a channel's incoming webhook. Nobody who pressed a
//! button on such a post could be told apart, so alerts keep only their link to MoleSignal.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use domain::notify::{
    buttons::AlertButtons,
    connector::{
        ConnectorAdapter, ConnectorCapabilities, ConnectorDeliveryResult, NotifyChannel,
        NotifyMessage, NotifyTarget, NotifyTargetType,
    },
};
use kernel::{Error, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use signals::http_trace::{self, HttpTarget};
use url::Url;

use super::{attachment::mattermost_post, check};
use crate::infra::{
    connectors::dispatcher::http::pinned_client,
    notify::adapters::common::{default_timeout_secs, delivered, validate_timeout},
};

pub const MATTERMOST_WEBHOOK_CONNECTOR_TYPE: &str = "mattermost_webhook";
const CONNECTOR: &str = MATTERMOST_WEBHOOK_CONNECTOR_TYPE;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebhookConfig {
    #[serde(default)]
    webhook_url: String,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
}

impl WebhookConfig {
    fn parse(value: &Value) -> Result<Self> {
        let config: Self = serde_json::from_value(value.clone())
            .map_err(|error| Error::invalid(format!("invalid {CONNECTOR} config: {error}")))?;

        hook_url(&config.webhook_url, "webhookUrl")?;
        validate_timeout(config.timeout_secs, CONNECTOR)?;
        Ok(config)
    }
}

/// An incoming webhook's address: `http(s)://<server>/hooks/<key>`, the server maybe under a path.
fn hook_url(value: &str, field: &str) -> Result<Url> {
    let refused = || {
        Error::invalid(format!(
            "{CONNECTOR} {field} must be a Mattermost incoming webhook \
             URL, like https://chat.example.com/hooks/…"
        ))
    };

    let url = Url::parse(value.trim()).map_err(|_| refused())?;

    let hook = url.path_segments().is_some_and(|segments| {
        let segments = segments.collect::<Vec<_>>();

        matches!(segments.as_slice(), [.., "hooks", key] if !key.is_empty())
    });

    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() || !hook {
        return Err(refused());
    }

    Ok(url)
}

#[derive(Default)]
pub struct MattermostWebhookConnectorAdapter;

impl MattermostWebhookConnectorAdapter {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ConnectorAdapter for MattermostWebhookConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        MATTERMOST_WEBHOOK_CONNECTOR_TYPE
    }

    fn capabilities(&self) -> ConnectorCapabilities {
        ConnectorCapabilities {
            direct_user: false,
            group: true,
            rich_text: true,
            interactive: false,
            acknowledgement: false,
            attachments: false,
        }
    }

    fn channel(&self) -> NotifyChannel {
        NotifyChannel::Im
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        WebhookConfig::parse(config).map(drop)
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        match target.target_type {
            NotifyTargetType::Webhook => hook_url(&target.value, "target").map(drop),
            NotifyTargetType::FixedGroup if !target.value.trim().is_empty() => Ok(()),
            NotifyTargetType::FixedGroup => Err(Error::invalid(format!(
                "{CONNECTOR} target cannot be empty"
            ))),
            NotifyTargetType::DirectUser | NotifyTargetType::FixedAddress => Err(Error::invalid(
                format!("{CONNECTOR} target_type must be fixed_group or webhook"),
            )),
        }
    }

    async fn send(
        &self,
        config: &Value,
        target: &NotifyTarget,
        message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        let config = WebhookConfig::parse(config)?;
        self.validate_target(target)?;

        let url = if target.target_type == NotifyTargetType::Webhook {
            hook_url(&target.value, "target")?
        } else {
            hook_url(&config.webhook_url, "webhookUrl")?
        };

        let http = pinned_client(&url, Duration::from_secs(config.timeout_secs)).await?;

        let buttons = AlertButtons::read(message).map(|buttons| AlertButtons {
            actions: Vec::new(),
            callback: None,
            ..buttons
        });

        let post = mattermost_post(message, buttons.as_ref(), None);
        let mut body = json!({ "text": post.message });

        if !post.attachments.is_empty() {
            body["attachments"] = Value::Array(post.attachments);
        }

        let started = Instant::now();

        // The address carries the webhook's key: errors never repeat it.
        let response = http_trace::send(&http, http.post(url).json(&body), HttpTarget::ThirdParty)
            .await
            .map_err(|_| Error::unavailable(format!("{CONNECTOR} request failed")))?;

        check(response, CONNECTOR, "posting the message").await?;
        Ok(delivered(None, started))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use domain::notify::{
        buttons::AlertButtonLabels,
        template::{NotifyCard, NotifyCardField},
    };
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_json, method, path},
    };

    use super::*;

    fn target(target_type: NotifyTargetType, value: &str) -> NotifyTarget {
        NotifyTarget {
            target_type,
            value: value.into(),
            metadata: BTreeMap::new(),
        }
    }

    fn message() -> NotifyMessage {
        NotifyMessage {
            title: "CRITICAL · Disk full".into(),
            text: "Disk is at 97%.".into(),
            markdown: None,
            html: None,
            metadata: BTreeMap::from([("severity".to_string(), "critical".to_string())]),
            provider_templates: Default::default(),
            event: None,
            card: None,
        }
    }

    #[test]
    fn the_webhook_is_a_hooks_address() {
        let adapter = MattermostWebhookConnectorAdapter::new();

        assert!(adapter.addresses().is_empty());
        assert!(!adapter.capabilities().interactive);
        adapter
            .validate_config(&json!({ "webhookUrl": "https://chat.example.com/hooks/abc123" }))
            .unwrap();
        adapter
            .validate_config(&json!({
                "webhookUrl": "http://intranet.example.com/mattermost/hooks/abc123",
                "timeoutSecs": 20,
            }))
            .unwrap();

        for config in [
            json!({}),
            json!({ "webhookUrl": "https://chat.example.com" }),
            json!({ "webhookUrl": "https://chat.example.com/hooks/" }),
            json!({ "webhookUrl": "ftp://chat.example.com/hooks/abc" }),
            json!({ "webhookUrl": "https://chat.example.com/hooks/abc", "timeoutSecs": 61 }),
        ] {
            assert!(adapter.validate_config(&config).is_err(), "{config}");
        }

        adapter
            .validate_target(&target(NotifyTargetType::FixedGroup, "town-square"))
            .unwrap();
        adapter
            .validate_target(&target(
                NotifyTargetType::Webhook,
                "https://chat.example.com/hooks/other",
            ))
            .unwrap();
        assert!(
            adapter
                .validate_target(&target(NotifyTargetType::DirectUser, "alice"))
                .is_err()
        );
        assert!(
            adapter
                .validate_target(&target(NotifyTargetType::Webhook, "https://x.example.com/"))
                .is_err()
        );
    }

    #[tokio::test]
    async fn a_card_goes_to_the_webhook_as_an_attachment_without_buttons() {
        let server = MockServer::start().await;

        let mut alert = message();
        alert.card = Some(NotifyCard {
            fields: vec![NotifyCardField {
                label: "Host".into(),
                value: "db-1".into(),
            }],
            ..NotifyCard::default()
        });

        let labels = AlertButtonLabels {
            acknowledge: "Acknowledge".into(),
            resolve: "Resolve".into(),
            snooze: "Snooze 1h".into(),
            acknowledge_all: "Acknowledge all".into(),
            approve: "Approve".into(),
            reject: "Reject".into(),
            open: Some(("Open in MoleSignal".into(), "https://ms.example.com".into())),
            callback: Some("https://ms.example.com/callback".into()),
        };
        AlertButtons::new("inc-1", "open", "alert.triggered", None, &labels).write(&mut alert);

        Mock::given(method("POST"))
            .and(path("/hooks/abc123"))
            .and(body_json(json!({
                "text": "",
                "attachments": [{
                    "fallback": "CRITICAL · Disk full",
                    "color": "#D93025",
                    "title": "CRITICAL · Disk full",
                    "title_link": "https://ms.example.com/alerts/incidents/inc-1",
                    "text": "Disk is at 97%.",
                    "fields": [{ "short": true, "title": "Host", "value": "db-1" }],
                }],
            })))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .expect(1)
            .mount(&server)
            .await;

        let config = json!({ "webhookUrl": format!("{}/hooks/abc123", server.uri()) });

        let result = MattermostWebhookConnectorAdapter::new()
            .send(
                &config,
                &target(NotifyTargetType::FixedGroup, "alerts"),
                &alert,
            )
            .await
            .unwrap();

        assert!(result.delivered);
        assert_eq!(result.provider_message_id, None);
    }

    #[tokio::test]
    async fn a_webhook_target_overrides_the_configured_one_and_refusals_are_explained() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/hooks/team-b"))
            .and(body_json(
                json!({ "text": "**CRITICAL · Disk full**\nDisk is at 97%." }),
            ))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "id": "web.incoming_webhook.disabled.app_error",
                "message": "Incoming webhooks have been disabled by the system admin.",
                "status_code": 400,
            })))
            .expect(1)
            .mount(&server)
            .await;

        let config = json!({ "webhookUrl": format!("{}/hooks/team-a", server.uri()) });
        let other = format!("{}/hooks/team-b", server.uri());

        let error = MattermostWebhookConnectorAdapter::new()
            .send(
                &config,
                &target(NotifyTargetType::Webhook, &other),
                &message(),
            )
            .await
            .unwrap_err();

        assert!(matches!(error, Error::Unavailable(_)), "{error:?}");
        assert!(
            error
                .to_string()
                .contains("HTTP 400 Incoming webhooks have been disabled"),
            "{error}"
        );
        assert!(!error.to_string().contains("team-b"), "{error}");
    }
}
