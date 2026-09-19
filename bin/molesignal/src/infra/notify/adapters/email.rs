// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{collections::BTreeMap, time::Instant};

use async_trait::async_trait;
use lettre::message::Mailbox;
use serde::Deserialize;
use serde_json::Value;

use super::super::EmailSender;
use crate::{
    config::{SmtpSettings, SmtpTls},
    domain::notify::connector::{
        ConnectorAdapter, ConnectorCapabilities, ConnectorDeliveryResult, NotifyMessage,
        NotifyTarget, NotifyTargetType,
    },
    shared::{Error, Result},
};

pub const EMAIL_SMTP_CONNECTOR_TYPE: &str = "email_smtp";

#[derive(Debug, Clone, Deserialize)]
struct EmailSmtpConfig {
    host: String,
    #[serde(default)]
    port: u16,
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
    from: String,
    #[serde(default)]
    tls: SmtpTls,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u32,
}

fn default_timeout_secs() -> u32 {
    10
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
        Ok(())
    }

    fn into_settings(self) -> SmtpSettings {
        SmtpSettings {
            host: self.host,
            port: self.port,
            username: self.username,
            password: self.password,
            from: self.from,
            tls: self.tls,
            timeout_secs: self.timeout_secs,
        }
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

    fn validate_config(&self, config: &Value) -> Result<()> {
        EmailSmtpConfig::parse(config)?.validate()
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
        let sender = EmailSender::new(&config.into_settings())?;
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
        sender
            .send_message(
                &[target.value.trim().to_string()],
                &[],
                &[],
                None,
                &message.title,
                body,
                html,
            )
            .await?;
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
    use super::*;

    #[test]
    fn validates_config_and_target_without_network_io() {
        let adapter = EmailSmtpConnectorAdapter::new();
        adapter
            .validate_config(&serde_json::json!({
                "host": "smtp.example.com",
                "port": 587,
                "username": "mailer",
                "password": "secret",
                "from": "alerts@example.com",
                "tls": "starttls",
                "timeout_secs": 10
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
                .validate_config(&serde_json::json!({
                    "host": "smtp.example.com",
                    "from": "alerts@example.com",
                    "timeout_secs": 0
                }))
                .is_err()
        );
    }
}
