// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

mod aliyun;
mod aws;
mod card;
mod common;
mod dingtalk;
mod email;
mod google_chat;
mod http_sms;
mod huawei;
mod lark;
mod phone;
mod slack;
mod teams;
mod tencent;
mod twilio;
mod webhook;
mod wecom;
mod wecom_bot;

pub use aliyun::{
    ALIYUN_SMS_CONNECTOR_TYPE, ALIYUN_VOICE_CONNECTOR_TYPE, AliyunSmsConnectorAdapter,
    AliyunVoiceConnectorAdapter,
};
pub use aws::{
    AWS_SMS_CONNECTOR_TYPE, AWS_VOICE_CONNECTOR_TYPE, AwsSmsConnectorAdapter,
    AwsVoiceConnectorAdapter,
};
pub(crate) use common::{TOKENS, TokenCache, google_chat_text, truncate_bytes, truncate_chars};
pub(crate) use dingtalk::{
    DINGTALK_ALERT_PARAM, DINGTALK_API_BASE_URL, DINGTALK_CARD_CALLBACK_TOPIC, DingTalkApp,
    Recipient as DingTalkRecipient, card_params as dingtalk_card_params,
    post_markdown as post_dingtalk_markdown,
};
pub use dingtalk::{
    DINGTALK_APP_CONNECTOR_TYPE, DINGTALK_WEBHOOK_CONNECTOR_TYPE, DingTalkAppConnectorAdapter,
    DingTalkWebhookConnectorAdapter,
};
pub use email::{EMAIL_SMTP_CONNECTOR_TYPE, EmailSmtpConnectorAdapter};
#[cfg(test)]
pub(crate) use google_chat::test_key as google_chat_test_key;
pub(crate) use google_chat::{
    CHAT_BOT_SCOPE as GOOGLE_CHAT_SCOPE, GOOGLE_CHAT_ALERT_FUNCTION, GoogleChatApp,
    MAX_TEXT_CHARS as GOOGLE_CHAT_MAX_TEXT_CHARS, PUBSUB_SCOPE as GOOGLE_PUBSUB_SCOPE,
    check as google_chat_check, google_chat_card,
};
pub use google_chat::{
    GOOGLE_CHAT_APP_CONNECTOR_TYPE, GOOGLE_CHAT_WEBHOOK_CONNECTOR_TYPE,
    GoogleChatAppConnectorAdapter, GoogleChatWebhookConnectorAdapter,
};
pub use http_sms::{HTTP_SMS_CONNECTOR_TYPE, HttpSmsConnectorAdapter};
pub use huawei::{
    HUAWEI_SMS_CONNECTOR_TYPE, HUAWEI_VOICE_CONNECTOR_TYPE, HuaweiSmsConnectorAdapter,
    HuaweiVoiceConnectorAdapter,
};
pub(crate) use lark::{LARK_ALERT_VALUE_KEY, LARK_SITES, lark_card};
pub use lark::{
    LARK_APP_CONNECTOR_TYPE, LARK_WEBHOOK_CONNECTOR_TYPE, LarkAppConnectorAdapter,
    LarkWebhookConnectorAdapter,
};
pub(crate) use slack::{SLACK_ALERT_BLOCK_ID, SLACK_SITES, mrkdwn, slack_card};
pub use slack::{
    SLACK_APP_CONNECTOR_TYPE, SLACK_WEBHOOK_CONNECTOR_TYPE, SlackAppConnectorAdapter,
    SlackWebhookConnectorAdapter,
};
pub(crate) use teams::{
    MAX_TEXT_CHARS as TEAMS_MAX_TEXT_CHARS, TEAMS_ALERT_VALUE_KEY, TEAMS_ALERT_VERB, TeamsApp,
    card_message as teams_card_message, encode as teams_encode, teams_card,
};
pub use teams::{
    TEAMS_APP_CONNECTOR_TYPE, TEAMS_WEBHOOK_CONNECTOR_TYPE, TeamsAppConnectorAdapter,
    TeamsWebhookConnectorAdapter,
};
pub use tencent::{
    TENCENT_SMS_CONNECTOR_TYPE, TENCENT_VOICE_CONNECTOR_TYPE, TencentSmsConnectorAdapter,
    TencentVoiceConnectorAdapter,
};
pub use twilio::{
    TWILIO_SMS_CONNECTOR_TYPE, TWILIO_VOICE_CONNECTOR_TYPE, TwilioSmsConnectorAdapter,
    TwilioVoiceConnectorAdapter,
};
pub use webhook::{WEBHOOK_CONNECTOR_TYPE, WebhookConnectorAdapter};
pub use wecom::{
    WECOM_APP_CONNECTOR_TYPE, WECOM_WEBHOOK_CONNECTOR_TYPE, WecomAppConnectorAdapter,
    WecomWebhookConnectorAdapter,
};
pub(crate) use wecom::{
    WecomCallback, WecomCardEvent, create_wecom_chat, encrypted_of as wecom_encrypted_of,
    post_wecom_chat, tell_wecom_member, update_wecom_card, wecom_card_event, wecom_template_card,
};
pub(crate) use wecom_bot::WecomBot;
pub use wecom_bot::{WECOM_BOT_CONNECTOR_TYPE, WecomBotConnectorAdapter};

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::{Map, Value, json};

    use super::*;
    use crate::domain::notify::connector::{ConnectorAdapter, validate_addresses};

    #[test]
    fn a_provider_address_is_taken_only_from_the_catalogue() {
        let adapters: Vec<Arc<dyn ConnectorAdapter>> = vec![
            Arc::new(SlackAppConnectorAdapter::new()),
            Arc::new(LarkAppConnectorAdapter::new()),
            Arc::new(DingTalkAppConnectorAdapter::new()),
            Arc::new(WecomAppConnectorAdapter::new()),
            Arc::new(WecomBotConnectorAdapter::new()),
            Arc::new(TeamsAppConnectorAdapter::new()),
            Arc::new(GoogleChatAppConnectorAdapter::new()),
            Arc::new(AliyunVoiceConnectorAdapter::new()),
            Arc::new(AliyunSmsConnectorAdapter::new()),
            Arc::new(TwilioVoiceConnectorAdapter::new()),
            Arc::new(TwilioSmsConnectorAdapter::new()),
            Arc::new(AwsVoiceConnectorAdapter::new()),
            Arc::new(AwsSmsConnectorAdapter::new()),
            Arc::new(TencentVoiceConnectorAdapter::new()),
            Arc::new(TencentSmsConnectorAdapter::new()),
        ];

        let config = |field: &str, url: &str| {
            Value::Object(Map::from_iter([
                ("region".to_string(), json!("eu-west-1")),
                (field.to_string(), json!(url)),
            ]))
        };

        for adapter in adapters {
            let kind = adapter.connector_type();
            let addresses = adapter.addresses();
            let admitted = |config: &Value| validate_addresses(kind, &addresses, config).is_ok();

            assert!(!addresses.is_empty(), "{kind}");
            assert!(admitted(&json!({})), "{kind}");

            for address in &addresses {
                for choice in &address.choices {
                    assert!(
                        choice.url.starts_with("https://") || choice.url.starts_with("wss://"),
                        "{kind} {}",
                        choice.url
                    );
                    assert!(admitted(&config(address.field, choice.url)), "{kind}");
                }

                for elsewhere in ["https://attacker.example", "http://169.254.169.254/"] {
                    assert!(!admitted(&config(address.field, elsewhere)), "{kind}");
                }
            }
        }

        // 地址由人填的连接器没有目录。
        assert!(WebhookConnectorAdapter::new().addresses().is_empty());
        assert!(HttpSmsConnectorAdapter::new().addresses().is_empty());
        assert!(SlackWebhookConnectorAdapter::new().addresses().is_empty());
    }
}
