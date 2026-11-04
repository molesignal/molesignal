// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 华为云消息&短信：`/sms/batchSendSms/v1` 发给一个号码，表单请求，X-WSSE 认证。通道号跟着连接器（通用模板
//! 还要写签名名称），模板 ID 与变量写在通知策略选的短信模板里。整体成功不等于这个号码发出去了，要看它的
//! `status`。

use std::time::Instant;

use async_trait::async_trait;
use aws_lc_rs::digest;
use base64::Engine;
use serde::Deserialize;
use serde_json::Value;

use super::{
    Application, PROVIDER, approved_template, common, created_now, nonce, phone, template_params,
};
use crate::{
    domain::notify::connector::{
        ConnectorAdapter, ConnectorCapabilities, ConnectorDeliveryResult, NotifyChannel,
        NotifyMessage, NotifyTarget,
    },
    shared::{
        Error, Result,
        http_trace::{self, HttpTarget},
    },
};

pub const HUAWEI_SMS_CONNECTOR_TYPE: &str = "huawei_sms";

/// 单个变量默认最多多少个字：上限写在各自的模板变量上（如 `TXT_20`），这里按常见的取，放宽写在通知模板里。
const DEFAULT_MAX_PARAM_CHARS: u32 = 20;

const SUCCESS: &str = "000000";

#[derive(Debug, Clone, Deserialize)]
struct SmsConfig {
    #[serde(flatten)]
    application: Application,
    /// 通道号（`from`）：签名对应的那个。
    #[serde(default)]
    sender: String,
    /// 签名名称：只有通用模板要写。
    #[serde(default)]
    signature: String,
}

impl SmsConfig {
    fn parse(value: &Value) -> Result<Self> {
        serde_json::from_value(value.clone()).map_err(|error| {
            Error::invalid(format!(
                "invalid {HUAWEI_SMS_CONNECTOR_TYPE} config: {error}"
            ))
        })
    }

    fn validate(&self) -> Result<()> {
        self.application.validate(HUAWEI_SMS_CONNECTOR_TYPE)?;
        common::require(&self.sender, "huawei_sms sender")
    }
}

/// `PasswordDigest`：`Base64(SHA256(Nonce + Created + AppSecret))`。
fn wsse(app_key: &str, app_secret: &str, nonce: &str, created: &str) -> String {
    let digest = digest::digest(
        &digest::SHA256,
        format!("{nonce}{created}{app_secret}").as_bytes(),
    );

    let password_digest = base64::engine::general_purpose::STANDARD.encode(digest.as_ref());

    format!(
        "UsernameToken Username=\"{app_key}\",PasswordDigest=\"{password_digest}\",Nonce=\"{nonce}\",Created=\"{created}\""
    )
}

fn phone_number(target: &NotifyTarget) -> Result<String> {
    phone::target_number(
        target,
        HUAWEI_SMS_CONNECTOR_TYPE,
        phone::e164_or_mainland_mobile,
    )
}

#[derive(Default)]
pub struct HuaweiSmsConnectorAdapter;

impl HuaweiSmsConnectorAdapter {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ConnectorAdapter for HuaweiSmsConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        HUAWEI_SMS_CONNECTOR_TYPE
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

