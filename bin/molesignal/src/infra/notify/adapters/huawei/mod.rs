// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 华为云的语音通知（[`voice`]）与消息&短信（[`sms`]）。和阿里云、腾讯云一样只发审核过的模板：模板 ID 与
//! 变量写在通知模板里（服务商的名字是 [`PROVIDER`]），变量按位置填，变量名写 `1`、`2`、`3`。
//!
//! 接入地址随应用定，只能从控制台的「应用管理」抄，没有接口能查，所以是填的（`api_base_url`），不是选的。
//! 两个产品的认证都用应用的 AppKey、AppSecret，请求里只带摘要，不带 AppSecret 本身。

mod sms;
mod voice;

use aws_lc_rs::rand::{SecureRandom, SystemRandom};
use serde::Deserialize;
pub use sms::{HUAWEI_SMS_CONNECTOR_TYPE, HuaweiSmsConnectorAdapter};
pub use voice::{HUAWEI_VOICE_CONNECTOR_TYPE, HuaweiVoiceConnectorAdapter};

use super::{
    common::{self, default_timeout_secs},
    phone,
};
use crate::{
    domain::notify::{connector::NotifyMessage, template::ProviderTemplate},
    shared::{Error, Result},
};

/// 通知模板里写给华为云的那一份用这个名字。
pub(crate) const PROVIDER: &str = "huawei";

/// 两种连接器共用的：应用的凭证、接入地址与超时。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Application {
    #[serde(default)]
    app_key: String,
    #[serde(default)]
    app_secret: String,
    #[serde(default)]
    api_base_url: String,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
}

impl Application {
    fn validate(&self, connector: &str) -> Result<()> {
        common::require(&self.app_key, &format!("{connector} appKey"))?;
        common::require(&self.app_secret, &format!("{connector} appSecret"))?;
        common::require(&self.api_base_url, &format!("{connector} apiBaseUrl"))?;
        common::validate_http_url(&self.api_base_url, &format!("{connector} apiBaseUrl"))?;

        common::validate_timeout(self.timeout_secs, connector)
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}{path}", self.api_base_url.trim().trim_end_matches('/'))
    }
}

/// 通知模板里写给华为云的模板 ID 与变量。华为云只发审核过的模板，通知模板里没写就发不出去。
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
                "{connector} sends only templates Huawei Cloud approved: give the {kind} template \
                 of the notify policy a Huawei Cloud template ID"
            ))
        })
}

/// 模板变量按位置填：变量名是数字的按数字排，其余的按名字排在后面。去掉 Markdown 记号和华为云不收的
/// `$`、`{`、`}`，超长的截断。
fn template_params(template: &ProviderTemplate, default_max_chars: u32) -> Vec<String> {
    let max_chars = template.max_param_chars.unwrap_or(default_max_chars) as usize;

    let mut named: Vec<(&String, &String)> = template.params.iter().collect();

    named.sort_by_key(|(name, _)| match name.parse::<u32>() {
        Ok(position) => (0, position, name.to_string()),
        Err(_) => (1, 0, name.to_string()),
    });

    named
        .into_iter()
        .map(|(_, value)| {
            let plain = phone::plain(value).replace(['$', '{', '}'], "");

            common::truncate_chars(&plain, max_chars)
        })
        .collect()
}

/// 摘要用的时间：UTC，到秒。
fn created_now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// 每个请求不同的随机串。
fn nonce() -> Result<String> {
    let mut bytes = [0u8; 16];

    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| Error::internal("huawei signature nonce could not be generated"))?;

    Ok(hex::encode(bytes))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;

    #[test]
    fn variables_are_filled_by_position_without_what_huawei_refuses() {
        let template = ProviderTemplate {
            code: "t1".into(),
            params: BTreeMap::from([
                ("2".to_string(), "${cost} **high**".to_string()),
                ("1".to_string(), "a value that is much too long".to_string()),
            ]),
            max_param_chars: Some(10),
        };

        assert_eq!(
            template_params(&template, 32),
            vec!["a value t…", "cost high"]
        );
    }

    #[test]
    fn the_application_needs_its_keys_and_its_address() {
        let parse =
            |config: serde_json::Value| serde_json::from_value::<Application>(config).unwrap();

        let fine = parse(json!({
            "appKey": "k", "appSecret": "s",
            "apiBaseUrl": "https://smsapi.cn-north-4.myhuaweicloud.com:443/",
        }));

        assert!(fine.validate("huawei_sms").is_ok());
        assert_eq!(
            fine.endpoint("/sms/batchSendSms/v1"),
            "https://smsapi.cn-north-4.myhuaweicloud.com:443/sms/batchSendSms/v1"
        );
        assert!(
            parse(json!({"appKey": "k", "appSecret": "s"}))
                .validate("huawei_sms")
                .is_err()
        );
        assert!(
            parse(json!({"appSecret": "s", "apiBaseUrl": "https://x.example"}))
                .validate("huawei_sms")
                .is_err()
        );
    }
}
