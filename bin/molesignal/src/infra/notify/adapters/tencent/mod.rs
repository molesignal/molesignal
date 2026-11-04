// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 腾讯云的语音消息（[`voice`]）与短信（[`sms`]）：API 3.0，JSON 请求体，按 TC3-HMAC-SHA256 签名
//! （[`sign`]），不引 SDK。和阿里云一样只发审核过的模板：模板 ID 与变量写在通知模板里（服务商的名字是
//! [`PROVIDER`]）。腾讯云的模板变量按位置填，通知模板里变量名写 `1`、`2`、`3`。
//!
//! 要留意的限制：短信变量默认最多 6 个字符（2024-01-25 以后开通的账号和长期闲置的账号，控制台可申请放宽，
//! 放宽后写在通知模板的上限里）；语音是预付费的，套餐用完会被拒（`InsufficientBalanceInVoicePackage`）。
//!
//! 能选的地域用连接器的凭证向腾讯云查（[`regions`]），不写死。

mod sign;
mod sms;
mod voice;

use serde::Deserialize;
use serde_json::{Value, json};
pub use sms::{TENCENT_SMS_CONNECTOR_TYPE, TencentSmsConnectorAdapter};
use url::Url;
pub use voice::{TENCENT_VOICE_CONNECTOR_TYPE, TencentVoiceConnectorAdapter};

use super::{
    common::{self, default_timeout_secs},
    phone,
};
use crate::{
    domain::notify::{
        connector::{
            ConnectorAddress, ConnectorRegion, NotifyConnector, NotifyMessage, RegionHost,
        },
        template::ProviderTemplate,
    },
    shared::{
        Error, Result,
        http_trace::{self, HttpTarget},
    },
};

/// 通知模板里写给腾讯云的那一份用这个名字。
pub(crate) const PROVIDER: &str = "tencent";

/// 查地域的接口。
const REGION_BASE_URL: &str = "https://region.tencentcloudapi.com";

const CONTENT_TYPE: &str = "application/json; charset=utf-8";

fn default_region() -> String {
    "ap-guangzhou".into()
}

/// 两种连接器共用的：API 密钥、地域与超时。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Account {
    #[serde(default)]
    secret_id: String,
    #[serde(default)]
    secret_key: String,
    #[serde(default = "default_region")]
    region: String,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
}

impl Account {
    fn validate(&self, connector: &str) -> Result<()> {
        common::require(&self.secret_id, &format!("{connector} secretId"))?;
        common::require(&self.secret_key, &format!("{connector} secretKey"))?;
        common::require(&self.region, &format!("{connector} region"))?;

        common::validate_timeout(self.timeout_secs, connector)
    }
}

/// 一个 API 的版本和服务名。
#[derive(Clone, Copy)]
struct Api {
    service: &'static str,
    version: &'static str,
}

const SMS_API: Api = Api {
    service: "sms",
    version: "2021-01-11",
};

const VOICE_API: Api = Api {
    service: "vms",
    version: "2020-09-02",
};

const REGION_API: Api = Api {
    service: "region",
    version: "2022-06-27",
};

/// 通知模板里写给腾讯云的模板 ID 与变量。腾讯云只发审核过的模板，通知模板里没写就发不出去。
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
                "{connector} sends only templates Tencent Cloud approved: give the {kind} template \
                 of the notify policy a Tencent Cloud template ID"
            ))
        })
}

/// 模板变量按位置填：变量名是数字的按数字排，其余的按名字排在后面。去掉 Markdown 记号，超长的截断。
fn template_params(template: &ProviderTemplate, default_max_chars: u32) -> Vec<String> {
    let max_chars = template.max_param_chars.unwrap_or(default_max_chars) as usize;

    let mut named: Vec<(&String, &String)> = template.params.iter().collect();

    named.sort_by_key(|(name, _)| match name.parse::<u32>() {
        Ok(position) => (0, position, name.to_string()),
        Err(_) => (1, 0, name.to_string()),
    });

    named
        .into_iter()
        .map(|(_, value)| common::truncate_chars(&phone::plain(value), max_chars))
        .collect()
}