    fn template_provider(&self) -> Option<&'static str> {
        Some(PROVIDER)
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        SmsConfig::parse(config)?.validate()
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        phone_number(target).map(|_| ())
    }

    async fn send(
        &self,
        config: &Value,
        target: &NotifyTarget,
        message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        let config = SmsConfig::parse(config)?;
        config.validate()?;
        let template = approved_template(HUAWEI_SMS_CONNECTOR_TYPE, message, "SMS")?;
        let application = &config.application;

        let params = template_params(template, DEFAULT_MAX_PARAM_CHARS);
        let params = serde_json::to_string(&params)
            .map_err(|_| Error::internal("huawei_sms template variables could not be written"))?;

        let mut form = vec![
            ("from", config.sender.trim().to_string()),
            ("to", phone_number(target)?),
            ("templateId", template.code.trim().to_string()),
            ("templateParas", params),
        ];

        if !config.signature.trim().is_empty() {
            form.push(("signature", config.signature.trim().to_string()));
        }

        let client = common::client(application.timeout_secs, HUAWEI_SMS_CONNECTOR_TYPE)?;
        let started = Instant::now();

        let request = client
            .post(application.endpoint("/sms/batchSendSms/v1"))
            .header(
                "authorization",
                r#"WSSE realm="SDP",profile="UsernameToken",type="Appkey""#,
            )
            .header(
                "x-wsse",
                wsse(
                    application.app_key.trim(),
                    application.app_secret.trim(),
                    &nonce()?,
                    &created_now(),
                ),
            )
            .form(&form);

        let response = http_trace::send(&client, request, HttpTarget::ThirdParty)
            .await
            .map_err(|_| Error::unavailable("huawei_sms request failed"))?;

        let status = response.status();

        let answer = response.json::<Value>().await.map_err(|_| {
            Error::unavailable(format!("huawei_sms returned HTTP {}", status.as_u16()))
        })?;

        let code = answer["code"].as_str().unwrap_or_default();

        if !status.is_success() || code != SUCCESS {
            return Err(Error::unavailable(format!(
                "huawei_sms was refused: {}: {}",
                if code.is_empty() { "HTTP error" } else { code },
                common::provider_message(answer["description"].as_str().unwrap_or_default())
            )));
        }

        let result = &answer["result"][0];
        let number_status = result["status"].as_str().unwrap_or_default();

        if number_status != SUCCESS {
            return Err(Error::unavailable(format!(
                "huawei_sms was refused for this number: {number_status}"
            )));
        }

        Ok(common::delivered(
            result["smsMsgId"].as_str().map(str::to_string),
            started,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_string_contains, header_regex, method, path},
    };

    use super::*;
    use crate::domain::notify::{connector::NotifyTargetType, template::ProviderTemplate};

    fn config(base: &str) -> Value {
        json!({
            "appKey": "app-key",
            "appSecret": "app-secret",
            "apiBaseUrl": base,
            "sender": "csms100000001",
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
            text: "Disk full".into(),
            markdown: None,
            html: None,
            metadata: BTreeMap::new(),
            provider_templates: BTreeMap::from([(
                PROVIDER.to_string(),
                ProviderTemplate {
                    code: "tmpl-1".into(),
                    params: BTreeMap::from([("1".to_string(), "Disk full".to_string())]),
                    max_param_chars: None,
                },
            )]),
            event: None,
            card: None,
        }
    }

    #[test]
    fn the_digest_is_the_sha256_of_nonce_created_and_secret() {
        // 用同样的算法在别处（Python 的 hashlib）算出来对照。
        assert_eq!(
            wsse("app-key", "app-secret", "n0nce", "2026-10-31T12:00:00Z"),
            "UsernameToken Username=\"app-key\",\
             PasswordDigest=\"Bx5Ud4dpmExTDQ0LGeEZ2Uzjw/2m2p3VNmsCcfioekI=\",\
             Nonce=\"n0nce\",Created=\"2026-10-31T12:00:00Z\""
        );
    }

    #[tokio::test]
    async fn a_text_is_a_form_with_the_channel_template_and_variables() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/sms/batchSendSms/v1"))
            .and(header_regex(
                "authorization",
                r#"^WSSE realm="SDP",profile="UsernameToken",type="Appkey"$"#,
            ))
            .and(header_regex(
                "x-wsse",
                r#"^UsernameToken Username="app-key",PasswordDigest="[^"]+",Nonce="[0-9a-f]+",Created="\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z"$"#,
            ))
            .and(body_string_contains("from=csms100000001"))
            .and(body_string_contains("to=%2B8613800138000"))
            .and(body_string_contains("templateId=tmpl-1"))
            .and(body_string_contains("templateParas=%5B%22Disk+full%22%5D"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": "000000",
                "description": "Success",
                "result": [{
                    "smsMsgId": "m-1", "from": "csms100000001", "originTo": "+8613800138000",
                    "status": "000000", "createTime": "2026-10-31T12:00:00Z",
                }],
            })))
            .expect(1)
            .mount(&server)
            .await;

        let result = HuaweiSmsConnectorAdapter::new()
            .send(&config(&server.uri()), &person("13800138000"), &message())
            .await
            .unwrap();

        assert_eq!(result.provider_message_id.as_deref(), Some("m-1"));
    }

    #[tokio::test]
    async fn a_refusal_says_the_code_whether_for_the_request_or_the_number() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": "000000",
                "description": "Success",
                "result": [{ "smsMsgId": "m", "status": "E200028" }],
            })))
            .up_to_n_times(1)
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "code": "E000002", "description": "Invalid Key",
            })))
            .mount(&server)
            .await;

        let adapter = HuaweiSmsConnectorAdapter::new();
        let phone = person("13800138000");

        let number = adapter
            .send(&config(&server.uri()), &phone, &message())
            .await
            .unwrap_err()
            .to_string();
        assert!(number.contains("E200028"), "{number}");

        let request = adapter
            .send(&config(&server.uri()), &phone, &message())
            .await
            .unwrap_err()
            .to_string();
        assert!(request.contains("E000002"), "{request}");
        assert!(request.contains("Invalid Key"), "{request}");
    }

    #[test]
    fn the_configuration_needs_the_channel() {
        let adapter = HuaweiSmsConnectorAdapter::new();

        assert!(
            adapter
                .validate_config(&config("https://smsapi.example.com"))
                .is_ok()
        );
        assert!(
            adapter
                .validate_config(&json!({
                    "appKey": "k", "appSecret": "s", "apiBaseUrl": "https://smsapi.example.com"
                }))
                .is_err()
        );
        assert!(adapter.takes_replies(&json!({"replies": true})));
    }
}
