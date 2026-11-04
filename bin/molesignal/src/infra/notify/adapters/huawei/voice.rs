// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 华为云语音通知：`/rest/httpsessions/callnotify/v2.0` 向一个号码念审核过的模板，JSON 请求，X-AKSK 认证。
//! 外显号码跟着连接器，模板 ID 与变量写在通知策略选的电话模板里。
//!
//! 开了「回复」时，放音后收一位按键（`collectInd`），按 1 确认、按 2 解决。按键结果推到控制台配的呼叫状态
//! 地址，只带会话 ID：`userData` 带着回复引用，随更早的状态推送（呼出、振铃、接听）回来时把会话 ID 记到引用
//! 上。模板里要自己写上「按 1 确认，按 2 解决」。

use std::time::Instant;

use async_trait::async_trait;
use aws_lc_rs::hmac;
use base64::Engine;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    Application, PROVIDER, approved_template, common, created_now, nonce, phone, template_params,
};
use crate::{
    domain::notify::{
        connector::{
            ConnectorAdapter, ConnectorCapabilities, ConnectorDeliveryResult, NotifyChannel,
            NotifyMessage, NotifyTarget,
        },
        reply::REPLY_REF_KEY,
    },
    shared::{
        Error, Result,
        http_trace::{self, HttpTarget},
    },
};

pub const HUAWEI_VOICE_CONNECTOR_TYPE: &str = "huawei_voice";

/// 单个变量最多 32 个字节，一个汉字占 3 个：放不下时要拆成几个变量。
const MAX_PARAM_BYTES: usize = 32;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VoiceConfig {
    #[serde(flatten)]
    application: Application,
    /// 外显号码：订购的固话，写成国家码加区号加号码。
    #[serde(default)]
    show_number: String,
    #[serde(default)]
    replies: bool,
}

impl VoiceConfig {
    fn parse(value: &Value) -> Result<Self> {
        serde_json::from_value(value.clone()).map_err(|error| {
            Error::invalid(format!(
                "invalid {HUAWEI_VOICE_CONNECTOR_TYPE} config: {error}"
            ))
        })
    }

    fn validate(&self) -> Result<()> {
        self.application.validate(HUAWEI_VOICE_CONNECTOR_TYPE)?;
        common::require(&self.show_number, "huawei_voice showNumber")
    }
}

/// `PasswordDigest`：`Base64(HMAC-SHA256(AppSecret, Nonce + Created))`。
fn aksk(app_key: &str, app_secret: &str, nonce: &str, created: &str) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, app_secret.as_bytes());
    let tag = hmac::sign(&key, format!("{nonce}{created}").as_bytes());

    let password_digest = base64::engine::general_purpose::STANDARD.encode(tag.as_ref());

    format!(
        "UsernameToken Username=\"{app_key}\",PasswordDigest=\"{password_digest}\",Nonce=\"{nonce}\",Created=\"{created}\""
    )
}

fn called_number(target: &NotifyTarget) -> Result<String> {
    phone::target_number(
        target,
        HUAWEI_VOICE_CONNECTOR_TYPE,
        phone::e164_or_mainland_mobile,
    )
}

#[derive(Default)]
pub struct HuaweiVoiceConnectorAdapter;

impl HuaweiVoiceConnectorAdapter {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ConnectorAdapter for HuaweiVoiceConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        HUAWEI_VOICE_CONNECTOR_TYPE
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

    fn merge_window_secs(&self) -> Option<u32> {
        Some(phone::CALL_MERGE_WINDOW_SECS)
    }

