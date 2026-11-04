// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Twilio 短信：建一条 `Messages`。发送方可以是 Twilio 号码、Messaging Service（MG…）或字母
//! 发送方 ID（只在支持它的国家可用）。

use std::time::Instant;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use super::{Account, create, is_sid};
use crate::{
    domain::notify::connector::{
        ConnectorAdapter, ConnectorAddress, ConnectorCapabilities, ConnectorDeliveryResult,
        NotifyChannel, NotifyMessage, NotifyTarget,
    },
    infra::notify::adapters::{common, phone},
    shared::{Error, Result},
};

pub const TWILIO_SMS_CONNECTOR_TYPE: &str = "twilio_sms";

#[derive(Debug, Clone, Deserialize)]
struct SmsConfig {
    #[serde(flatten)]
    account: Account,
    #[serde(default)]
    sender: String,
}

/// 短信从哪里发出：表单里对应的字段与值。
fn sender(value: &str) -> Result<(&'static str, String)> {
    let from = value.trim();

    if is_sid(from, "MG") {
        return Ok(("MessagingServiceSid", from.to_string()));
    }

    if from.starts_with('+') || from.starts_with("00") {
        return Ok(("From", phone::e164(from)?));
    }

    let alphanumeric = (1..=11).contains(&from.len())
        && from.chars().all(|c| c.is_ascii_alphanumeric() || c == ' ')
        && from.chars().any(|c| c.is_ascii_alphabetic());

    if alphanumeric {
        return Ok(("From", from.to_string()));
    }

    Err(Error::invalid(
        "twilio_sms sender must be a Twilio phone number, a Messaging Service SID (MG…) or a sender ID of up to 11 letters and digits",
    ))
}

impl SmsConfig {
    fn parse(value: &Value) -> Result<Self> {
        serde_json::from_value(value.clone()).map_err(|error| {
            Error::invalid(format!(
                "invalid {TWILIO_SMS_CONNECTOR_TYPE} config: {error}"
            ))
        })
    }

    fn validate(&self) -> Result<()> {
        self.account.validate(TWILIO_SMS_CONNECTOR_TYPE)?;
        sender(&self.sender).map(|_| ())
    }
}

#[derive(Default)]
pub struct TwilioSmsConnectorAdapter;

impl TwilioSmsConnectorAdapter {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ConnectorAdapter for TwilioSmsConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        TWILIO_SMS_CONNECTOR_TYPE
    }

    fn capabilities(&self) -> ConnectorCapabilities {
        ConnectorCapabilities {
            direct_user: true,
            ..ConnectorCapabilities::default()
        }
    }

    fn channel(&self) -> NotifyChannel {
        NotifyChannel::Sms
    }

    fn takes_replies(&self, config: &Value) -> bool {
        super::super::phone::replies_on(config)
    }

    fn addresses(&self) -> Vec<ConnectorAddress> {
        vec![super::address()]
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        SmsConfig::parse(config)?.validate()
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        phone::target_number(target, TWILIO_SMS_CONNECTOR_TYPE, phone::e164).map(|_| ())
    }

    async fn send(
        &self,
        config: &Value,
        target: &NotifyTarget,
        message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        let config = SmsConfig::parse(config)?;
        config.validate()?;

        let to = phone::target_number(target, TWILIO_SMS_CONNECTOR_TYPE, phone::e164)?;
        let (field, from) = sender(&config.sender)?;
        let started = Instant::now();

        let sid = create(
            TWILIO_SMS_CONNECTOR_TYPE,
            &config.account,
            "Messages",
            &[("To", to), (field, from), ("Body", phone::texted(message))],
        )
        .await?;

        Ok(common::delivered(sid, started))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_string_contains, method, path},
    };

    use super::{super::tests::SID, *};
    use crate::domain::notify::connector::NotifyTargetType;

    #[test]
    fn the_sender_is_a_number_a_service_or_a_sender_id() {
        assert_eq!(
            sender("+14155550100").unwrap(),
            ("From", "+14155550100".to_string())
        );
        assert_eq!(
            sender("MG0123456789abcdef0123456789abcdef").unwrap().0,
            "MessagingServiceSid"
        );
        assert_eq!(
            sender("MoleSignal").unwrap(),
            ("From", "MoleSignal".to_string())
        );
        assert!(sender("").is_err());
        assert!(sender("Mole Signal Alerts").is_err());
    }

    #[tokio::test]
    async fn a_refused_text_says_why() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path(format!("/2010-04-01/Accounts/{SID}/Messages.json")))
            .and(body_string_contains("MessagingServiceSid=MG"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "code": 21211,
                "message": "The 'To' number is not a valid phone number.",
                "status": 400,
            })))
            .mount(&server)
            .await;

        let config = json!({
            "accountSid": SID,
            "authToken": "token",
            "sender": "MG0123456789abcdef0123456789abcdef",
            "apiBaseUrl": server.uri(),
        });

        let target = NotifyTarget {
            target_type: NotifyTargetType::FixedAddress,
            value: "+14155550123".into(),
            metadata: BTreeMap::new(),
        };

        let message = NotifyMessage {
            title: "Disk full".into(),
            text: "db-1 has 1% left.".into(),
            markdown: None,
            html: None,
            metadata: BTreeMap::new(),
            provider_templates: Default::default(),
            event: None,
            card: None,
        };

        let error = TwilioSmsConnectorAdapter::new()
            .send(&config, &target, &message)
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("HTTP 400 21211"), "{error}");
    }
}
