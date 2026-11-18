// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! WhatsApp through Twilio: a `Messages` resource sent from a WhatsApp sender. A business may
//! write to a person on WhatsApp first only with a template WhatsApp approved, so this connector
//! sends nothing else: the SMS notify template carries a [`PROVIDER`] block whose code is the
//! template's Twilio Content SID (`HX…`) and whose variables (`1`, `2`, …) say what fills each of
//! its placeholders. Replies reach the same address as Twilio SMS replies.

use std::time::Instant;

use async_trait::async_trait;
use domain::notify::{
    connector::{
        ConnectorAdapter, ConnectorAddress, ConnectorCapabilities, ConnectorDeliveryResult,
        NotifyChannel, NotifyMessage, NotifyTarget,
    },
    template::ProviderTemplate,
};
use kernel::{Error, Result};
use serde::Deserialize;
use serde_json::{Map, Value};

use super::{Account, create, is_sid};
use crate::infra::notify::adapters::{common, phone};

pub const TWILIO_WHATSAPP_CONNECTOR_TYPE: &str = "twilio_whatsapp";

/// The notify template's block for this connector goes by this name.
const PROVIDER: &str = "whatsapp";

/// WhatsApp caps a template's body at 1,024 characters with its variables filled in: a variable is
/// cut at this many characters unless the notify template sets its own maximum.
const DEFAULT_MAX_PARAM_CHARS: u32 = 300;

/// The channel prefix Twilio writes before a WhatsApp number.
const CHANNEL_PREFIX: &str = "whatsapp:";

#[derive(Debug, Clone, Deserialize)]
struct WhatsappConfig {
    #[serde(flatten)]
    account: Account,
    #[serde(default)]
    sender: String,
}

impl WhatsappConfig {
    fn parse(value: &Value) -> Result<Self> {
        serde_json::from_value(value.clone()).map_err(|error| {
            Error::invalid(format!(
                "invalid {TWILIO_WHATSAPP_CONNECTOR_TYPE} config: {error}"
            ))
        })
    }

    fn validate(&self) -> Result<()> {
        self.account.validate(TWILIO_WHATSAPP_CONNECTOR_TYPE)?;
        sender(&self.sender).map(|_| ())
    }
}

/// Where a message is sent from: the form field and its value. A WhatsApp sender's number goes in
/// `From` with the channel prefix; a Messaging Service picks a WhatsApp sender from its pool.
fn sender(value: &str) -> Result<(&'static str, String)> {
    let from = without_prefix(value);

    if is_sid(from, "MG") {
        return Ok(("MessagingServiceSid", from.to_string()));
    }

    match phone::e164(from) {
        Ok(number) => Ok(("From", format!("{CHANNEL_PREFIX}{number}"))),
        Err(_) => Err(Error::invalid(
            "twilio_whatsapp sender must be the number of a WhatsApp sender, like +14155550100, or a Messaging Service SID (MG…)",
        )),
    }
}

/// A number written with or without Twilio's channel prefix.
fn without_prefix(value: &str) -> &str {
    let value = value.trim();

    value.strip_prefix(CHANNEL_PREFIX).unwrap_or(value).trim()
}

/// A person is reached on WhatsApp at their phone number, in E.164.
fn number(value: &str) -> Result<String> {
    phone::e164(without_prefix(value))
}

/// The approved template the notify template names for WhatsApp: without one nothing is sent.
fn approved_template(message: &NotifyMessage) -> Result<&ProviderTemplate> {
    let template = message
        .provider_templates
        .get(PROVIDER)
        .filter(|template| !template.code.trim().is_empty())
        .ok_or_else(|| {
            Error::invalid(
                "twilio_whatsapp sends only templates WhatsApp approved: give the SMS template of \
                 the notify policy a WhatsApp template with its Twilio Content SID (HX…)",
            )
        })?;

    let code = template.code.trim();

    if !is_sid(code, "HX") {
        return Err(Error::invalid(format!(
            "twilio_whatsapp template code must be a Twilio Content SID (HX…), not {code}"
        )));
    }

    Ok(template)
}

/// `ContentVariables`: the template's variables as JSON, or `None` when it has none. WhatsApp
/// refuses a variable that is empty, holds a line break or a tab, or runs more than four spaces:
/// Markdown marks are dropped, every run of white space becomes one space, a long value is cut,
/// and one that fills to nothing is sent as a dash.
fn content_variables(template: &ProviderTemplate) -> Option<String> {
    let max_chars = template.max_param_chars.unwrap_or(DEFAULT_MAX_PARAM_CHARS) as usize;

    let variables = template
        .params
        .iter()
        .map(|(name, value)| {
            let line = phone::plain(value)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");

            let value = if line.is_empty() {
                "-".to_string()
            } else {
                common::truncate_chars(&line, max_chars)
            };

            (name.clone(), Value::String(value))
        })
        .collect::<Map<_, _>>();

    (!variables.is_empty()).then(|| Value::Object(variables).to_string())
}

