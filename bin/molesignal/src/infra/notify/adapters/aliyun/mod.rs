// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 阿里云的语音通知（[`voice`]）与短信（[`sms`]）。两者都是 RPC 风格的 OpenAPI（版本 2017-05-25）：
//! 参数放在查询串里，按 V3 签名（[`sign`]），不引 SDK。阿里云只发审核过的模板：模板编号与每个变量
//! 填什么写在通知模板里（服务商的名字是 [`PROVIDER`]），连接器只管 AccessKey、地域与签名。
//!
//! 能选的地域与各地域的接入地址向阿里云 OpenAPI 门户的公开元数据查（[`regions`]），不写死。

mod sign;
mod sms;
mod voice;

use aws_lc_rs::rand::{SecureRandom, SystemRandom};
use serde::Deserialize;
use serde_json::{Map, Value};
pub use sms::{ALIYUN_SMS_CONNECTOR_TYPE, AliyunSmsConnectorAdapter};
use url::Url;
pub use voice::{ALIYUN_VOICE_CONNECTOR_TYPE, AliyunVoiceConnectorAdapter};

use super::{
    common::{self, default_timeout_secs},
    phone,
};
use crate::{
    domain::notify::{
        connector::{ConnectorAddress, ConnectorRegion, NotifyMessage, RegionHost},
        template::ProviderTemplate,
    },
    shared::{
        Error, Result,
        http_trace::{self, HttpTarget},
    },
};

const API_VERSION: &str = "2017-05-25";

/// 阿里云 OpenAPI 门户的公开元数据：每个产品在哪些地域开服、各地域的公网接入地址，地域名称按语言
/// 写，不用凭证。
const META_BASE_URL: &str = "https://api.aliyun.com/meta/v1";

/// 通知模板里写给阿里云的那一份用这个名字。
pub(crate) const PROVIDER: &str = "aliyun";

/// 通知类模板的单个变量默认最多 35 个字；企业用户可以向阿里云申请放宽，放宽后在通知模板里写上限。
const DEFAULT_MAX_PARAM_CHARS: u32 = 35;

/// 两种连接器共用的：AccessKey 与超时。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Account {
    #[serde(default)]
    access_key_id: String,
    #[serde(default)]
    access_key_secret: String,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
}

impl Account {
    fn validate(&self, connector: &str) -> Result<()> {
        common::require(&self.access_key_id, &format!("{connector} accessKeyId"))?;
        common::require(
            &self.access_key_secret,
            &format!("{connector} accessKeySecret"),
        )?;

        common::validate_timeout(self.timeout_secs, connector)
    }
}

/// 通知模板里写给阿里云的模板编号与变量。阿里云只发审核过的模板，通知模板里没写就发不出去。
fn approved_template<'a>(
    connector: &str,
    message: &'a NotifyMessage,
    kind: &str,
) -> Result<&'a ProviderTemplate> {
    message
        .provider_templates
        .get(PROVIDER)
        .filter(|template| !template.code.trim().is_empty())
        .ok_or_else(|| {
            Error::invalid(format!(
                "{connector} sends only templates Alibaba Cloud approved: give the {kind} template \
                 of the notify policy an Alibaba Cloud template code"
            ))
        })
}

/// 模板变量的 JSON：去掉 Markdown 记号，超长的截断。模板没有变量时是 `None`。
fn template_param(template: &ProviderTemplate) -> Option<String> {
    let max_chars = template.max_param_chars.unwrap_or(DEFAULT_MAX_PARAM_CHARS) as usize;

    let params = template
        .params
        .iter()
        .map(|(name, value)| {
            let value = common::truncate_chars(&phone::plain(value), max_chars);
            (name.clone(), Value::String(value))
        })
        .collect::<Map<_, _>>();

    (!params.is_empty()).then(|| Value::Object(params).to_string())
}

