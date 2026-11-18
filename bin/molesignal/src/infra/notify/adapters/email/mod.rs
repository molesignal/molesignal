// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Email over SMTP. The server is signed in to with a password, or with OAuth 2.0 (SASL
//! XOAUTH2) as Microsoft 365 or Google Workspace (see [`oauth`]).

mod oauth;

use std::{collections::BTreeMap, time::Instant};

use async_trait::async_trait;
use domain::notify::connector::{
    ConnectorAdapter, ConnectorAddress, ConnectorCapabilities, ConnectorDeliveryResult,
    NotifyChannel, NotifyMessage, NotifyTarget, NotifyTargetType,
};
use kernel::{Error, Result};
use lettre::message::Mailbox;
use serde::Deserialize;
use serde_json::Value;
use settings::{SmtpSettings, SmtpTls};

use self::oauth::MICROSOFT_LOGIN_BASE_URL;
use super::{super::EmailSender, google_account::ServiceAccount};

pub const EMAIL_SMTP_CONNECTOR_TYPE: &str = "email_smtp";

/// How the connector signs in to the SMTP server.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SmtpAuth {
    /// A user name and password, or none for an open relay.
    #[default]
    Password,
    /// An app registered in Microsoft Entra ID, for Exchange Online.
    Microsoft,
    /// A Google Workspace service account with domain-wide delegation.
    Google,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EmailSmtpConfig {
    host: String,
    #[serde(default)]
    port: u16,
    #[serde(default)]
    auth: SmtpAuth,
    /// The SMTP user; for the OAuth modes, the mailbox that sends.
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
    from: String,
    #[serde(default)]
    tls: SmtpTls,
    #[serde(default)]
    tenant_id: String,
    #[serde(default)]
    client_id: String,
    #[serde(default)]
    client_secret: String,
    #[serde(default = "default_login_base_url")]
    login_base_url: String,
    /// The Google service account key: a JSON object, or its text.
    #[serde(default)]
    credentials: Value,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u32,
}

fn default_timeout_secs() -> u32 {
    10
}

fn default_login_base_url() -> String {
    MICROSOFT_LOGIN_BASE_URL.into()
}

impl EmailSmtpConfig {
    fn parse(value: &Value) -> Result<Self> {
        serde_json::from_value(value.clone())
            .map_err(|error| Error::invalid(format!("invalid email_smtp config: {error}")))
    }

    fn validate(&self) -> Result<()> {
        if self.host.trim().is_empty() {
            return Err(Error::invalid("email_smtp host cannot be empty"));
        }

        if self.from.trim().is_empty() {
            return Err(Error::invalid("email_smtp from cannot be empty"));
        }

        self.from
            .parse::<Mailbox>()
            .map_err(|_| Error::invalid("email_smtp from must be a valid email address"))?;

        if !(1..=60).contains(&self.timeout_secs) {
            return Err(Error::invalid(
                "email_smtp timeout_secs must be between 1 and 60",
            ));
        }

        match self.auth {
            SmtpAuth::Password => Ok(()),
            SmtpAuth::Microsoft => {
                self.validate_mailbox()?;

                for (value, field) in [
                    (&self.tenant_id, "tenantId"),
                    (&self.client_id, "clientId"),
                    (&self.client_secret, "clientSecret"),
                ] {
                    if value.trim().is_empty() {
                        return Err(Error::invalid(format!(
                            "email_smtp {field} is required to sign in with Microsoft"
                        )));
                    }
                }

                Ok(())
            }
            SmtpAuth::Google => {
                self.validate_mailbox()?;
                ServiceAccount::parse(&self.credentials, EMAIL_SMTP_CONNECTOR_TYPE)?;
                Ok(())
            }
        }
    }

    /// The OAuth modes sign in as a mailbox: the username is its address.
    fn validate_mailbox(&self) -> Result<()> {
        self.username.trim().parse::<Mailbox>().map(|_| ()).map_err(|_| {
            Error::invalid(
                "email_smtp username must be the sending mailbox's address to sign in with OAuth",
            )
        })
    }

    fn settings(&self) -> SmtpSettings {
        SmtpSettings {
            host: self.host.clone(),
            port: self.port,
            username: self.username.clone(),
            password: self.password.clone(),
            from: self.from.clone(),
            tls: self.tls.clone(),
            timeout_secs: self.timeout_secs,
        }
    }

    /// The sender, signed in the configured way.
    async fn sender(&self) -> Result<EmailSender> {
        let token = match self.auth {
            SmtpAuth::Password => return EmailSender::new(&self.settings()),
            SmtpAuth::Microsoft => oauth::microsoft_token(self).await?,
            SmtpAuth::Google => oauth::google_token(self).await?,
        };

        EmailSender::with_xoauth2(&self.settings(), &token)
    }
}

#[derive(Default)]
pub struct EmailSmtpConnectorAdapter;