/// 调哪个 API 的哪个动作。
struct ApiCall<'a> {
    api: Api,
    base_url: &'a str,
    action: &'a str,
    region: Option<&'a str>,
    language: Option<&'a str>,
}

/// 调一次 API。应答的 `Response.Error` 有值就是被拒。
async fn call(
    connector: &str,
    account: &Account,
    target: ApiCall<'_>,
    payload: &Value,
) -> Result<Value> {
    let ApiCall {
        api,
        base_url,
        action,
        region,
        language,
    } = target;

    let url = Url::parse(base_url.trim())
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

    let body = payload.to_string();
    let timestamp = chrono::Utc::now().timestamp();

    let authorization = sign::authorization(&sign::SignedCall {
        secret_id: account.secret_id.trim(),
        secret_key: account.secret_key.trim(),
        service: api.service,
        host: &host,
        action,
        content_type: CONTENT_TYPE,
        payload: &body,
        timestamp,
    });

    let client = common::client(account.timeout_secs, connector)?;

    let mut request = client
        .post(url)
        .header("content-type", CONTENT_TYPE)
        .header("authorization", authorization)
        .header("x-tc-action", action)
        .header("x-tc-version", api.version)
        .header("x-tc-timestamp", timestamp.to_string());

    if let Some(region) = region {
        request = request.header("x-tc-region", region);
    }

    if let Some(language) = language {
        request = request.header("x-tc-language", language);
    }

    let response = http_trace::send(&client, request.body(body), HttpTarget::ThirdParty)
        .await
        .map_err(|_| Error::unavailable(format!("{connector} request failed")))?;

    let status = response.status();

    let answer = response.json::<Value>().await.map_err(|_| {
        Error::unavailable(format!("{connector} returned HTTP {}", status.as_u16()))
    })?;

    let error = &answer["Response"]["Error"];

    if !error.is_null() || !status.is_success() {
        return Err(Error::unavailable(format!(
            "{connector} was refused: {}: {}",
            error["Code"].as_str().unwrap_or("HTTP error"),
            common::provider_message(error["Message"].as_str().unwrap_or_default())
        )));
    }

    Ok(answer)
}

/// 随地域的接入地址：`https://{service}.{地域}.tencentcloudapi.com`；还没选地域时是就近接入的地址。
fn address(service: &'static str, default: &'static str) -> ConnectorAddress {
    ConnectorAddress::by_region(
        "apiBaseUrl",
        RegionHost {
            service,
            domains: &["tencentcloudapi.com"],
        },
        Some(default),
    )
}

