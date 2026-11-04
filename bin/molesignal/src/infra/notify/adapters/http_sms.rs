// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 通用 HTTP 短信：把一条短信按配置好的请求交给短信网关，给自建网关或没有专门连接器的服务商用。
//!
//! 地址、请求头与请求体里写占位符：`{{phone}}` 号码、`{{content}}` 短信内容（标题一行、正文
//! 一行），`{{title}}`、`{{text}}`，以及 `{{secret}}`（单独加密保存的密钥，免得写进模板）。值按
//! 所在的位置转义：JSON 请求体里按 JSON 字符串，表单与地址里按 URL 编码。网关回 2xx 算发出；
//! 填了成功字段时，回应 JSON 里这个字段的值还要等于成功值。

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use reqwest::{
    Method,
    header::{HeaderName, HeaderValue},
};
use serde::Deserialize;
use serde_json::Value;
use url::form_urlencoded;

use super::{common, phone};
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

pub const HTTP_SMS_CONNECTOR_TYPE: &str = "http_sms";

/// 网关回应里写进错误的最多这么多字符。
const MAX_ANSWER_CHARS: usize = 200;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RequestMethod {
    #[default]
    Post,
    Put,
    Get,
}

impl RequestMethod {
    fn reqwest(self) -> Method {
        match self {
            Self::Post => Method::POST,
            Self::Put => Method::PUT,
            Self::Get => Method::GET,
        }
    }
}

/// 请求体的写法。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum BodyFormat {
    #[default]
    Json,
    Form,
}

/// 号码交给网关的写法。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PhoneFormat {
    /// `+8613800138000`
    #[default]
    E164,
    /// `8613800138000`
    Digits,
    /// 内地号码写 `13800138000`，其他号码同 `Digits`。
    Mainland,
}

fn default_body() -> String {
    r#"{"phone": "{{phone}}", "content": "{{content}}"}"#.into()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HttpSmsConfig {
    #[serde(default)]
    url: String,
    #[serde(default)]
    method: RequestMethod,
    #[serde(default)]
    body_format: BodyFormat,
    #[serde(default = "default_body")]
    body: String,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    secret: String,
    #[serde(default)]
    phone_format: PhoneFormat,
    /// 回应 JSON 里说明成没成的字段，`a.b` 写法；空着只看 HTTP 状态。
    #[serde(default)]
    success_field: String,
    #[serde(default)]
    success_value: String,
    #[serde(default = "common::default_timeout_secs")]
    timeout_secs: u64,
}

/// 一条短信填进占位符的值。
struct Fills<'a> {
    phone: &'a str,
    content: &'a str,
    title: &'a str,
    text: &'a str,
    secret: &'a str,
}

impl Fills<'_> {
    fn get(&self, name: &str) -> Option<&str> {
        match name {
            "phone" => Some(self.phone),
            "content" => Some(self.content),
            "title" => Some(self.title),
            "text" => Some(self.text),
            "secret" => Some(self.secret),
            _ => None,
        }
    }
}

/// 检查配置时填的样例值：带引号、换行与 `&`，转义不对时模板会坏。
const SAMPLE: Fills<'static> = Fills {
    phone: "+8613800138000",
    content: "CRITICAL · \"api\" & db\nAlert is open.",
    title: "CRITICAL · \"api\" & db",
    text: "Alert is open.",
    secret: "secret",
};

/// 把 `template` 里认得的 `{{name}}` 换成转义后的值；认不得的原样留着。
fn render(template: &str, fills: &Fills<'_>, escape: fn(&str) -> String) -> String {
    let mut written = String::with_capacity(template.len());
    let mut rest = template;

    while let Some(open) = rest.find("{{") {
        written.push_str(&rest[..open]);
        let after = &rest[open + 2..];

        let filled = after.find("}}").and_then(|close| {
            fills
                .get(after[..close].trim())
                .map(|value| (escape(value), &after[close + 2..]))
        });

        match filled {
            Some((value, remainder)) => {
                written.push_str(&value);
                rest = remainder;
            }
            None => {
                written.push_str("{{");
                rest = after;
            }
        }
    }

    written.push_str(rest);
    written
}