/// 调一次 RPC 接口：参数放在查询串里，请求体为空，按 V3 签名。应答的 `Code` 不是 `OK` 就是被拒。
async fn call(
    connector: &str,
    account: &Account,
    api_base_url: &str,
    action: &str,
    params: &[(&str, String)],
) -> Result<Value> {
    let mut url = Url::parse(api_base_url.trim())
        .map_err(|_| Error::invalid(format!("{connector} apiBaseUrl must be an absolute URL")))?;

    let host = match (url.host_str(), url.port()) {
        (Some(host), Some(port)) => format!("{host}:{port}"),
        (Some(host), None) => host.to_string(),
        (None, _) => {
            return Err(Error::invalid(format!(
                "{connector} apiBaseUrl must name a host"
            )));
        }
    };

    let query = sign::canonical_query(params);
    url.set_path("/");
    url.set_query(Some(&query));

    let payload_hash = sign::sha256_hex(b"");
    let date = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let nonce = nonce()?;

    let headers = [
        ("host", host.as_str()),
        ("x-acs-action", action),
        ("x-acs-version", API_VERSION),
        ("x-acs-date", date.as_str()),
        ("x-acs-signature-nonce", nonce.as_str()),
        ("x-acs-content-sha256", payload_hash.as_str()),
    ];

    let authorization = sign::authorization(
        account.access_key_id.trim(),
        account.access_key_secret.trim(),
        &sign::Request {
            method: "POST",
            query: &query,
            headers: &headers,
            payload_hash: &payload_hash,
        },
    );

    let client = common::client(account.timeout_secs, connector)?;

    let mut request = client
        .post(url)
        .header("accept", "application/json")
        .header("authorization", authorization);

    // `host` 由 HTTP 客户端按地址填写，与签进去的一致。
    for (name, value) in &headers[1..] {
        request = request.header(*name, *value);
    }

    let response = http_trace::send(&client, request, HttpTarget::ThirdParty)
        .await
        .map_err(|_| Error::unavailable(format!("{connector} request failed")))?;

    let status = response.status();

    let answer = response.json::<Value>().await.map_err(|_| {
        Error::unavailable(format!("{connector} returned HTTP {}", status.as_u16()))
    })?;

    let code = answer
        .get("Code")
        .and_then(Value::as_str)
        .unwrap_or_default();

    if status.is_success() && code == "OK" {
        return Ok(answer);
    }

    let code = if code.is_empty() {
        format!("HTTP {}", status.as_u16())
    } else {
        code.to_string()
    };

    let message = answer
        .get("Message")
        .and_then(Value::as_str)
        .unwrap_or_default();

    Err(Error::unavailable(format!(
        "{connector} was refused: {code}: {}",
        common::provider_message(message)
    )))
}

/// 随地域的接入地址：`https://{service}.{地域}.aliyuncs.com`，或不带地域的 `default`（还没选地域时
/// 用它，内地的地域大多也是它）。
fn address(service: &'static str, default: &'static str) -> ConnectorAddress {
    ConnectorAddress::by_region(
        "apiBaseUrl",
        RegionHost {
            service,
            domains: &["aliyuncs.com"],
        },
        Some(default),
    )
}

