// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 腾讯云语音消息：`SendTtsVoice`（2020-09-02）向一个号码念审核过的模板。应用（`VoiceSdkAppid`）跟着连接器，
//! 模板 ID 与变量写在通知策略选的电话模板里。语音是预付费的：套餐用完时接口拒绝，错误原样记在投递记录上。
//!
//! 开了「回复」时，接听的人按 1 确认、按 2 解决：按键由腾讯云推送到回调地址，按被叫号码对回这通电话。模板
//! 里要自己写上「按 1 确认，按 2 解决」，控制台要打开按键回调。

use std::time::Instant;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    Account, ApiCall, PROVIDER, VOICE_API, address, approved_template, call, common, phone,
    regions, template_params,
};
use crate::{
    domain::notify::{
        connector::{
            ConnectorAdapter, ConnectorAddress, ConnectorCapabilities, ConnectorDeliveryResult,
            ConnectorRegion, NotifyChannel, NotifyConnector, NotifyMessage, NotifyTarget,
        },
        reply::REPLY_REF_KEY,
    },
    shared::{Error, Result},
};

pub const TENCENT_VOICE_CONNECTOR_TYPE: &str = "tencent_voice";

const TENCENT_VOICE_API_BASE_URL: &str = "https://vms.tencentcloudapi.com";

/// 单个变量默认最多多少个字：文档没写上限，超了会被拒（`ContentLengthLimit`），一通电话也念不长。放宽写在
/// 通知模板里。
const DEFAULT_MAX_PARAM_CHARS: u32 = 20;

fn default_api_base_url() -> String {
    TENCENT_VOICE_API_BASE_URL.into()
}