/// `product`（`sms`、`vms`）开服的地域与各自的接入地址，用连接器的凭证查，名称用 `language`。
/// `region_base_url` 空着是腾讯云的接口，测试时换成本机的服务。
async fn regions(
    connector_type: &str,
    connector: &NotifyConnector,
    region_base_url: Option<&str>,
    product: &'static str,
    language: &str,
) -> Result<Vec<ConnectorRegion>> {
    let account: Account = serde_json::from_value(connector.config.clone())
        .map_err(|error| Error::invalid(format!("invalid {connector_type} config: {error}")))?;

    common::require(&account.secret_id, &format!("{connector_type} secretId"))?;
    common::require(&account.secret_key, &format!("{connector_type} secretKey"))?;

    let language = if language.to_ascii_lowercase().starts_with("zh") {
        "zh-CN"
    } else {
        "en-US"
    };

    let answer = call(
        connector_type,
        &account,
        ApiCall {
            api: REGION_API,
            base_url: region_base_url.unwrap_or(REGION_BASE_URL),
            action: "DescribeRegions",
            region: None,
            language: Some(language),
        },
        &json!({ "Product": product }),
    )
    .await?;

    let regions = answer["Response"]["RegionSet"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|region| region["RegionState"].as_str().unwrap_or("AVAILABLE") == "AVAILABLE")
        .filter_map(|region| {
            let id = region["Region"].as_str().map(str::trim)?;

            if id.is_empty() {
                return None;
            }

            let name = region["RegionName"]
                .as_str()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .unwrap_or(id);

            Some(ConnectorRegion {
                id: id.to_string(),
                name: name.to_string(),
                api_base_url: format!("https://{product}.{id}.tencentcloudapi.com"),
            })
        })
        .collect::<Vec<_>>();

    if regions.is_empty() {
        return Err(Error::unavailable(format!(
            "{connector_type} regions: Tencent Cloud listed none for {product}"
        )));
    }

    Ok(regions)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{header, header_regex, method},
    };

    use super::*;

    pub(super) fn draft(
        connector_type: &str,
        config: Value,
    ) -> crate::domain::notify::connector::NotifyConnector {
        crate::domain::notify::connector::NotifyConnector {
            id: crate::shared::ids::Id::from_string("draft"),
            organization_id: crate::shared::ids::Id::from_string("org-1"),
            name: String::new(),
            connector_type: connector_type.into(),
            config,
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
    fn variables_are_filled_by_position() {
        let template = ProviderTemplate {
            code: "1110".into(),
            params: BTreeMap::from([
                ("10".to_string(), "tenth".to_string()),
                ("2".to_string(), "**second** line".to_string()),
                ("1".to_string(), "first value that is long".to_string()),
            ]),
            max_param_chars: Some(8),
        };

        assert_eq!(
            template_params(&template, 6),
            vec!["first v…", "second …", "tenth"]
        );

        let default_limit = ProviderTemplate {
            max_param_chars: None,
            ..template
        };

        assert_eq!(template_params(&default_limit, 6)[0], "first…");
    }

    #[test]
    fn the_account_needs_both_keys() {
        let parse = |config: Value| serde_json::from_value::<Account>(config).unwrap();

        assert!(
            parse(json!({"secretId": "id", "secretKey": "key"}))
                .validate("tencent_sms")
                .is_ok()
        );
        assert!(
            parse(json!({"secretId": "id"}))
                .validate("tencent_sms")
                .is_err()
        );
    }

    #[tokio::test]
    async fn regions_are_the_ones_tencent_cloud_lists_for_the_product() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(header("x-tc-action", "DescribeRegions"))
            .and(header("x-tc-version", "2022-06-27"))
            .and(header("x-tc-language", "zh-CN"))
            .and(header_regex("authorization", "/region/tc3_request"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "Response": {
                    "TotalCount": 3,
                    "RegionSet": [
                        {"Region": "ap-guangzhou", "RegionName": "华南地区(广州)", "RegionState": "AVAILABLE"},
                        {"Region": "ap-beijing", "RegionName": "华北地区(北京)", "RegionState": "AVAILABLE"},
                        {"Region": "ap-gone", "RegionName": "下线了", "RegionState": "UNAVAILABLE"},
                    ],
                    "RequestId": "r-1",
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let listed = regions(
            "tencent_sms",
            &draft("tencent_sms", json!({"secretId": "id", "secretKey": "key"})),
            Some(&server.uri()),
            "sms",
            "zh-cn",
        )
        .await
        .unwrap();

        assert_eq!(
            listed,
            vec![
                ConnectorRegion {
                    id: "ap-guangzhou".into(),
                    name: "华南地区(广州)".into(),
                    api_base_url: "https://sms.ap-guangzhou.tencentcloudapi.com".into(),
                },
                ConnectorRegion {
                    id: "ap-beijing".into(),
                    name: "华北地区(北京)".into(),
                    api_base_url: "https://sms.ap-beijing.tencentcloudapi.com".into(),
                },
            ]
        );
    }

    #[tokio::test]
    async fn regions_need_the_credentials() {
        assert!(
            regions(
                "tencent_sms",
                &draft("tencent_sms", json!({})),
                Some("http://127.0.0.1:9"),
                "sms",
                "en-us",
            )
            .await
            .is_err()
        );
    }
}