/// `product`（`Dysmsapi`、`Dyvmsapi`）开服的地域与各自的接入地址，名称用 `language`（中文或英文）。
/// `meta_base_url` 空着是阿里云的门户，测试时换成本机的服务。
async fn regions(
    connector: &str,
    meta_base_url: Option<&str>,
    product: &str,
    language: &str,
) -> Result<Vec<ConnectorRegion>> {
    let base = meta_base_url.unwrap_or(META_BASE_URL).trim_end_matches('/');

    let language = if language.to_ascii_lowercase().starts_with("zh") {
        "zh_CN"
    } else {
        "en_US"
    };

    let client = common::client(default_timeout_secs(), connector)?;

    let response = http_trace::send(
        &client,
        client.get(format!(
            "{base}/products/{product}/endpoints.json?language={language}"
        )),
        HttpTarget::ThirdParty,
    )
    .await
    .map_err(|_| {
        Error::unavailable(format!(
            "{connector} could not reach Alibaba Cloud to list its regions"
        ))
    })?;

    let status = response.status();

    let body = response.json::<Value>().await.map_err(|_| {
        Error::unavailable(format!(
            "{connector} regions: Alibaba Cloud returned HTTP {}",
            status.as_u16()
        ))
    })?;

    if !status.is_success() || body["code"].as_i64() != Some(0) {
        return Err(Error::unavailable(format!(
            "{connector} regions: Alibaba Cloud returned HTTP {} with code {}",
            status.as_u16(),
            body["code"]
        )));
    }

    let regions = body["data"]["endpoints"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|endpoint| {
            let id = endpoint["regionId"].as_str().map(str::trim)?;
            let host = endpoint["public"]
                .as_str()
                .or_else(|| endpoint["endpoint"].as_str())
                .map(str::trim)?;

            if id.is_empty() || host.is_empty() {
                return None;
            }

            let name = endpoint["regionName"]
                .as_str()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .unwrap_or(id);

            Some(ConnectorRegion {
                id: id.to_string(),
                name: name.to_string(),
                api_base_url: format!("https://{host}"),
            })
        })
        .collect::<Vec<_>>();

    if regions.is_empty() {
        return Err(Error::unavailable(format!(
            "{connector} regions: Alibaba Cloud listed none for {product}"
        )));
    }

    Ok(regions)
}

/// 每个请求不同的随机串，防重放。
fn nonce() -> Result<String> {
    let mut bytes = [0u8; 16];

    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| Error::internal("aliyun signature nonce could not be generated"))?;

    Ok(hex::encode(bytes))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;

    fn account(config: Value) -> Account {
        serde_json::from_value(config).unwrap()
    }

    /// 还没保存的连接器：列地域不用凭证。
    pub(super) fn draft(connector_type: &str) -> crate::domain::notify::connector::NotifyConnector {
        crate::domain::notify::connector::NotifyConnector {
            id: crate::shared::ids::Id::from_string("draft"),
            organization_id: crate::shared::ids::Id::from_string("org-1"),
            name: String::new(),
            connector_type: connector_type.into(),
            config: json!({}),
            capabilities: Default::default(),
            enabled: true,
            status: Default::default(),
            last_tested_at: None,
            last_test_status: None,
            last_test_error: None,
            created_at: crate::shared::time::TimestampMicros(1),
            updated_at: crate::shared::time::TimestampMicros(1),
        }
    }

    #[test]
    fn the_variables_come_from_the_notify_template() {
        let mut message = NotifyMessage {
            title: String::new(),
            text: "CRITICAL · Checkout errors rose above five percent".into(),
            markdown: None,
            html: None,
            metadata: BTreeMap::new(),
            provider_templates: BTreeMap::new(),
            event: None,
            card: None,
        };

        // 通知模板里没写阿里云的模板：发不出去。
        assert!(approved_template("aliyun_sms", &message, "SMS").is_err());

        message.provider_templates.insert(
            PROVIDER.into(),
            ProviderTemplate {
                code: "SMS_1".into(),
                params: BTreeMap::from([
                    ("name".to_string(), "**Checkout errors** rose".to_string()),
                    (
                        "detail".to_string(),
                        "Error rate above five percent".to_string(),
                    ),
                ]),
                max_param_chars: Some(20),
            },
        );

        let template = approved_template("aliyun_sms", &message, "SMS").unwrap();
        let param: Value = serde_json::from_str(&template_param(template).unwrap()).unwrap();

        assert_eq!(param["name"], "Checkout errors rose");
        assert_eq!(param["detail"], "Error rate above fi…");

        let no_params = ProviderTemplate {
            code: "SMS_2".into(),
            params: BTreeMap::new(),
            max_param_chars: None,
        };
        assert!(template_param(&no_params).is_none());
    }

    #[test]
    fn the_account_needs_both_keys() {
        let valid = json!({"accessKeyId": "id", "accessKeySecret": "secret"});
        assert!(account(valid).validate("aliyun_sms").is_ok());

        assert!(
            account(json!({"accessKeyId": "id"}))
                .validate("aliyun_sms")
                .is_err()
        );
    }
}