fn default_play_times() -> u8 {
    2
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VoiceConfig {
    #[serde(flatten)]
    account: Account,
    #[serde(default)]
    sdk_app_id: String,
    #[serde(default = "default_play_times")]
    play_times: u8,
    #[serde(default = "default_api_base_url")]
    api_base_url: String,
    #[serde(default)]
    replies: bool,
}

impl VoiceConfig {
    fn parse(value: &Value) -> Result<Self> {
        serde_json::from_value(value.clone()).map_err(|error| {
            Error::invalid(format!(
                "invalid {TENCENT_VOICE_CONNECTOR_TYPE} config: {error}"
            ))
        })
    }

    fn validate(&self) -> Result<()> {
        self.account.validate(TENCENT_VOICE_CONNECTOR_TYPE)?;
        common::require(&self.sdk_app_id, "tencent_voice sdkAppId")?;

        if !(1..=3).contains(&self.play_times) {
            return Err(Error::invalid(
                "tencent_voice play_times must be between 1 and 3",
            ));
        }

        common::validate_http_url(&self.api_base_url, "tencent_voice apiBaseUrl")
    }
}

fn called_number(target: &NotifyTarget) -> Result<String> {
    phone::target_number(
        target,
        TENCENT_VOICE_CONNECTOR_TYPE,
        phone::e164_or_mainland_mobile,
    )
}

#[derive(Default)]
pub struct TencentVoiceConnectorAdapter {
    /// 查地域的地址；空着是腾讯云的接口。
    region_base_url: Option<String>,
}

impl TencentVoiceConnectorAdapter {
    pub const fn new() -> Self {
        Self {
            region_base_url: None,
        }
    }
}

#[async_trait]
impl ConnectorAdapter for TencentVoiceConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        TENCENT_VOICE_CONNECTOR_TYPE
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
        vec![address("vms", TENCENT_VOICE_API_BASE_URL)]
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
        connector: &NotifyConnector,
        language: &str,
    ) -> Result<Vec<ConnectorRegion>> {
        regions(
            TENCENT_VOICE_CONNECTOR_TYPE,
            connector,
            self.region_base_url.as_deref(),
            "vms",
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
        let template = approved_template(TENCENT_VOICE_CONNECTOR_TYPE, message, "phone")?;

        let mut payload = json!({
            "TemplateId": template.code.trim(),
            "TemplateParamSet": template_params(template, DEFAULT_MAX_PARAM_CHARS),
            "CalledNumber": called_number(target)?,
            "VoiceSdkAppid": config.sdk_app_id.trim(),
            "PlayTimes": config.play_times,
        });

        if let Some(reference) = message.metadata.get(REPLY_REF_KEY) {
            payload["SessionContext"] = json!(reference);
        }

        let started = Instant::now();

        let answer = call(
            TENCENT_VOICE_CONNECTOR_TYPE,
            &config.account,
            ApiCall {
                api: VOICE_API,
                base_url: &config.api_base_url,
                action: "SendTtsVoice",
                region: Some(config.account.region.trim()),
                language: None,
            },
            &payload,
        )
        .await?;

        Ok(common::delivered(
            answer["Response"]["SendStatus"]["CallId"]
                .as_str()
                .map(str::to_string),
            started,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_json, header, header_regex, method, path},
    };

    use super::*;
    use crate::domain::notify::{connector::NotifyTargetType, template::ProviderTemplate};

    fn config(base: &str) -> Value {
        json!({
            "secretId": "AKID-id",
            "secretKey": "secret",
            "sdkAppId": "1400006666",
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

    fn message() -> NotifyMessage {
        NotifyMessage {
            title: String::new(),
            text: "Critical alert".into(),
            markdown: None,
            html: None,
            metadata: BTreeMap::new(),
            provider_templates: BTreeMap::from([(
                PROVIDER.to_string(),
                ProviderTemplate {
                    code: "4356".into(),
                    params: BTreeMap::from([("1".to_string(), "Checkout errors".to_string())]),
                    max_param_chars: None,
                },
            )]),
            event: None,
            card: None,
        }
    }

    #[tokio::test]
    async fn a_call_reads_the_template_with_its_variables() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/"))
            .and(header("x-tc-action", "SendTtsVoice"))
            .and(header("x-tc-version", "2020-09-02"))
            .and(header_regex("authorization", "/vms/tc3_request"))
            .and(body_json(json!({
                "TemplateId": "4356",
                "TemplateParamSet": ["Checkout errors"],
                "CalledNumber": "+8613800138000",
                "VoiceSdkAppid": "1400006666",
                "PlayTimes": 2,
                "SessionContext": "ref-1",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "Response": {
                    "SendStatus": { "CallId": "c-1", "SessionContext": "ref-1" },
                    "RequestId": "r-1",
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let mut replied = message();
        replied
            .metadata
            .insert(REPLY_REF_KEY.into(), "ref-1".into());

        let result = TencentVoiceConnectorAdapter::new()
            .send(&config(&server.uri()), &person("+8613800138000"), &replied)
            .await
            .unwrap();

        assert_eq!(result.provider_message_id.as_deref(), Some("c-1"));
    }

    #[tokio::test]
    async fn a_used_up_package_says_so() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "Response": {
                    "Error": {
                        "Code": "FailedOperation.InsufficientBalanceInVoicePackage",
                        "Message": "语音套餐包余量不足",
                    },
                    "RequestId": "r-2",
                }
            })))
            .mount(&server)
            .await;

        let error = TencentVoiceConnectorAdapter::new()
            .send(&config(&server.uri()), &person("13800138000"), &message())
            .await
            .unwrap_err()
            .to_string();

        assert!(
            error.contains("InsufficientBalanceInVoicePackage"),
            "{error}"
        );
        assert!(error.contains("语音套餐包余量不足"), "{error}");
    }

    #[test]
    fn the_call_counts_are_checked() {
        let adapter = TencentVoiceConnectorAdapter::new();
        let mut settings = config("https://vms.tencentcloudapi.com");

        assert!(adapter.validate_config(&settings).is_ok());
        assert!(!adapter.takes_replies(&settings));

        settings["playTimes"] = json!(5);
        assert!(adapter.validate_config(&settings).is_err());

        settings["playTimes"] = json!(1);
        settings["replies"] = json!(true);
        assert!(adapter.takes_replies(&settings));
        assert_eq!(
            adapter.merge_window_secs(),
            Some(phone::CALL_MERGE_WINDOW_SECS)
        );
    }
}
