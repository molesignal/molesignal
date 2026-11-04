// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 阿里云短信：`SendSms` 用审核过的签名与模板发给一个号码，内地号码与国际号码都可以。签名跟着连接器，
//! 模板编号与变量写在通知策略选的短信模板里。

use std::time::Instant;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use super::{Account, PROVIDER, approved_template, call, common, phone, regions, template_param};
use crate::{
    domain::notify::connector::{
        ConnectorAdapter, ConnectorAddress, ConnectorCapabilities, ConnectorDeliveryResult,
        ConnectorRegion, NotifyChannel, NotifyConnector, NotifyMessage, NotifyTarget,
    },
    shared::{Error, Result},
};

pub const ALIYUN_SMS_CONNECTOR_TYPE: &str = "aliyun_sms";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SmsConfig {
    #[serde(flatten)]
    account: Account,
    #[serde(default)]
    sign_name: String,
    #[serde(default = "default_api_base_url")]
    api_base_url: String,
}

const ALIYUN_SMS_API_BASE_URL: &str = "https://dysmsapi.aliyuncs.com";

fn default_api_base_url() -> String {
    ALIYUN_SMS_API_BASE_URL.into()
}

impl SmsConfig {
    fn parse(value: &Value) -> Result<Self> {
        serde_json::from_value(value.clone()).map_err(|error| {
            Error::invalid(format!(
                "invalid {ALIYUN_SMS_CONNECTOR_TYPE} config: {error}"
            ))
        })
    }

    fn validate(&self) -> Result<()> {
        self.account.validate(ALIYUN_SMS_CONNECTOR_TYPE)?;
        common::require(&self.sign_name, "aliyun_sms signName")?;
        common::validate_http_url(&self.api_base_url, "aliyun_sms apiBaseUrl")
    }
}

/// 短信发往的号码：内地号码写 11 位，其他地区写国家码加号码，都不带 `+`。
fn phone_number(target: &NotifyTarget) -> Result<String> {
    let number = phone::target_number(
        target,
        ALIYUN_SMS_CONNECTOR_TYPE,
        phone::e164_or_mainland_mobile,
    )?;

    let national = number
        .strip_prefix("+86")
        .or_else(|| number.strip_prefix('+'))
        .unwrap_or(&number);

    Ok(national.to_string())
}

#[derive(Default)]
pub struct AliyunSmsConnectorAdapter {
    /// 列地域用的元数据地址；空着是阿里云的门户。
    meta_base_url: Option<String>,
}

impl AliyunSmsConnectorAdapter {
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
impl ConnectorAdapter for AliyunSmsConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        ALIYUN_SMS_CONNECTOR_TYPE
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
        vec![super::address("dysmsapi", ALIYUN_SMS_API_BASE_URL)]
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
        _connector: &NotifyConnector,
        language: &str,
    ) -> Result<Vec<ConnectorRegion>> {
        regions(
            ALIYUN_SMS_CONNECTOR_TYPE,
            self.meta_base_url.as_deref(),
            "Dysmsapi",
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
        let template = approved_template(ALIYUN_SMS_CONNECTOR_TYPE, message, "SMS")?;

        let mut params = vec![
            ("PhoneNumbers", phone_number(target)?),
            ("SignName", config.sign_name.trim().to_string()),
            ("TemplateCode", template.code.trim().to_string()),
        ];

        if let Some(param) = template_param(template) {
            params.push(("TemplateParam", param));
        }

        let started = Instant::now();

        let answer = call(
            ALIYUN_SMS_CONNECTOR_TYPE,
            &config.account,
            &config.api_base_url,
            "SendSms",
            &params,
        )
        .await?;

        let biz_id = answer
            .get("BizId")
            .and_then(Value::as_str)
            .map(str::to_string);

        Ok(common::delivered(biz_id, started))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{header, method, path, query_param},
    };

    use super::{super::tests::draft, *};
    use crate::domain::notify::{connector::NotifyTargetType, template::ProviderTemplate};

    fn target(target_type: NotifyTargetType, value: &str) -> NotifyTarget {
        NotifyTarget {
            target_type,
            value: value.into(),
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn numbers_are_written_the_way_aliyun_reads_them() {
        assert_eq!(
            phone_number(&target(NotifyTargetType::DirectUser, "+8613800138000")).unwrap(),
            "13800138000"
        );
        assert_eq!(
            phone_number(&target(NotifyTargetType::FixedAddress, "+85291234567")).unwrap(),
            "85291234567"
        );
        assert!(phone_number(&target(NotifyTargetType::FixedGroup, "13800138000")).is_err());
    }

    #[tokio::test]
    async fn a_text_uses_the_signature_template_and_variables() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/"))
            .and(header("x-acs-action", "SendSms"))
            .and(query_param("PhoneNumbers", "13800138000"))
            .and(query_param("SignName", "墨信"))
            .and(query_param("TemplateCode", "SMS_1"))
            .and(query_param("TemplateParam", r#"{"name":"Disk full"}"#))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"Code": "OK", "Message": "OK", "BizId": "9006^0"})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let config = json!({
            "accessKeyId": "LTAI-id",
            "accessKeySecret": "secret",
            "signName": "墨信",
            "apiBaseUrl": server.uri(),
        });

        let mut message = NotifyMessage {
            title: String::new(),
            text: "[critical] Disk full".into(),
            markdown: None,
            html: None,
            metadata: BTreeMap::new(),
            provider_templates: Default::default(),
            event: None,
            card: None,
        };

        let adapter = AliyunSmsConnectorAdapter::new();
        let phone = target(NotifyTargetType::DirectUser, "13800138000");

        // 通知模板里没写阿里云的模板编号：不发。
        assert!(adapter.send(&config, &phone, &message).await.is_err());

        message.provider_templates.insert(
            PROVIDER.into(),
            ProviderTemplate {
                code: "SMS_1".into(),
                params: BTreeMap::from([("name".to_string(), "Disk full".to_string())]),
                max_param_chars: None,
            },
        );

        let result = adapter.send(&config, &phone, &message).await.unwrap();

        assert_eq!(result.provider_message_id.as_deref(), Some("9006^0"));
    }

    #[tokio::test]
    async fn regions_and_their_addresses_come_from_the_alibaba_cloud_portal() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/products/Dysmsapi/endpoints.json"))
            .and(query_param("language", "zh_CN"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": 0,
                "data": {
                    "type": "regional",
                    "endpoints": [
                        {"regionId": "cn-hangzhou", "regionName": "华东1（杭州）", "public": "dysmsapi.aliyuncs.com"},
                        {"regionId": "ap-southeast-1", "regionName": "新加坡", "public": "dysmsapi.ap-southeast-1.aliyuncs.com"},
                        {"regionName": "没有地域 ID", "public": "dysmsapi.aliyuncs.com"},
                    ],
                },
            })))
            .expect(1)
            .mount(&server)
            .await;

        let regions = AliyunSmsConnectorAdapter::with_meta_base_url(server.uri())
            .regions(&draft(ALIYUN_SMS_CONNECTOR_TYPE), "zh-cn")
            .await
            .unwrap();

        assert_eq!(
            regions,
            vec![
                ConnectorRegion {
                    id: "cn-hangzhou".into(),
                    name: "华东1（杭州）".into(),
                    api_base_url: "https://dysmsapi.aliyuncs.com".into(),
                },
                ConnectorRegion {
                    id: "ap-southeast-1".into(),
                    name: "新加坡".into(),
                    api_base_url: "https://dysmsapi.ap-southeast-1.aliyuncs.com".into(),
                },
            ]
        );
    }
}
