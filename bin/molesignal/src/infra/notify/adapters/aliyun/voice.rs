// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 阿里云语音通知：`SingleCallByTts` 拨一个内地号码，念审核过的文本转语音模板；模板编号与变量写在
//! 通知策略选的电话模板里。不填外显号码是公共模式；填了是专属模式，模板也要是专属模式的。
//!
//! 开了「回复」（按键确认）时改用 `IvrCall`：外显号码必须是购入的真实号，模板里自己写上「按 1 确认，
//! 按 2 解决」；按键结果由阿里云推送到回调地址，`OutId` 是回复引用的 ID。

use std::time::Instant;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use super::{Account, PROVIDER, approved_template, call, common, phone, regions, template_param};
use crate::{
    domain::notify::{
        connector::{
            ConnectorAdapter, ConnectorAddress, ConnectorCapabilities, ConnectorDeliveryResult,
            ConnectorRegion, NotifyChannel, NotifyConnector, NotifyMessage, NotifyTarget,
        },
        reply::REPLY_REF_KEY,
        template::ProviderTemplate,
    },
    shared::{Error, Result},
};

pub const ALIYUN_VOICE_CONNECTOR_TYPE: &str = "aliyun_voice";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VoiceConfig {
    #[serde(flatten)]
    account: Account,
    #[serde(default)]
    show_number: String,
    #[serde(default = "default_play_times")]
    play_times: u8,
    #[serde(default = "default_api_base_url")]
    api_base_url: String,
    /// 按键确认：用 `IvrCall` 拨，收 1（确认）、2（解决）。
    #[serde(default)]
    replies: bool,
}

fn default_play_times() -> u8 {
    2
}

/// 等按键多久（毫秒）。
const KEYPRESS_TIMEOUT_MILLIS: u32 = 10_000;

const ALIYUN_VOICE_API_BASE_URL: &str = "https://dyvmsapi.aliyuncs.com";

fn default_api_base_url() -> String {
    ALIYUN_VOICE_API_BASE_URL.into()
}

impl VoiceConfig {
    fn parse(value: &Value) -> Result<Self> {
        serde_json::from_value(value.clone()).map_err(|error| {
            Error::invalid(format!(
                "invalid {ALIYUN_VOICE_CONNECTOR_TYPE} config: {error}"
            ))
        })
    }

    fn validate(&self) -> Result<()> {
        self.account.validate(ALIYUN_VOICE_CONNECTOR_TYPE)?;

        if !(1..=3).contains(&self.play_times) {
            return Err(Error::invalid(
                "aliyun_voice play_times must be between 1 and 3",
            ));
        }

        if self.replies && self.show_number.trim().is_empty() {
            return Err(Error::invalid(
                "aliyun_voice takes keypresses only with a showNumber: a real number bought for IvrCall",
            ));
        }

        common::validate_http_url(&self.api_base_url, "aliyun_voice apiBaseUrl")
    }
}

/// 拨的号码：内地手机号 11 位，固话带区号前的 0。
fn called_number(target: &NotifyTarget) -> Result<String> {
    let number = phone::target_number(
        target,
        ALIYUN_VOICE_CONNECTOR_TYPE,
        phone::e164_or_mainland_mobile,
    )?;

    phone::mainland_dial(&number)
}

#[derive(Default)]
pub struct AliyunVoiceConnectorAdapter {
    /// 列地域用的元数据地址；空着是阿里云的门户。
    meta_base_url: Option<String>,
}

impl AliyunVoiceConnectorAdapter {
    pub const fn new() -> Self {
        Self {
            meta_base_url: None,
        }
    }

    #[cfg(test)]
    fn with_meta_base_url(meta_base_url: String) -> Self {
        Self {
            meta_base_url: Some(meta_base_url),
        }
    }
}

#[async_trait]
impl ConnectorAdapter for AliyunVoiceConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        ALIYUN_VOICE_CONNECTOR_TYPE
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
        vec![super::address("dyvmsapi", ALIYUN_VOICE_API_BASE_URL)]
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

    async fn regions(
        &self,
        _connector: &NotifyConnector,
        language: &str,
    ) -> Result<Vec<ConnectorRegion>> {
        regions(
            ALIYUN_VOICE_CONNECTOR_TYPE,
            self.meta_base_url.as_deref(),
            "Dyvmsapi",
            language,
        )
        .await
    }

    async fn send(
        &self,
        config: &Value,
        target: &NotifyTarget,
        message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        let config = VoiceConfig::parse(config)?;
        config.validate()?;
        let template = approved_template(ALIYUN_VOICE_CONNECTOR_TYPE, message, "phone")?;

        if config.replies
            && let Some(reference) = message.metadata.get(REPLY_REF_KEY)
        {
            return ivr_call(&config, target, template, reference).await;
        }

        let mut params = vec![
            ("CalledNumber", called_number(target)?),
            ("TtsCode", template.code.trim().to_string()),
            ("PlayTimes", config.play_times.to_string()),
        ];

        if let Some(param) = template_param(template) {
            params.push(("TtsParam", param));
        }

        if !config.show_number.trim().is_empty() {
            params.push(("CalledShowNumber", config.show_number.trim().to_string()));
        }

        let started = Instant::now();

        let answer = call(
            ALIYUN_VOICE_CONNECTOR_TYPE,
            &config.account,
            &config.api_base_url,
            "SingleCallByTts",
            &params,
        )
        .await?;

        let call_id = answer
            .get("CallId")
            .and_then(Value::as_str)
            .map(str::to_string);

        Ok(common::delivered(call_id, started))
    }
}