impl EmailSmtpConnectorAdapter {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ConnectorAdapter for EmailSmtpConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        EMAIL_SMTP_CONNECTOR_TYPE
    }

    fn capabilities(&self) -> ConnectorCapabilities {
        ConnectorCapabilities {
            direct_user: true,
            group: true,
            rich_text: true,
            interactive: false,
            acknowledgement: false,
            attachments: false,
        }
    }

    fn channel(&self) -> NotifyChannel {
        NotifyChannel::Email
    }

    /// Microsoft's sign-in address. The SMTP host itself is entered by people.
    fn addresses(&self) -> Vec<ConnectorAddress> {
        vec![ConnectorAddress::fixed(
            "loginBaseUrl",
            MICROSOFT_LOGIN_BASE_URL,
        )]
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        let config = EmailSmtpConfig::parse(config)?;
        config.validate()?;

        if config.auth == SmtpAuth::Google {
            ServiceAccount::parse(&config.credentials, EMAIL_SMTP_CONNECTOR_TYPE)?
                .validate_token_uri(EMAIL_SMTP_CONNECTOR_TYPE)?;
        }

        Ok(())
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        if !matches!(
            target.target_type,
            NotifyTargetType::DirectUser | NotifyTargetType::FixedAddress
        ) {
            return Err(Error::invalid(
                "email_smtp target_type must be direct_user or fixed_address",
            ));
        }

        target
            .value
            .trim()
            .parse::<Mailbox>()
            .map_err(|_| Error::invalid("email_smtp target must be a valid email address"))?;

        Ok(())
    }

    async fn send(
        &self,
        config: &Value,
        target: &NotifyTarget,
        message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        self.validate_target(target)?;
        let config = EmailSmtpConfig::parse(config)?;
        config.validate()?;

        let (body, html) = match message.html.as_deref() {
            Some(html) if !html.trim().is_empty() => (html, true),
            _ => (
                message
                    .markdown
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or(&message.text),
                false,
            ),
        };

        let started = Instant::now();
        let sender = config.sender().await?;

        let sent = sender
            .send_message(
                &[target.value.trim().to_string()],
                &[],
                &[],
                None,
                &message.title,
                body,
                html,
            )
            .await;

        // A refused token is signed in again next time instead of being reused until it expires.
        if sent.is_err() && config.auth != SmtpAuth::Password {
            oauth::forget(&config);
        }

        sent?;

        Ok(ConnectorDeliveryResult {
            provider_message_id: None,
            delivered: true,
            latency_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            metadata: BTreeMap::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::infra::notify::adapters::google_account::test_key;

    fn mailbox_config(auth: &str) -> Value {
        json!({
            "host": "smtp.office365.com",
            "port": 587,
            "auth": auth,
            "username": "alerts@example.com",
            "from": "alerts@example.com",
            "tls": "starttls",
        })
    }

    #[test]
    fn validates_config_and_target_without_network_io() {
        let adapter = EmailSmtpConnectorAdapter::new();
        adapter
            .validate_config(&json!({
                "host": "smtp.example.com",
                "port": 587,
                "username": "mailer",
                "password": "secret",
                "from": "alerts@example.com",
                "tls": "starttls",
                "timeoutSecs": 10
            }))
            .unwrap();
        adapter
            .validate_target(&NotifyTarget {
                target_type: NotifyTargetType::DirectUser,
                value: "user@example.com".into(),
                metadata: BTreeMap::new(),
            })
            .unwrap();
    }

    #[test]
    fn rejects_non_email_target_and_bad_timeout() {
        let adapter = EmailSmtpConnectorAdapter::new();
        assert!(
            adapter
                .validate_target(&NotifyTarget {
                    target_type: NotifyTargetType::FixedGroup,
                    value: "#ops".into(),
                    metadata: BTreeMap::new(),
                })
                .is_err()
        );
        assert!(
            adapter
                .validate_config(&json!({
                    "host": "smtp.example.com",
                    "from": "alerts@example.com",
                    "timeoutSecs": 0
                }))
                .is_err()
        );
    }

    #[test]
    fn microsoft_sign_in_needs_the_app_and_the_mailbox() {
        let adapter = EmailSmtpConnectorAdapter::new();
        let mut config = mailbox_config("microsoft");

        assert!(adapter.validate_config(&config).is_err());

        config["tenantId"] = json!("contoso.onmicrosoft.com");
        config["clientId"] = json!("00000000-0000-0000-0000-000000000001");
        config["clientSecret"] = json!("secret");
        adapter.validate_config(&config).unwrap();

        config["username"] = json!("mailer");
        let refused = adapter.validate_config(&config).unwrap_err();
        assert!(refused.to_string().contains("username"));
    }

    #[test]
    fn google_sign_in_needs_a_service_account_key_from_google() {
        let adapter = EmailSmtpConnectorAdapter::new();
        let mut config = mailbox_config("google");

        assert!(adapter.validate_config(&config).is_err());

        config["credentials"] = json!({
            "client_email": "mailer@project.iam.gserviceaccount.com",
            "private_key": test_key(),
        });
        adapter.validate_config(&config).unwrap();

        config["credentials"]["token_uri"] = json!("https://attacker.example/token");
        assert!(adapter.validate_config(&config).is_err());
    }

    #[test]
    fn the_microsoft_sign_in_address_comes_from_the_catalogue() {
        let addresses = EmailSmtpConnectorAdapter::new().addresses();
        let admitted = |url: &str| {
            domain::notify::connector::validate_addresses(
                EMAIL_SMTP_CONNECTOR_TYPE,
                &addresses,
                &json!({ "loginBaseUrl": url }),
            )
            .is_ok()
        };

        assert!(admitted("https://login.microsoftonline.com/"));
        assert!(!admitted("https://login.partner.microsoftonline.cn"));
        assert!(!admitted("https://login.attacker.example"));
    }
}
