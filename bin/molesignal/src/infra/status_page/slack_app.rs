// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The Slack apps visitors add a status page to their channels with: Slack
//! App message connectors that carry a client ID and a client secret, kept
//! encrypted with the connector. The OAuth flow adds an incoming webhook to the
//! channel a visitor picks; the redirect URL to register with Slack is
//! `<http.external_url>/api/v1/public/slack/callback`.

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use url::Url;

use crate::{
    domain::{
        notify::repositories::NotifyConnectorRepository,
        status_page::{
            SlackAppConnectorChoice, SlackIncomingWebhook, StatusPageSlackApp, StatusPageSlackApps,
        },
    },
    infra::notify::adapters::SLACK_APP_CONNECTOR_TYPE,
    shared::{
        Error, Result,
        http_trace::{self, HttpTarget},
        ids::Id,
    },
};

const AUTHORIZE_URL: &str = "https://slack.com/oauth/v2/authorize";
const ACCESS_URL: &str = "https://slack.com/api/oauth.v2.access";
pub const SLACK_CALLBACK_PATH: &str = "/api/v1/public/slack/callback";

pub struct SlackOAuthApp {
    client_id: String,
    client_secret: String,
    redirect_uri: String,
}

impl SlackOAuthApp {
    /// The app with these credentials. The callback lives on the public
    /// address of the installation, which must be an HTTPS address.
    pub fn new(client_id: &str, client_secret: &str, external_url: &str) -> Result<Self> {
        let (client_id, client_secret) = (client_id.trim(), client_secret.trim());

        if client_id.is_empty() || client_secret.is_empty() {
            return Err(Error::invalid(
                "the Slack App connector needs a client ID and a client secret to add channels",
            ));
        }

        let redirect_uri = redirect_url(external_url).ok_or_else(|| {
            Error::unavailable(
                "adding Slack channels needs http.external_url to be an HTTPS address",
            )
        })?;

        Ok(Self {
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            redirect_uri,
        })
    }
}

/// Where Slack sends visitors back: the callback on the public address of the
/// installation, which must be an HTTPS address.
fn redirect_url(external_url: &str) -> Option<String> {
    let base = Url::parse(external_url.trim())
        .ok()
        .filter(|url| url.scheme() == "https")?;

    base.join(SLACK_CALLBACK_PATH).ok().map(String::from)
}

fn config_text<'a>(config: &'a Value, key: &str) -> &'a str {
    config
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
}

/// Slack apps read from the Slack App message connectors of an organization.
pub struct ConnectorSlackApps {
    connectors: Arc<dyn NotifyConnectorRepository>,
    external_url: String,
}

impl ConnectorSlackApps {
    pub fn new(connectors: Arc<dyn NotifyConnectorRepository>, external_url: String) -> Self {
        Self {
            connectors,
            external_url,
        }
    }
}

#[async_trait]
impl StatusPageSlackApps for ConnectorSlackApps {
    async fn slack_app(
        &self,
        org_id: &Id,
        connector_id: &Id,
    ) -> Result<Arc<dyn StatusPageSlackApp>> {
        let connector = self.connectors.get(org_id, connector_id).await?;

        if connector.connector_type != SLACK_APP_CONNECTOR_TYPE {
            return Err(Error::invalid("the connector is not a Slack App connector"));
        }

        if !connector.enabled {
            return Err(Error::conflict("the Slack App connector is turned off"));
        }

        Ok(Arc::new(SlackOAuthApp::new(
            config_text(&connector.config, "client_id"),
            config_text(&connector.config, "client_secret"),
            &self.external_url,
        )?))
    }

    async fn connectors(&self, org_id: &Id) -> Result<Vec<SlackAppConnectorChoice>> {
        let connectors = self.connectors.list(org_id).await?;

        Ok(connectors
            .into_iter()
            .filter(|connector| connector.connector_type == SLACK_APP_CONNECTOR_TYPE)
            .map(|connector| SlackAppConnectorChoice {
                adds_channels: !config_text(&connector.config, "client_id").is_empty()
                    && !config_text(&connector.config, "client_secret").is_empty(),
                id: connector.id,
                name: connector.name,
                enabled: connector.enabled,
            })
            .collect())
    }

