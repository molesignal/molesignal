// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 腾讯云短信：`SendSms`（2021-01-11）用审核过的签名与模板发给一个号码。应用（`SmsSdkAppId`）和签名跟着
//! 连接器，模板 ID 与变量写在通知策略选的短信模板里。接口整体成功不等于发出去了，要看这个号码的 `Code`。

use std::time::Instant;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    Account, ApiCall, PROVIDER, SMS_API, address, approved_template, call, common, phone, regions,
    template_params,
};
use crate::{
    domain::notify::connector::{
        ConnectorAdapter, ConnectorAddress, ConnectorCapabilities, ConnectorDeliveryResult,
        ConnectorRegion, NotifyChannel, NotifyConnector, NotifyMessage, NotifyTarget,
    },
    shared::{Error, Result},
};

pub const TENCENT_SMS_CONNECTOR_TYPE: &str = "tencent_sms";

const TENCENT_SMS_API_BASE_URL: &str = "https://sms.tencentcloudapi.com";

/// 单个变量默认最多 6 个字符：2024-01-25 以后开通的账号和长期闲置的账号都是这样，放宽后写在通知模板里。
const DEFAULT_MAX_PARAM_CHARS: u32 = 6;

fn default_api_base_url() -> String {
    TENCENT_SMS_API_BASE_URL.into()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SmsConfig {
    #[serde(flatten)]
    account: Account,
    #[serde(default)]
    sdk_app_id: String,
    /// 大陆短信要写签名的文字；境外短信可以不写。
    #[serde(default)]
    sign_name: String,
    #[serde(default = "default_api_base_url")]
    api_base_url: String,
}

impl SmsConfig {
    fn parse(value: &Value) -> Result<Self> {
        serde_json::from_value(value.clone()).map_err(|error| {
            Error::invalid(format!(
                "invalid {TENCENT_SMS_CONNECTOR_TYPE} config: {error}"
            ))
        })
    }

    fn validate(&self) -> Result<()> {
        self.account.validate(TENCENT_SMS_CONNECTOR_TYPE)?;
        common::require(&self.sdk_app_id, "tencent_sms sdkAppId")?;
        common::validate_http_url(&self.api_base_url, "tencent_sms apiBaseUrl")
    }
}

fn phone_number(target: &NotifyTarget) -> Result<String> {
    phone::target_number(
        target,
        TENCENT_SMS_CONNECTOR_TYPE,
        phone::e164_or_mainland_mobile,
    )
}

#[derive(Default)]
pub struct TencentSmsConnectorAdapter {
    /// 查地域的地址；空着是腾讯云的接口。
    region_base_url: Option<String>,
}

impl TencentSmsConnectorAdapter {
    pub const fn new() -> Self {
        Self {
            region_base_url: None,
        }
    }
}

#[async_trait]
impl ConnectorAdapter for TencentSmsConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        TENCENT_SMS_CONNECTOR_TYPE
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
        vec![address("sms", TENCENT_SMS_API_BASE_URL)]
    }

    fn template_provider(&self) -> Option<&'static str> {
        Some(PROVIDER)
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        SmsConfig::parse(config)?.validate()
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        phone_number(target).map(|_| ())
    }

    async fn regions(
        &self,
        connector: &NotifyConnector,
        language: &str,
    ) -> Result<Vec<ConnectorRegion>> {
        regions(
            TENCENT_SMS_CONNECTOR_TYPE,
            connector,
            self.region_base_url.as_deref(),
            "sms",
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
        let config = SmsConfig::parse(config)?;
        config.validate()?;
        let template = approved_template(TENCENT_SMS_CONNECTOR_TYPE, message, "SMS")?;

        let mut payload = json!({
            "PhoneNumberSet": [phone_number(target)?],
            "SmsSdkAppId": config.sdk_app_id.trim(),
            "TemplateId": template.code.trim(),
            "TemplateParamSet": template_params(template, DEFAULT_MAX_PARAM_CHARS),
        });

        if !config.sign_name.trim().is_empty() {
            payload["SignName"] = json!(config.sign_name.trim());
        }

        let started = Instant::now();

        let answer = call(
            TENCENT_SMS_CONNECTOR_TYPE,
            &config.account,
            ApiCall {
                api: SMS_API,
                base_url: &config.api_base_url,
                action: "SendSms",
                region: Some(config.account.region.trim()),
                language: None,
            },
            &payload,
        )
        .await?;

        let status = &answer["Response"]["SendStatusSet"][0];

        if status["Code"].as_str() != Some("Ok") {
            return Err(Error::unavailable(format!(
                "tencent_sms was refused: {}: {}",
                status["Code"].as_str().unwrap_or("no result"),
                common::provider_message(status["Message"].as_str().unwrap_or_default())
            )));
        }

        Ok(common::delivered(
            status["SerialNo"].as_str().map(str::to_string),
            started,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_json, header, header_exists, header_regex, method, path},
    };

    use super::*;
    use crate::domain::notify::{connector::NotifyTargetType, template::ProviderTemplate};

    fn config(base: &str) -> Value {
        json!({
            "secretId": "AKID-id",
            "secretKey": "secret",
            "region": "ap-guangzhou",
            "sdkAppId": "1400006666",
            "signName": "墨信",
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
            text: "[critical] Disk full".into(),
            markdown: None,
            html: None,
            metadata: BTreeMap::new(),
            provider_templates: BTreeMap::from([(
                PROVIDER.to_string(),
                ProviderTemplate {
                    code: "1110".into(),
                    params: BTreeMap::from([
                        ("1".to_string(), "Disk full on web-1".to_string()),
                        ("2".to_string(), "31".to_string()),
                    ]),
                    max_param_chars: None,
                },
            )]),
            event: None,
            card: None,
        }
    }

    #[tokio::test]
    async fn a_text_uses_the_app_signature_template_and_variables_by_position() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/"))
            .and(header("x-tc-action", "SendSms"))
            .and(header("x-tc-version", "2021-01-11"))
            .and(header("x-tc-region", "ap-guangzhou"))
            .and(header_exists("x-tc-timestamp"))
            .and(header_regex(
                "authorization",
                "^TC3-HMAC-SHA256 Credential=AKID-id/.*/sms/tc3_request, SignedHeaders=content-type;host;x-tc-action, Signature=[0-9a-f]{64}$",
            ))
            .and(body_json(json!({
                "PhoneNumberSet": ["+8613800138000"],
                "SmsSdkAppId": "1400006666",
                "TemplateId": "1110",
                "SignName": "墨信",
                "TemplateParamSet": ["Disk …", "31"],
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "Response": {
                    "SendStatusSet": [{
                        "SerialNo": "2433:1", "PhoneNumber": "+8613800138000", "Fee": 1,
                        "SessionContext": "", "Code": "Ok", "Message": "send success", "IsoCode": "CN",
                    }],
                    "RequestId": "r-1",
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let result = TencentSmsConnectorAdapter::new()
            .send(&config(&server.uri()), &person("13800138000"), &message())
            .await
            .unwrap();

        assert_eq!(result.provider_message_id.as_deref(), Some("2433:1"));
    }

    #[tokio::test]
    async fn a_number_refused_inside_a_successful_answer_is_a_failure() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "Response": {
                    "SendStatusSet": [{
                        "SerialNo": "", "Code": "InvalidParameterValue.TemplateParameterLengthLimit",
                        "Message": "the parameter length is over the limit",
                    }],
                    "RequestId": "r-2",
                }
            })))
            .mount(&server)
            .await;

        let error = TencentSmsConnectorAdapter::new()
            .send(&config(&server.uri()), &person("13800138000"), &message())
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("TemplateParameterLengthLimit"), "{error}");
    }

    #[tokio::test]
    async fn an_error_of_the_api_says_what_it_was() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "Response": {
                    "Error": { "Code": "AuthFailure.SignatureFailure", "Message": "bad signature" },
                    "RequestId": "r-3",
                }
            })))
            .mount(&server)
            .await;

        let error = TencentSmsConnectorAdapter::new()
            .send(&config(&server.uri()), &person("13800138000"), &message())
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("AuthFailure.SignatureFailure"), "{error}");
    }

    #[tokio::test]
    async fn without_a_template_nothing_is_sent() {
        let mut unwritten = message();
        unwritten.provider_templates.clear();

        assert!(
            TencentSmsConnectorAdapter::new()
                .send(
                    &config("http://127.0.0.1:9"),
                    &person("13800138000"),
                    &unwritten
                )
                .await
                .is_err()
        );
    }

    #[test]
    fn the_configuration_needs_the_app() {
        let adapter = TencentSmsConnectorAdapter::new();

        assert!(
            adapter
                .validate_config(&config("https://sms.tencentcloudapi.com"))
                .is_ok()
        );
        assert!(
            adapter
                .validate_config(&json!({"secretId": "a", "secretKey": "b"}))
                .is_err()
        );
        assert!(adapter.validate_target(&person("+12025550123")).is_ok());
        assert!(adapter.validate_target(&person("not a number")).is_err());
    }
}