/// JSON 字符串里的写法，不带两边的引号（模板里自己写了）。
fn json_escaped(value: &str) -> String {
    let quoted = serde_json::to_string(value).unwrap_or_default();
    quoted[1..quoted.len() - 1].to_string()
}

fn url_encoded(value: &str) -> String {
    form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

/// 请求头里不能有换行：换成空格。
fn header_safe(value: &str) -> String {
    value.replace(['\r', '\n'], " ")
}

/// 回应 JSON 里 `path`（`a.b.0` 写法）处的值，写成字符串。
fn answer_field(answer: &Value, path: &str) -> Option<String> {
    let found = path.split('.').filter(|step| !step.is_empty()).try_fold(
        answer,
        |value, step| match value {
            Value::Array(items) => step
                .parse::<usize>()
                .ok()
                .and_then(|index| items.get(index)),
            _ => value.get(step),
        },
    )?;

    Some(match found {
        Value::String(text) => text.clone(),
        Value::Null => "null".into(),
        other => other.to_string(),
    })
}

impl HttpSmsConfig {
    fn parse(value: &Value) -> Result<Self> {
        serde_json::from_value(value.clone()).map_err(|error| {
            Error::invalid(format!("invalid {HTTP_SMS_CONNECTOR_TYPE} config: {error}"))
        })
    }

    fn has_body(&self) -> bool {
        self.method != RequestMethod::Get
    }

    /// 发往的地址：地址里的占位符按 URL 编码填。
    fn url(&self, fills: &Fills<'_>) -> Result<String> {
        let url = render(self.url.trim(), fills, url_encoded);
        common::validate_http_url(&url, "http_sms url")?;
        Ok(url)
    }

    fn body(&self, fills: &Fills<'_>) -> String {
        match self.body_format {
            BodyFormat::Json => render(&self.body, fills, json_escaped),
            BodyFormat::Form => render(self.body.trim(), fills, url_encoded),
        }
    }

    fn headers(&self, fills: &Fills<'_>) -> Result<Vec<(HeaderName, HeaderValue)>> {
        self.headers
            .iter()
            .map(|(name, value)| {
                let name = HeaderName::from_bytes(name.trim().as_bytes())
                    .map_err(|_| Error::invalid("http_sms contains an invalid header name"))?;
                let value = HeaderValue::from_str(&render(value, fills, header_safe))
                    .map_err(|_| Error::invalid("http_sms contains an invalid header value"))?;
                Ok((name, value))
            })
            .collect()
    }

    fn validate(&self) -> Result<()> {
        common::require(&self.url, "http_sms url")?;
        self.url(&SAMPLE)?;
        self.headers(&SAMPLE)?;

        if self.has_body() {
            common::require(&self.body, "http_sms body")?;

            if self.body_format == BodyFormat::Json {
                serde_json::from_str::<Value>(&self.body(&SAMPLE)).map_err(|error| {
                    Error::invalid(format!(
                        "http_sms body must be JSON once its placeholders are filled: {error}"
                    ))
                })?;
            }
        }

        // 网关要知道发给谁、发什么：号码与内容总得写在请求的某处。
        let request = [
            self.url.as_str(),
            if self.has_body() { &self.body } else { "" },
            &self
                .headers
                .values()
                .cloned()
                .collect::<Vec<_>>()
                .join("\n"),
        ]
        .join("\n");

        if !request.contains("{{phone}}") {
            return Err(Error::invalid(
                "http_sms must put {{phone}} in its URL, headers or body",
            ));
        }

        if !["{{content}}", "{{title}}", "{{text}}"]
            .iter()
            .any(|placeholder| request.contains(placeholder))
        {
            return Err(Error::invalid(
                "http_sms must put {{content}} (or {{title}} and {{text}}) in its URL, headers or body",
            ));
        }

        if self.success_field.trim().is_empty() && !self.success_value.trim().is_empty() {
            return Err(Error::invalid(
                "http_sms success_value needs the success_field it is read from",
            ));
        }

        common::validate_timeout(self.timeout_secs, HTTP_SMS_CONNECTOR_TYPE)
    }
}

/// 号码按网关要的写法。
fn written_number(e164: &str, format: PhoneFormat) -> String {
    let digits = e164.trim_start_matches('+');

    match format {
        PhoneFormat::E164 => e164.to_string(),
        PhoneFormat::Digits => digits.to_string(),
        PhoneFormat::Mainland => e164.strip_prefix("+86").unwrap_or(digits).to_string(),
    }
}

#[derive(Default)]
pub struct HttpSmsConnectorAdapter;

impl HttpSmsConnectorAdapter {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ConnectorAdapter for HttpSmsConnectorAdapter {
    fn connector_type(&self) -> &'static str {
        HTTP_SMS_CONNECTOR_TYPE
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
        super::phone::replies_on(config)
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        HttpSmsConfig::parse(config)?.validate()
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        phone::target_number(
            target,
            HTTP_SMS_CONNECTOR_TYPE,
            phone::e164_or_mainland_mobile,
        )
        .map(|_| ())
    }

    async fn send(
        &self,
        config: &Value,
        target: &NotifyTarget,
        message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        let config = HttpSmsConfig::parse(config)?;
        config.validate()?;

        let e164 = phone::target_number(
            target,
            HTTP_SMS_CONNECTOR_TYPE,
            phone::e164_or_mainland_mobile,
        )?;

        let number = written_number(&e164, config.phone_format);
        let content = phone::texted(message);
        let title = phone::plain(&message.title);
        let text = phone::plain(&message.text);

        let fills = Fills {
            phone: &number,
            content: &content,
            title: &title,
            text: &text,
            secret: config.secret.trim(),
        };

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|_| Error::internal("http_sms HTTP client could not be initialized"))?;

        let mut request = client.request(config.method.reqwest(), config.url(&fills)?);

        for (name, value) in config.headers(&fills)? {
            request = request.header(name, value);
        }

        if config.has_body() {
            let content_type = match config.body_format {
                BodyFormat::Json => "application/json",
                BodyFormat::Form => "application/x-www-form-urlencoded",
            };

            request = request
                .header("content-type", content_type)
                .body(config.body(&fills));
        }

        let started = Instant::now();

        let response = http_trace::send(&client, request, HttpTarget::ThirdParty)
            .await
            .map_err(|_| Error::unavailable("http_sms request failed"))?;

        let status = response.status();
        let answer = response.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(Error::unavailable(format!(
                "http_sms gateway returned HTTP {}: {}",
                status.as_u16(),
                common::truncate_chars(answer.trim(), MAX_ANSWER_CHARS)
            )));
        }

        let field = config.success_field.trim();

        if !field.is_empty() {
            let found = serde_json::from_str::<Value>(&answer)
                .ok()
                .and_then(|answer| answer_field(&answer, field));

            if found.as_deref() != Some(config.success_value.trim()) {
                return Err(Error::unavailable(format!(
                    "http_sms gateway did not accept the text: {field} is {}, not {}",
                    found.as_deref().unwrap_or("missing"),
                    config.success_value.trim()
                )));
            }
        }

        Ok(ConnectorDeliveryResult {
            provider_message_id: None,
            delivered: true,
            latency_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            metadata: BTreeMap::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_json, body_string, header, method, path, query_param},
    };

    use super::*;
    use crate::domain::notify::connector::NotifyTargetType;

    fn target(value: &str) -> NotifyTarget {
        NotifyTarget {
            target_type: NotifyTargetType::DirectUser,
            value: value.into(),
            metadata: BTreeMap::new(),
        }
    }

    fn message() -> NotifyMessage {
        NotifyMessage {
            title: "CRITICAL · \"checkout\" & payments".into(),
            text: "Alert **Checkout errors** is open.".into(),
            markdown: None,
            html: None,
            metadata: BTreeMap::new(),
            provider_templates: Default::default(),
            event: None,
            card: None,
        }
    }

    #[tokio::test]
    async fn a_json_body_carries_the_number_and_the_escaped_text() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/send"))
            .and(header("authorization", "Bearer s3cret"))
            .and(header("content-type", "application/json"))
            .and(body_json(json!({
                "to": "13800138000",
                "msg": "CRITICAL · \"checkout\" & payments\nAlert Checkout errors is open.",
                "sign": "s3cret",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"code": 0})))
            .expect(1)
            .mount(&server)
            .await;

        let config = json!({
            "url": format!("{}/send", server.uri()),
            "body": r#"{"to": "{{phone}}", "msg": "{{content}}", "sign": "{{secret}}"}"#,
            "headers": {"Authorization": "Bearer {{secret}}"},
            "secret": "s3cret",
            "phoneFormat": "mainland",
            "successField": "code",
            "successValue": "0",
        });

        let result = HttpSmsConnectorAdapter::new()
            .send(&config, &target("+8613800138000"), &message())
            .await
            .unwrap();

        assert!(result.delivered);
    }

    #[tokio::test]
    async fn a_form_body_and_a_get_address_are_url_encoded() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/form"))
            .and(body_string(
                "mobile=8613800138000&text=CRITICAL+%C2%B7+%22checkout%22+%26+payments",
            ))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/get"))
            .and(query_param("to", "+12065550123"))
            .and(query_param("msg", "Alert Checkout errors is open."))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        let adapter = HttpSmsConnectorAdapter::new();

        adapter
            .send(
                &json!({
                    "url": format!("{}/form", server.uri()),
                    "bodyFormat": "form",
                    "body": "mobile={{phone}}&text={{title}}",
                    "phoneFormat": "digits",
                }),
                &target("13800138000"),
                &message(),
            )
            .await
            .unwrap();

        adapter
            .send(
                &json!({
                    "url": format!("{}/get?to={{{{phone}}}}&msg={{{{text}}}}", server.uri()),
                    "method": "get",
                }),
                &target("+12065550123"),
                &message(),
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_gateway_that_says_no_is_a_failure() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"result": {"status": "REJECTED"}})),
            )
            .mount(&server)
            .await;

        let error = HttpSmsConnectorAdapter::new()
            .send(
                &json!({
                    "url": server.uri(),
                    "successField": "result.status",
                    "successValue": "OK",
                }),
                &target("+8613800138000"),
                &message(),
            )
            .await
            .unwrap_err()
            .to_string();

        assert!(
            error.contains("result.status is REJECTED, not OK"),
            "{error}"
        );
    }

    #[test]
    fn a_request_that_cannot_say_who_or_what_is_refused() {
        let adapter = HttpSmsConnectorAdapter::new();
        let valid = json!({"url": "https://sms.example.com/send"});

        assert!(adapter.validate_config(&valid).is_ok());

        let no_phone = json!({
            "url": "https://sms.example.com/send",
            "body": r#"{"content": "{{content}}"}"#,
        });
        assert!(adapter.validate_config(&no_phone).is_err());

        let not_json = json!({
            "url": "https://sms.example.com/send",
            "body": r#"{"phone": {{phone}}, "content": "{{content}}"}"#,
        });
        assert!(adapter.validate_config(&not_json).is_err());

        let value_without_field = json!({
            "url": "https://sms.example.com/send",
            "successValue": "0",
        });
        assert!(adapter.validate_config(&value_without_field).is_err());

        assert!(
            adapter
                .validate_config(&json!({"url": "ftp://sms.example.com"}))
                .is_err()
        );
    }

    #[test]
    fn numbers_and_answers_are_read_the_way_the_gateway_writes_them() {
        assert_eq!(
            written_number("+8613800138000", PhoneFormat::Mainland),
            "13800138000"
        );
        assert_eq!(
            written_number("+12065550123", PhoneFormat::Mainland),
            "12065550123"
        );
        assert_eq!(
            written_number("+8613800138000", PhoneFormat::Digits),
            "8613800138000"
        );

        let answer = json!({"data": [{"ok": true}], "code": 0});
        assert_eq!(answer_field(&answer, "code").as_deref(), Some("0"));
        assert_eq!(answer_field(&answer, "data.0.ok").as_deref(), Some("true"));
        assert_eq!(answer_field(&answer, "missing"), None);
    }
}
