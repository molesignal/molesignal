// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Twilio 电话：建一通 `Calls`，接通后用 `<Say>` 念通知，按设置重复几遍。开了「回复」时放在
//! `<Gather>` 里收一个键：1 确认、2 解决，交给 MoleSignal 的按键回调。

use std::time::Instant;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use super::{Account, create};
use crate::{
    domain::notify::{
        connector::{
            ConnectorAdapter, ConnectorAddress, ConnectorCapabilities, ConnectorDeliveryResult,
            NotifyChannel, NotifyMessage, NotifyTarget,
        },
        reply::{REPLY_KEYPRESS_URL_KEY, REPLY_VOICE_HINT_KEY},
    },
    infra::notify::adapters::{common, phone},
    shared::{Error, Result},
};

pub const TWILIO_VOICE_CONNECTOR_TYPE: &str = "twilio_voice";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VoiceConfig {
    #[serde(flatten)]
    account: Account,
    #[serde(default)]
    from_number: String,
    #[serde(default = "default_language")]
    language: String,
    #[serde(default)]
    voice: String,
    #[serde(default = "default_play_times")]
    play_times: u8,
    /// 按键确认：念完后收 1（确认）、2（解决）。
    #[serde(default)]
    replies: bool,
}

fn default_language() -> String {
    "en-US".into()
}

fn default_play_times() -> u8 {
    2
}

impl VoiceConfig {
    fn parse(value: &Value) -> Result<Self> {
        serde_json::from_value(value.clone()).map_err(|error| {
            Error::invalid(format!(
                "invalid {TWILIO_VOICE_CONNECTOR_TYPE} config: {error}"
            ))
        })
    }

    fn validate(&self) -> Result<()> {
        self.account.validate(TWILIO_VOICE_CONNECTOR_TYPE)?;

        phone::e164(&self.from_number).map_err(|_| {
            Error::invalid(
                "twilio_voice from_number must be a Twilio phone number, like +14155550100",
            )
        })?;

        let language = self.language.trim();

        if language.is_empty()
            || language.len() > 16
            || !language
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            return Err(Error::invalid(
                "twilio_voice language must be a language tag, like en-US or cmn-CN",
            ));
        }

        let voice = self.voice.trim();

        if voice.len() > 64
            || !voice
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        {
            return Err(Error::invalid(
                "twilio_voice voice must be a Twilio voice name, like Polly.Joanna",
            ));
        }

        if !(1..=3).contains(&self.play_times) {
            return Err(Error::invalid(
                "twilio_voice play_times must be between 1 and 3",
            ));
        }

        Ok(())
    }

    /// 这通电话的 TwiML：用设置的语言和声音念通知。收按键时放在 `<Gather>` 里，念完提示等一个键，
    /// 按下后 Twilio 把它交给回调地址。
    fn twiml(&self, message: &NotifyMessage) -> String {
        let voice = match self.voice.trim() {
            "" => String::new(),
            voice => format!(" voice=\"{}\"", escape(voice)),
        };

        let language = escape(self.language.trim());

        let said = format!(
            "<Say language=\"{language}\"{voice} loop=\"{}\">{}</Say>",
            self.play_times,
            escape(&phone::spoken(message))
        );

        let Some(action) = message
            .metadata
            .get(REPLY_KEYPRESS_URL_KEY)
            .filter(|_| self.replies)
        else {
            return format!("<Response>{said}</Response>");
        };

        let hint = message
            .metadata
            .get(REPLY_VOICE_HINT_KEY)
            .map(|hint| format!("<Say language=\"{language}\"{voice}>{}</Say>", escape(hint)))
            .unwrap_or_default();

        format!(
            "<Response><Gather input=\"dtmf\" numDigits=\"1\" timeout=\"8\" method=\"POST\" \
             action=\"{}\">{said}{hint}</Gather></Response>",
            escape(action)
        )
    }
}

/// XML 里的特殊字符。
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[derive(Default)]
pub struct TwilioVoiceConnectorAdapter;