    fn template_provider(&self) -> Option<&'static str> {
        Some(PROVIDER)
    }

    fn takes_replies(&self, config: &Value) -> bool {
        VoiceConfig::parse(config).is_ok_and(|config| config.replies)
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        VoiceConfig::parse(config)?.validate()
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        called_number(target).map(|_| ())
    }

    async fn send(
        &self,
        config: &Value,
        target: &NotifyTarget,
        message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        let config = VoiceConfig::parse(config)?;
        config.validate()?;
        let template = approved_template(HUAWEI_VOICE_CONNECTOR_TYPE, message, "phone")?;
        let application = &config.application;

        let params: Vec<String> = template_params(template, MAX_PARAM_BYTES as u32)
            .iter()
            .map(|value| common::truncate_bytes(value, MAX_PARAM_BYTES))
            .collect();

        let mut play = json!({
            "templateId": template.code.trim(),
            "templateParas": params,
        });

        let reference = message.metadata.get(REPLY_REF_KEY);

        if config.replies && reference.is_some() {
            play["collectInd"] = json!(1);
        }

        let mut payload = json!({
            "displayNbr": config.show_number.trim(),
            "calleeNbr": called_number(target)?,
            "playInfoList": [play],
        });

        if let Some(reference) = reference {
            payload["userData"] = json!(reference);
        }

        let client = common::client(application.timeout_secs, HUAWEI_VOICE_CONNECTOR_TYPE)?;
        let started = Instant::now();

        let request = client
            .post(application.endpoint("/rest/httpsessions/callnotify/v2.0"))
            .header(
                "authorization",
                r#"AKSK realm="SDP",profile="UsernameToken",type="Appkey""#,
            )
            .header(
                "x-aksk",
                aksk(
                    application.app_key.trim(),
                    application.app_secret.trim(),
                    &nonce()?,
                    &created_now(),
                ),
            )
            .json(&payload);

        let response = http_trace::send(&client, request, HttpTarget::ThirdParty)
            .await
            .map_err(|_| Error::unavailable("huawei_voice request failed"))?;

        let status = response.status();

        let answer = response.json::<Value>().await.map_err(|_| {
            Error::unavailable(format!("huawei_voice returned HTTP {}", status.as_u16()))
        })?;

        let code = answer["resultcode"].as_str().unwrap_or_default();

        if !status.is_success() || code != "0" {
            return Err(Error::unavailable(format!(
                "huawei_voice was refused: {}: {}",
                if code.is_empty() { "HTTP error" } else { code },
                common::provider_message(answer["resultdesc"].as_str().unwrap_or_default())
            )));
        }

        Ok(common::delivered(
            answer["sessionId"].as_str().map(str::to_string),
            started,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_json, header_regex, method, path},
    };

    use super::*;
    use crate::domain::notify::{connector::NotifyTargetType, template::ProviderTemplate};

    fn config(base: &str) -> Value {
        json!({
            "appKey": "app-key",
            "appSecret": "app-secret",
            "apiBaseUrl": base,
            "showNumber": "+862012345678",
        })
    }

    fn person(value: &str) -> NotifyTarget {
        NotifyTarget {
            target_type: NotifyTargetType::DirectUser,
            value: value.into(),
            metadata: BTreeMap::new(),
        }
    }

    fn message(first: &str) -> NotifyMessage {
        NotifyMessage {
            title: String::new(),
            text: "Critical alert".into(),
            markdown: None,
            html: None,
            metadata: BTreeMap::new(),
            provider_templates: BTreeMap::from([(
                PROVIDER.to_string(),
                ProviderTemplate {
                    code: "tmpl-voice".into(),
                    params: BTreeMap::from([("1".to_string(), first.to_string())]),
                    max_param_chars: None,
                },
            )]),
            event: None,
            card: None,
        }
    }

    #[test]
    fn the_digest_is_the_hmac_of_nonce_and_created() {
        // 用同样的算法在别处（Python 的 hmac）算出来对照。
        assert_eq!(
            aksk("app-key", "app-secret", "n0nce", "2026-10-31T12:00:00Z"),
            "UsernameToken Username=\"app-key\",\
             PasswordDigest=\"rbG3j/LZb5yzxiXYQuUSu1Em/RZQ5jwt5uZA0Melj2E=\",\
             Nonce=\"n0nce\",Created=\"2026-10-31T12:00:00Z\""
        );
    }

    #[tokio::test]
    async fn a_call_plays_the_template_and_collects_a_key_when_replies_are_on() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/rest/httpsessions/callnotify/v2.0"))
            .and(header_regex(
                "authorization",
                r#"^AKSK realm="SDP",profile="UsernameToken",type="Appkey"$"#,
            ))
            .and(header_regex(
                "x-aksk",
                r#"^UsernameToken Username="app-key",PasswordDigest="#,
            ))
            .and(body_json(json!({
                "displayNbr": "+862012345678",
                "calleeNbr": "+8613800138000",
                "playInfoList": [{
                    "templateId": "tmpl-voice",
                    "templateParas": ["Disk full"],
                    "collectInd": 1,
                }],
                "userData": "ref-1",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "resultcode": "0", "resultdesc": "Success", "sessionId": "s-1",
            })))
            .expect(1)
            .mount(&server)
            .await;

        let mut settings = config(&server.uri());
        settings["replies"] = json!(true);

        let mut replied = message("Disk full");
        replied
            .metadata
            .insert(REPLY_REF_KEY.into(), "ref-1".into());

        let result = HuaweiVoiceConnectorAdapter::new()
            .send(&settings, &person("13800138000"), &replied)
            .await
            .unwrap();

        assert_eq!(result.provider_message_id.as_deref(), Some("s-1"));
    }

    #[tokio::test]
    async fn a_variable_fits_in_its_bytes_and_a_refusal_says_why() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({
                "resultcode": "1012005", "resultdesc": "templateParas invalid",
            })))
            .mount(&server)
            .await;

        let error = HuaweiVoiceConnectorAdapter::new()
            .send(
                &config(&server.uri()),
                &person("13800138000"),
                &message(&"磁盘".repeat(40)),
            )
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("1012005"), "{error}");

        let requests = server.received_requests().await.unwrap();
        let sent: Value = serde_json::from_slice(&requests[0].body).unwrap();
        let value = sent["playInfoList"][0]["templateParas"][0]
            .as_str()
            .unwrap();

        assert!(value.len() <= MAX_PARAM_BYTES, "{value}");
    }

    #[test]
    fn the_configuration_needs_a_number_to_call_from() {
        let adapter = HuaweiVoiceConnectorAdapter::new();

        assert!(
            adapter
                .validate_config(&config("https://rtccall.example.com"))
                .is_ok()
        );

        let mut settings = config("https://rtccall.example.com");
        settings["showNumber"] = json!("");
        assert!(adapter.validate_config(&settings).is_err());

        assert!(!adapter.takes_replies(&config("https://rtccall.example.com")));
    }
}