    fn redirect_url(&self) -> Option<String> {
        redirect_url(&self.external_url)
    }
}

#[derive(Debug, Deserialize)]
struct AccessResponse {
    ok: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    incoming_webhook: Option<IncomingWebhook>,
    #[serde(default)]
    team: Option<Team>,
}

#[derive(Debug, Deserialize)]
struct IncomingWebhook {
    url: String,
    #[serde(default)]
    channel: String,
}

#[derive(Debug, Deserialize)]
struct Team {
    #[serde(default)]
    name: String,
}

#[async_trait]
impl StatusPageSlackApp for SlackOAuthApp {
    fn authorize_url(&self, state: &str) -> Result<String> {
        let mut url = Url::parse(AUTHORIZE_URL)
            .map_err(|_| Error::internal("the Slack authorization address is invalid"))?;

        url.query_pairs_mut()
            .append_pair("client_id", &self.client_id)
            .append_pair("scope", "incoming-webhook")
            .append_pair("redirect_uri", &self.redirect_uri)
            .append_pair("state", state);

        Ok(url.to_string())
    }

    async fn exchange_code(&self, code: &str) -> Result<SlackIncomingWebhook> {
        let code = code.trim();

        if code.is_empty() || code.len() > 512 {
            return Err(Error::invalid("Slack sent back no usable code"));
        }

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| Error::internal("Slack HTTP client could not be initialized"))?;

        let response = http_trace::send(
            &client,
            client.post(ACCESS_URL).form(&[
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
                ("code", code),
                ("redirect_uri", self.redirect_uri.as_str()),
            ]),
            HttpTarget::ThirdParty,
        )
        .await
        .map_err(|_| Error::unavailable("Slack could not be reached"))?;

        if !response.status().is_success() {
            return Err(Error::unavailable(format!(
                "Slack returned HTTP {}",
                response.status().as_u16()
            )));
        }

        let access: AccessResponse = response
            .json()
            .await
            .map_err(|_| Error::unavailable("Slack returned an invalid response"))?;

        incoming_webhook(access)
    }
}