impl TwilioVoiceConnectorAdapter {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ConnectorAdapter for TwilioVoiceConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        TWILIO_VOICE_CONNECTOR_TYPE
    }

    fn capabilities(&self) -> ConnectorCapabilities {
        ConnectorCapabilities {
            direct_user: true,
            ..ConnectorCapabilities::default()
        }
    }

    fn channel(&self) -> NotifyChannel {
        NotifyChannel::Phone
    }

    fn addresses(&self) -> Vec<ConnectorAddress> {
        vec![super::address()]
    }

    fn merge_window_secs(&self) -> Option<u32> {
        Some(phone::CALL_MERGE_WINDOW_SECS)
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        VoiceConfig::parse(config)?.validate()
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        phone::target_number(target, TWILIO_VOICE_CONNECTOR_TYPE, phone::e164).map(|_| ())
    }

    fn takes_replies(&self, config: &Value) -> bool {
        VoiceConfig::parse(config).is_ok_and(|config| config.replies)
    }

    async fn send(
        &self,
        config: &Value,
        target: &NotifyTarget,
        message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        let config = VoiceConfig::parse(config)?;
        config.validate()?;

        let to = phone::target_number(target, TWILIO_VOICE_CONNECTOR_TYPE, phone::e164)?;
        let from = phone::e164(&config.from_number)?;
        let started = Instant::now();

        let sid = create(
            TWILIO_VOICE_CONNECTOR_TYPE,
            &config.account,
            "Calls",
            &[("To", to), ("From", from), ("Twiml", config.twiml(message))],
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
        matchers::{basic_auth, body_string_contains, method, path},
    };

    use super::{super::tests::SID, *};
    use crate::domain::notify::connector::NotifyTargetType;

    fn message(title: &str) -> NotifyMessage {
        NotifyMessage {
            title: title.into(),
            text: String::new(),
            markdown: None,
            html: None,
            metadata: BTreeMap::new(),
            provider_templates: Default::default(),
            event: None,
            card: None,
        }
    }

    fn config(base: &str) -> Value {
        json!({
            "accountSid": SID,
            "authToken": "token",
            "fromNumber": "+14155550100",
            "language": "cmn-CN",
            "voice": "Polly.Zhiyu",
            "apiBaseUrl": base,
        })
    }

    #[test]
    fn the_call_says_the_notification_in_the_chosen_voice() {
        let config = VoiceConfig::parse(&config("https://api.twilio.com")).unwrap();
        config.validate().unwrap();

        assert_eq!(
            config.twiml(&message("Disk <full> & \"slow\"")),
            "<Response><Say language=\"cmn-CN\" voice=\"Polly.Zhiyu\" loop=\"2\">\
             Disk &lt;full&gt; &amp; &quot;slow&quot;</Say></Response>"
        );
    }

    #[test]
    fn the_caller_is_a_twilio_number() {
        let adapter = TwilioVoiceConnectorAdapter::new();
        let mut wrong = config("https://api.twilio.com");
        wrong["fromNumber"] = json!("MoleSignal");

        assert!(adapter.validate_config(&wrong).is_err());

        let target = NotifyTarget {
            target_type: NotifyTargetType::DirectUser,
            value: "13800138000".into(),
            metadata: BTreeMap::new(),
        };

        assert!(adapter.validate_target(&target).is_err());
    }

    #[tokio::test]
    async fn a_call_is_created_under_the_account() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path(format!("/2010-04-01/Accounts/{SID}/Calls.json")))
            .and(basic_auth(SID, "token"))
            .and(body_string_contains("To=%2B8613800138000"))
            .and(body_string_contains("From=%2B14155550100"))
            .and(body_string_contains("Twiml=%3CResponse%3E"))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(json!({"sid": "CA1", "status": "queued"})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let target = NotifyTarget {
            target_type: NotifyTargetType::DirectUser,
            value: "+8613800138000".into(),
            metadata: BTreeMap::new(),
        };

        let result = TwilioVoiceConnectorAdapter::new()
            .send(&config(&server.uri()), &target, &message("Disk full"))
            .await
            .unwrap();

        assert_eq!(result.provider_message_id.as_deref(), Some("CA1"));
    }
}