/// 收按键的一通：念模板，按 1 确认、按 2 解决，`OutId` 带着回复引用。
async fn ivr_call(
    config: &VoiceConfig,
    target: &NotifyTarget,
    template: &ProviderTemplate,
    reference: &str,
) -> Result<ConnectorDeliveryResult> {
    let mut params = vec![
        ("CalledShowNumber", config.show_number.trim().to_string()),
        ("CalledNumber", called_number(target)?),
        ("StartCode", template.code.trim().to_string()),
        ("PlayTimes", config.play_times.to_string()),
        ("Timeout", KEYPRESS_TIMEOUT_MILLIS.to_string()),
        ("OutId", reference.to_string()),
        ("MenuKeyMap.1.Key", "1".to_string()),
        ("MenuKeyMap.2.Key", "2".to_string()),
    ];

    if let Some(param) = template_param(template) {
        params.push(("StartTtsParams", param));
    }

    let started = Instant::now();

    let answer = call(
        ALIYUN_VOICE_CONNECTOR_TYPE,
        &config.account,
        &config.api_base_url,
        "IvrCall",
        &params,
    )
    .await?;

    let call_id = answer
        .get("CallId")
        .and_then(Value::as_str)
        .map(str::to_string);

    Ok(common::delivered(call_id, started))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{header, header_exists, method, path, query_param},
    };

    use super::{super::tests::draft, *};
    use crate::domain::notify::{connector::NotifyTargetType, template::ProviderTemplate};

    fn config(base: &str) -> Value {
        json!({
            "accessKeyId": "LTAI-id",
            "accessKeySecret": "secret",
            "apiBaseUrl": base,
        })
    }

    fn person(value: &str) -> NotifyTarget {
        NotifyTarget {
            target_type: NotifyTargetType::DirectUser,
            value: value.into(),
            metadata: BTreeMap::new(),
        }
    }

    /// 电话模板套用后的通知：阿里云念 `TTS_1`，`content` 填告警的名字。
    fn message() -> NotifyMessage {
        NotifyMessage {
            title: String::new(),
            text: "Critical alert. Checkout errors.".into(),
            markdown: None,
            html: None,
            metadata: BTreeMap::new(),
            provider_templates: BTreeMap::from([(
                PROVIDER.to_string(),
                ProviderTemplate {
                    code: "TTS_1".into(),
                    params: BTreeMap::from([(
                        "content".to_string(),
                        "CRITICAL · Checkout errors".to_string(),
                    )]),
                    max_param_chars: None,
                },
            )]),
            event: None,
            card: None,
        }
    }

    #[test]
    fn only_mainland_numbers_are_called() {
        let adapter = AliyunVoiceConnectorAdapter::new();

        adapter.validate_target(&person("13800138000")).unwrap();
        adapter
            .validate_target(&person("+86 138 0013 8000"))
            .unwrap();
        assert!(adapter.validate_target(&person("+12025550123")).is_err());
        adapter
            .validate_config(&config("https://dyvmsapi.aliyuncs.com"))
            .unwrap();
        assert!(
            adapter
                .validate_config(&json!({"accessKeyId": "id"}))
                .is_err()
        );
    }

    #[tokio::test]
    async fn a_call_reads_the_template_from_the_notify_template() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/"))
            .and(header("x-acs-action", "SingleCallByTts"))
            .and(header("x-acs-version", "2017-05-25"))
            .and(header_exists("authorization"))
            .and(query_param("CalledNumber", "13800138000"))
            .and(query_param("TtsCode", "TTS_1"))
            .and(query_param("PlayTimes", "2"))
            .and(query_param(
                "TtsParam",
                r#"{"content":"CRITICAL · Checkout errors"}"#,
            ))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"Code": "OK", "Message": "OK", "CallId": "116^10"})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let result = AliyunVoiceConnectorAdapter::new()
            .send(
                &config(&server.uri()),
                &person("+8613800138000"),
                &message(),
            )
            .await
            .unwrap();

        assert!(result.delivered);
        assert_eq!(result.provider_message_id.as_deref(), Some("116^10"));

        // 通知模板里没写阿里云的模板编号：不拨。
        let mut unwritten = message();
        unwritten.provider_templates.clear();

        assert!(
            AliyunVoiceConnectorAdapter::new()
                .send(&config(&server.uri()), &person("13800138000"), &unwritten)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn a_refused_call_says_why() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "Code": "isv.BUSINESS_LIMIT_CONTROL",
                "Message": "触发号码天级流控",
            })))
            .mount(&server)
            .await;

        let error = AliyunVoiceConnectorAdapter::new()
            .send(&config(&server.uri()), &person("13800138000"), &message())
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("isv.BUSINESS_LIMIT_CONTROL"), "{error}");
        assert!(error.contains("触发号码天级流控"), "{error}");
    }

    #[tokio::test]
    async fn the_voice_service_lists_its_own_regions_in_english() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/products/Dyvmsapi/endpoints.json"))
            .and(query_param("language", "en_US"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": 0,
                "data": {"endpoints": [
                    {"regionId": "cn-hangzhou", "regionName": "China (Hangzhou)", "public": "dyvmsapi.aliyuncs.com"},
                ]},
            })))
            .mount(&server)
            .await;

        let adapter = AliyunVoiceConnectorAdapter::with_meta_base_url(server.uri());
        let draft = draft(ALIYUN_VOICE_CONNECTOR_TYPE);
        let regions = adapter.regions(&draft, "en-us").await.unwrap();

        assert_eq!(regions[0].name, "China (Hangzhou)");
        assert_eq!(regions[0].api_base_url, "https://dyvmsapi.aliyuncs.com");

        let refused =
            AliyunVoiceConnectorAdapter::with_meta_base_url(format!("{}/gone", server.uri()))
                .regions(&draft, "en-us")
                .await;

        assert!(refused.is_err());
    }
}