fn incoming_webhook(access: AccessResponse) -> Result<SlackIncomingWebhook> {
    if !access.ok {
        return Err(Error::unavailable(format!(
            "Slack refused the request: {}",
            access.error.as_deref().unwrap_or("unknown_error")
        )));
    }

    let webhook = access
        .incoming_webhook
        .filter(|webhook| webhook.url.starts_with("https://hooks.slack"))
        .ok_or_else(|| Error::unavailable("Slack returned no incoming webhook"))?;

    Ok(SlackIncomingWebhook {
        url: webhook.url,
        channel: webhook.channel,
        team: access.team.map(|team| team.name).unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{
        domain::notify::connector::{
            ConnectorCapabilities, ConnectorStatus, ConnectorTestStatus, NotifyConnector,
        },
        shared::time::TimestampMicros,
    };

    struct Connectors(Vec<NotifyConnector>);

    #[async_trait]
    impl NotifyConnectorRepository for Connectors {
        async fn create(&self, _: NotifyConnector) -> Result<NotifyConnector> {
            unreachable!("only read here")
        }

        async fn update(&self, _: NotifyConnector) -> Result<NotifyConnector> {
            unreachable!("only read here")
        }

        async fn get(&self, organization_id: &Id, id: &Id) -> Result<NotifyConnector> {
            self.0
                .iter()
                .find(|connector| {
                    &connector.organization_id == organization_id && &connector.id == id
                })
                .cloned()
                .ok_or_else(|| Error::not_found("notify connector not found"))
        }

        async fn list(&self, organization_id: &Id) -> Result<Vec<NotifyConnector>> {
            Ok(self
                .0
                .iter()
                .filter(|connector| &connector.organization_id == organization_id)
                .cloned()
                .collect())
        }

        async fn record_test_result(
            &self,
            _: &Id,
            _: &Id,
            _: TimestampMicros,
            _: ConnectorTestStatus,
            _: Option<String>,
        ) -> Result<NotifyConnector> {
            unreachable!("only read here")
        }

        async fn delete(&self, _: &Id, _: &Id) -> Result<()> {
            unreachable!("only read here")
        }
    }

    fn connector(id: &str, connector_type: &str, enabled: bool, config: Value) -> NotifyConnector {
        NotifyConnector {
            id: Id(id.into()),
            organization_id: Id("org".into()),
            name: id.into(),
            connector_type: connector_type.into(),
            config,
            capabilities: ConnectorCapabilities::default(),
            enabled,
            status: ConnectorStatus::Unknown,
            last_tested_at: None,
            last_test_status: None,
            last_test_error: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    #[tokio::test]
    async fn a_page_adds_channels_with_a_slack_app_connector_that_holds_client_credentials() {
        let credentials = json!({"client_id": "1.2", "client_secret": "secret"});

        let apps = ConnectorSlackApps::new(
            Arc::new(Connectors(vec![
                connector("app", SLACK_APP_CONNECTOR_TYPE, true, credentials.clone()),
                connector("off", SLACK_APP_CONNECTOR_TYPE, false, credentials.clone()),
                connector(
                    "bot",
                    SLACK_APP_CONNECTOR_TYPE,
                    true,
                    json!({"bot_token": "xoxb"}),
                ),
                connector("hook", "slack_webhook", true, credentials),
            ])),
            "https://ms.example.com".into(),
        );

        let org = Id("org".into());

        let authorize = apps
            .slack_app(&org, &Id("app".into()))
            .await
            .unwrap()
            .authorize_url("state.token")
            .unwrap();

        assert!(authorize.contains("client_id=1.2"));

        for refused in ["off", "bot", "hook", "missing"] {
            assert!(
                apps.slack_app(&org, &Id(refused.into())).await.is_err(),
                "{refused}"
            );
        }

        let choices = apps.connectors(&org).await.unwrap();

        assert_eq!(
            choices
                .iter()
                .map(|choice| (choice.id.as_str(), choice.enabled, choice.adds_channels))
                .collect::<Vec<_>>(),
            [
                ("app", true, true),
                ("off", false, true),
                ("bot", true, false)
            ]
        );

        assert_eq!(
            apps.redirect_url().as_deref(),
            Some("https://ms.example.com/api/v1/public/slack/callback")
        );
    }

    #[test]
    fn the_app_needs_both_credentials_and_an_https_address() {
        assert!(SlackOAuthApp::new("", "", "https://ms.example.com").is_err());
        assert!(SlackOAuthApp::new("id", "", "https://ms.example.com").is_err());
        assert!(SlackOAuthApp::new("id", "secret", "http://ms.example.com").is_err());

        let app = SlackOAuthApp::new("id", "secret", "https://ms.example.com/").unwrap();
        assert_eq!(
            app.redirect_uri,
            "https://ms.example.com/api/v1/public/slack/callback"
        );

        let authorize = app.authorize_url("state.token").unwrap();
        assert!(authorize.starts_with("https://slack.com/oauth/v2/authorize?client_id=id"));
        assert!(authorize.contains("scope=incoming-webhook"));
        assert!(authorize.contains("state=state.token"));
    }

    #[test]
    fn a_refusal_or_a_foreign_webhook_is_not_taken() {
        let refused: AccessResponse =
            serde_json::from_str(r#"{"ok":false,"error":"invalid_code"}"#).unwrap();
        assert!(incoming_webhook(refused).is_err());

        let foreign: AccessResponse = serde_json::from_str(
            r##"{"ok":true,"incoming_webhook":{"url":"https://evil.example/x","channel":"#ops"}}"##,
        )
        .unwrap();
        assert!(incoming_webhook(foreign).is_err());

        let added: AccessResponse = serde_json::from_str(
            r##"{"ok":true,"team":{"name":"Acme"},"incoming_webhook":{"url":"https://hooks.slack.com/services/T/B/C","channel":"#ops"}}"##,
        )
        .unwrap();
        let webhook = incoming_webhook(added).unwrap();
        assert_eq!(webhook.channel, "#ops");
        assert_eq!(webhook.team, "Acme");
    }
}