#[derive(Default)]
pub struct TwilioWhatsappConnectorAdapter;

impl TwilioWhatsappConnectorAdapter {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ConnectorAdapter for TwilioWhatsappConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        TWILIO_WHATSAPP_CONNECTOR_TYPE
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

    fn template_provider(&self) -> Option<&'static str> {
        Some(PROVIDER)
    }

    fn takes_replies(&self, config: &Value) -> bool {
        phone::replies_on(config)
    }

    fn addresses(&self) -> Vec<ConnectorAddress> {
        vec![super::address()]
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        WhatsappConfig::parse(config)?.validate()
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        phone::target_number(target, TWILIO_WHATSAPP_CONNECTOR_TYPE, number).map(|_| ())
    }

    async fn send(
        &self,
        config: &Value,
        target: &NotifyTarget,
        message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        let config = WhatsappConfig::parse(config)?;
        config.validate()?;

        let to = phone::target_number(target, TWILIO_WHATSAPP_CONNECTOR_TYPE, number)?;
        let (field, from) = sender(&config.sender)?;
        let template = approved_template(message)?;

        let mut form = vec![
            ("To", format!("{CHANNEL_PREFIX}{to}")),
            (field, from),
            ("ContentSid", template.code.trim().to_string()),
        ];

        if let Some(variables) = content_variables(template) {
            form.push(("ContentVariables", variables));
        }

        let started = Instant::now();

        let sid = create(
            TWILIO_WHATSAPP_CONNECTOR_TYPE,
            &config.account,
            "Messages",
            &form,
        )
        .await?;

        Ok(common::delivered(sid, started))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use domain::notify::connector::NotifyTargetType;
    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{basic_auth, method, path},
    };

    use super::{super::tests::SID, *};

    const CONTENT: &str = "HXb5b62575e6e4ff6129ad7c8efe1f983e";
    const SERVICE: &str = "MG0123456789abcdef0123456789abcdef";

    fn config(base: &str, sender: &str) -> Value {
        json!({
            "accountSid": SID,
            "authToken": "token",
            "sender": sender,
            "apiBaseUrl": base,
        })
    }

    fn target(target_type: NotifyTargetType, value: &str) -> NotifyTarget {
        NotifyTarget {
            target_type,
            value: value.into(),
            metadata: BTreeMap::new(),
        }
    }

    fn template(params: &[(&str, &str)], max_param_chars: Option<u32>) -> ProviderTemplate {
        ProviderTemplate {
            code: CONTENT.into(),
            params: params
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
            max_param_chars,
        }
    }

    fn message(template: Option<ProviderTemplate>) -> NotifyMessage {
        NotifyMessage {
            title: String::new(),
            text: "CRITICAL · Checkout errors".into(),
            markdown: None,
            html: None,
            metadata: BTreeMap::new(),
            provider_templates: template
                .map(|template| BTreeMap::from([(PROVIDER.to_string(), template)]))
                .unwrap_or_default(),
            event: None,
            card: None,
        }
    }

    /// The form of the one message the server was asked to create.
    async fn sent(server: &MockServer) -> BTreeMap<String, String> {
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);

        url::form_urlencoded::parse(&requests[0].body)
            .into_owned()
            .collect()
    }

    #[test]
    fn the_sender_is_a_whatsapp_number_or_a_messaging_service() {
        let from = ("From", "whatsapp:+14155550100".to_string());

        assert_eq!(sender("+1 415 555 0100").unwrap(), from);
        assert_eq!(sender("whatsapp:+14155550100").unwrap(), from);
        assert_eq!(
            sender(SERVICE).unwrap(),
            ("MessagingServiceSid", SERVICE.to_string())
        );
        assert!(sender("").is_err());
        assert!(sender("MoleSignal").is_err());
    }

    #[test]
    fn the_connector_needs_its_account_and_a_sender_and_follows_the_replies_switch() {
        let adapter = TwilioWhatsappConnectorAdapter::new();
        let mut valid = config("https://api.twilio.com", "+14155550100");

        assert!(adapter.validate_config(&valid).is_ok());
        assert_eq!(adapter.template_provider(), Some("whatsapp"));
        assert_eq!(adapter.channel(), NotifyChannel::Sms);
        assert!(!adapter.takes_replies(&valid));

        valid["replies"] = json!(true);
        assert!(adapter.takes_replies(&valid));

        let mut no_token = valid.clone();
        no_token["authToken"] = json!("");
        assert!(adapter.validate_config(&no_token).is_err());

        let mut no_sender = valid;
        no_sender["sender"] = json!("MoleSignal");
        assert!(adapter.validate_config(&no_sender).is_err());
    }

    #[test]
    fn a_person_is_reached_at_their_phone_number() {
        let adapter = TwilioWhatsappConnectorAdapter::new();

        for value in ["+8613800138000", "whatsapp:+14155550123"] {
            assert!(
                adapter
                    .validate_target(&target(NotifyTargetType::DirectUser, value))
                    .is_ok(),
                "{value}"
            );
        }

        assert!(
            adapter
                .validate_target(&target(NotifyTargetType::FixedAddress, "13800138000"))
                .is_err()
        );
        assert!(
            adapter
                .validate_target(&target(NotifyTargetType::FixedGroup, "+14155550123"))
                .is_err()
        );
    }

    #[test]
    fn variables_are_one_line_and_never_empty() {
        let filled = content_variables(&template(
            &[
                ("1", "**Checkout errors**\n\nrose\tabove      5%"),
                ("2", "  "),
                ("3", "abcdefghij"),
            ],
            Some(5),
        ))
        .unwrap();

        let filled: Value = serde_json::from_str(&filled).unwrap();
        assert_eq!(filled["1"], "Chec…");
        assert_eq!(filled["2"], "-");
        assert_eq!(filled["3"], "abcd…");

        let whole = content_variables(&template(&[("1", "Disk\nfull  on   db-1")], None)).unwrap();
        assert_eq!(whole, r#"{"1":"Disk full on db-1"}"#);

        assert!(content_variables(&template(&[], None)).is_none());
    }

    #[test]
    fn only_a_content_template_is_sent() {
        assert!(approved_template(&message(None)).is_err());

        let mut not_content = template(&[], None);
        not_content.code = "SMS_123456789".into();

        let error = approved_template(&message(Some(not_content)))
            .unwrap_err()
            .to_string();
        assert!(error.contains("Content SID"), "{error}");

        assert!(approved_template(&message(Some(template(&[], None)))).is_ok());
    }

    #[tokio::test]
    async fn a_template_is_sent_by_its_content_sid_with_its_variables() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path(format!("/2010-04-01/Accounts/{SID}/Messages.json")))
            .and(basic_auth(SID, "token"))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(json!({"sid": "SM1", "status": "queued"})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let message = message(Some(template(
            &[("1", "Checkout errors"), ("2", "31")],
            None,
        )));

        let result = TwilioWhatsappConnectorAdapter::new()
            .send(
                &config(&server.uri(), "+14155550100"),
                &target(NotifyTargetType::DirectUser, "+8613800138000"),
                &message,
            )
            .await
            .unwrap();

        assert_eq!(result.provider_message_id.as_deref(), Some("SM1"));

        let form = sent(&server).await;
        assert_eq!(form["To"], "whatsapp:+8613800138000");
        assert_eq!(form["From"], "whatsapp:+14155550100");
        assert_eq!(form["ContentSid"], CONTENT);
        assert!(!form.contains_key("Body"));

        let variables: Value = serde_json::from_str(&form["ContentVariables"]).unwrap();
        assert_eq!(variables, json!({"1": "Checkout errors", "2": "31"}));
    }

    #[tokio::test]
    async fn a_messaging_service_picks_the_whatsapp_sender() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({"sid": "SM2"})))
            .expect(1)
            .mount(&server)
            .await;

        TwilioWhatsappConnectorAdapter::new()
            .send(
                &config(&server.uri(), SERVICE),
                &target(NotifyTargetType::FixedAddress, "whatsapp:+14155550123"),
                &message(Some(template(&[], None))),
            )
            .await
            .unwrap();

        let form = sent(&server).await;
        assert_eq!(form["MessagingServiceSid"], SERVICE);
        assert_eq!(form["To"], "whatsapp:+14155550123");
        assert!(!form.contains_key("From"));
        assert!(!form.contains_key("ContentVariables"));
    }

    #[tokio::test]
    async fn without_an_approved_template_nothing_is_sent() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({"sid": "SM3"})))
            .expect(0)
            .mount(&server)
            .await;

        let error = TwilioWhatsappConnectorAdapter::new()
            .send(
                &config(&server.uri(), "+14155550100"),
                &target(NotifyTargetType::DirectUser, "+14155550123"),
                &message(None),
            )
            .await
            .unwrap_err();

        assert!(matches!(error, Error::InvalidArgument(_)), "{error}");
        assert!(error.to_string().contains("approved"), "{error}");
    }

    #[tokio::test]
    async fn a_refused_message_says_why() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "code": 63016,
                "message": "Failed to send freeform message because you are outside the allowed window.",
                "status": 400,
            })))
            .mount(&server)
            .await;

        let error = TwilioWhatsappConnectorAdapter::new()
            .send(
                &config(&server.uri(), "+14155550100"),
                &target(NotifyTargetType::DirectUser, "+14155550123"),
                &message(Some(template(&[("1", "Disk full")], None))),
            )
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("HTTP 400 63016"), "{error}");
        assert!(error.contains("outside the allowed window"), "{error}");
    }
}
