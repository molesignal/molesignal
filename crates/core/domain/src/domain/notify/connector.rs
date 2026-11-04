// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::template::{NotifyCard, ProviderTemplate};
use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorStatus {
    #[default]
    Unknown,
    Connected,
    Error,
}

impl ConnectorStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Connected => "connected",
            Self::Error => "error",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "unknown" => Ok(Self::Unknown),
            "connected" => Ok(Self::Connected),
            "error" => Ok(Self::Error),
            other => Err(Error::internal(format!(
                "unknown notify connector status: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorTestStatus {
    #[default]
    Success,
    Failed,
}

impl ConnectorTestStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "success" => Ok(Self::Success),
            "failed" => Ok(Self::Failed),
            other => Err(Error::internal(format!(
                "unknown notify connector test status: {other}"
            ))),
        }
    }
}

/// 连接器发往的渠道。通知模板按渠道分，通知策略可以给每个渠道另选模板。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotifyChannel {
    Im,
    Phone,
    Sms,
    Email,
    Webhook,
}

impl NotifyChannel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Im => "im",
            Self::Phone => "phone",
            Self::Sms => "sms",
            Self::Email => "email",
            Self::Webhook => "webhook",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "im" => Ok(Self::Im),
            "phone" => Ok(Self::Phone),
            "sms" => Ok(Self::Sms),
            "email" => Ok(Self::Email),
            "webhook" => Ok(Self::Webhook),
            other => Err(Error::invalid(format!("unknown notify channel: {other}"))),
        }
    }

    /// 电话与短信只发一段纯文本：这两个渠道的模板正文就是发出去的全部内容，不再加上标题。
    pub const fn plain_text(self) -> bool {
        matches!(self, Self::Phone | Self::Sms)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorCapabilities {
    pub direct_user: bool,
    pub group: bool,
    pub rich_text: bool,
    pub interactive: bool,
    pub acknowledgement: bool,
    pub attachments: bool,
}

/// 企业级发送连接器。`config` 是运行时解密后的配置，只允许在受控 service /
/// adapter 路径中使用，序列化时永不输出。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotifyConnector {
    pub id: Id,
    pub organization_id: Id,
    pub name: String,
    pub connector_type: String,
    #[serde(skip_serializing)]
    pub config: Value,
    pub capabilities: ConnectorCapabilities,
    pub enabled: bool,
    pub status: ConnectorStatus,
    pub last_tested_at: Option<TimestampMicros>,
    pub last_test_status: Option<ConnectorTestStatus>,
    pub last_test_error: Option<String>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotifyTargetType {
    DirectUser,
    FixedAddress,
    FixedGroup,
    Webhook,
}

impl NotifyTargetType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DirectUser => "direct_user",
            Self::FixedAddress => "fixed_address",
            Self::FixedGroup => "fixed_group",
            Self::Webhook => "webhook",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotifyTarget {
    pub target_type: NotifyTargetType,
    pub value: String,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotifyMessage {
    pub title: String,
    pub text: String,
    #[serde(default)]
    pub markdown: Option<String>,
    #[serde(default)]
    pub html: Option<String>,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
    /// 写给只发审核过模板的服务商的那几份，按服务商的名字，变量已经填好。通知模板里写了才有。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub provider_templates: BTreeMap<String, ProviderTemplate>,
    /// 即时通讯的 App 与群机器人发成的卡片，占位符已经填好。即时通讯模板里写了才有。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<NotifyCard>,
    /// 写给机器读的事件：Webhook 的默认请求体里带上它（版本、事件、告警、规则、升级等）。只在发往
    /// Webhook 渠道时由通知引擎填上。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<serde_json::Value>,
}

/// 分区域的连接器能用的一个区域：区域 ID、写给人看的名称，以及在这个区域时请求发往的地址。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorRegion {
    pub id: String,
    pub name: String,
    pub api_base_url: String,
}

/// 连接器配置里服务商的一个地址：`field` 这个键只能取这里列出的地址。连接器里只读显示；保存、测试与
/// 直发前按它查，免得凭证被发到别处。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorAddress {
    /// 配置里的键，例如 `apiBaseUrl`、`serviceUrl`、`wsUrl`。
    pub field: &'static str,
    /// 有几个地址时在连接器里按什么选：`site`（平台）、`cloud`（云环境）。它本身不保存。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picker: Option<&'static str>,
    /// 能取的地址，第一个是默认的。
    pub choices: Vec<ConnectorAddressChoice>,
    /// 随所选的区域定的地址：取服务商为这个区域列出的，`choices` 只是还没选区域时的地址。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by_region: Option<RegionHost>,
}

/// 地址目录里的一个地址：`key` 是它在 `picker` 里的名字，只有一个地址时是 `default`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConnectorAddressChoice {
    pub key: &'static str,
    pub url: &'static str,
}

/// 随区域定的地址：`https://{service}.{区域}.{domains 之一}`，区域是配置里的 `region`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RegionHost {
    pub service: &'static str,
    pub domains: &'static [&'static str],
}

impl ConnectorAddress {
    /// 只有一个地址。
    pub fn fixed(field: &'static str, url: &'static str) -> Self {
        Self {
            field,
            picker: None,
            choices: vec![ConnectorAddressChoice {
                key: "default",
                url,
            }],
            by_region: None,
        }
    }

    /// 按 `picker` 在几个地址里选，第一个是默认的。
    pub fn picked(
        field: &'static str,
        picker: &'static str,
        choices: &[(&'static str, &'static str)],
    ) -> Self {
        Self {
            field,
            picker: Some(picker),
            choices: choices
                .iter()
                .map(|&(key, url)| ConnectorAddressChoice { key, url })
                .collect(),
            by_region: None,
        }
    }

    /// 随所选的区域定；`default` 是还没选区域时的地址，没有的空着由连接器按区域推出来。
    pub fn by_region(field: &'static str, host: RegionHost, default: Option<&'static str>) -> Self {
        Self {
            field,
            picker: None,
            choices: default
                .map(|url| ConnectorAddressChoice {
                    key: "default",
                    url,
                })
                .into_iter()
                .collect(),
            by_region: Some(host),
        }
    }

    /// `config` 里这个键的值收不收：没填或空着（用默认的）、列出的地址之一（末尾的 `/` 不算），或随区域
    /// 定的配置里那个区域的地址。
    pub fn admits(&self, config: &Value) -> bool {
        let value = match config.get(self.field) {
            None | Some(Value::Null) => return true,
            Some(Value::String(value)) => value.trim().trim_end_matches('/'),
            Some(_) => return false,
        };

        if value.is_empty()
            || self
                .choices
                .iter()
                .any(|choice| choice.url.trim_end_matches('/') == value)
        {
            return true;
        }

        let Some(host) = &self.by_region else {
            return false;
        };

        let region = config
            .get("region")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default();

        let well_formed = !region.is_empty()
            && region
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');

        well_formed
            && value.strip_prefix("https://").is_some_and(|named| {
                host.domains
                    .iter()
                    .any(|domain| named == format!("{}.{region}.{domain}", host.service))
            })
    }
}

/// `config` 里服务商的地址都在 `addresses` 里；有一个不在就拒绝，说是哪个键。
pub fn validate_addresses(
    connector_type: &str,
    addresses: &[ConnectorAddress],
    config: &Value,
) -> Result<()> {
    match addresses.iter().find(|address| !address.admits(config)) {
        Some(address) => Err(Error::invalid(format!(
            "{connector_type} {} must be one of the provider's addresses",
            address.field
        ))),
        None => Ok(()),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorDeliveryResult {
    pub provider_message_id: Option<String>,
    pub delivered: bool,
    pub latency_ms: u64,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

/// 品牌差异的唯一扩展点。通知路由层只按 `connector_type` 查注册表，不直接
/// 判断 Email、Slack、Lark 等具体品牌。
#[async_trait]
pub trait ConnectorAdapter: Send + Sync {
    fn connector_type(&self) -> &'static str;
    fn capabilities(&self) -> ConnectorCapabilities;
    /// 发往哪个渠道：投递时按它挑通知策略给这个渠道另选的模板。
    fn channel(&self) -> NotifyChannel;
    /// 只发服务商审核过的模板的连接器（阿里云的短信与语音）给出服务商的名字：通知模板里写给它的那一份
    /// 按这个名字找，没有就发不出去。其他连接器发通知本身的内容，是 `None`。
    fn template_provider(&self) -> Option<&'static str> {
        None
    }
    /// 打电话的连接器给出合并窗口（秒）：同一个号码一个窗口里只拨一次，其间的通知合并到窗口结束时的
    /// 那一通。其他连接器每条通知各发各的。
    fn merge_window_secs(&self) -> Option<u32> {
        None
    }
    /// 这个连接器发出的短信或电话收不收回复（短信回复代码、电话按键），看配置；默认不收。
    fn takes_replies(&self, config: &Value) -> bool {
        let _ = config;
        false
    }
    /// 配置里服务商的地址，各能取哪些值（见 [`ConnectorAddress`]）。没有的是地址由人填的连接器，
    /// 例如 Webhook 与 SMTP。
    fn addresses(&self) -> Vec<ConnectorAddress> {
        Vec::new()
    }
    fn validate_config(&self, config: &Value) -> Result<()>;
    fn validate_target(&self, target: &NotifyTarget) -> Result<()>;

    /// 分区域的连接器（阿里云、AWS）能用的区域，向厂商的接口查，不在代码里写死；名称用
    /// `language`（`zh-cn`、`en-us`）。要凭证时从 `connector` 的配置里读，还没保存的配置也行。
    async fn regions(
        &self,
        connector: &NotifyConnector,
        language: &str,
    ) -> Result<Vec<ConnectorRegion>> {
        let _ = (connector, language);

        Err(Error::invalid(format!(
            "{} has no regions to choose from",
            self.connector_type()
        )))
    }

    async fn send(
        &self,
        config: &Value,
        target: &NotifyTarget,
        message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult>;

    /// 以 `connector` 的身份发送。投递、测试与直发都走这里；大多数连接器只用到它的配置，
    /// 凭证取决于是哪个组织的哪个连接器的（例如以组织的 External ID 扮演 AWS 角色）才读其余部分。
    async fn send_as(
        &self,
        connector: &NotifyConnector,
        target: &NotifyTarget,
        message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        self.send(&connector.config, target, message).await
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const TEAMS: &[(&str, &str)] = &[
        ("global", "https://smba.trafficmanager.net/teams/"),
        ("gcc", "https://smba.infra.gcc.teams.microsoft.com/teams"),
    ];

    #[test]
    fn an_address_is_one_the_catalogue_lists_or_left_to_its_default() {
        let teams = [ConnectorAddress::picked("serviceUrl", "cloud", TEAMS)];
        let admitted = |config: Value| validate_addresses("teams_app", &teams, &config).is_ok();

        assert!(admitted(json!({})));
        assert!(admitted(json!({ "serviceUrl": "" })));
        assert!(admitted(
            json!({ "serviceUrl": "https://smba.trafficmanager.net/teams" })
        ));
        assert!(admitted(json!({
            "serviceUrl": " https://smba.infra.gcc.teams.microsoft.com/teams/ "
        })));
        assert!(!admitted(
            json!({ "serviceUrl": "https://attacker.example/teams/" })
        ));
        assert!(!admitted(
            json!({ "serviceUrl": "http://smba.trafficmanager.net/teams/" })
        ));
        assert!(!admitted(json!({ "serviceUrl": 1 })));

        let refused = validate_addresses(
            "teams_app",
            &teams,
            &json!({ "serviceUrl": "https://10.0.0.1/" }),
        );
        assert!(refused.unwrap_err().to_string().contains("serviceUrl"));
    }

    #[test]
    fn a_regional_address_is_the_providers_host_for_the_chosen_region() {
        let aws = ConnectorAddress::by_region(
            "apiBaseUrl",
            RegionHost {
                service: "sms-voice",
                domains: &["amazonaws.com", "amazonaws.com.cn"],
            },
            None,
        );
        let aliyun = ConnectorAddress::by_region(
            "apiBaseUrl",
            RegionHost {
                service: "dysmsapi",
                domains: &["aliyuncs.com"],
            },
            Some("https://dysmsapi.aliyuncs.com"),
        );
        let at = |region: &str, url: &str| json!({ "region": region, "apiBaseUrl": url });

        assert!(aws.admits(&at("eu-west-1", "")));
        assert!(aws.admits(&at(
            "eu-west-1",
            "https://sms-voice.eu-west-1.amazonaws.com/"
        )));
        assert!(aws.admits(&at(
            "cn-north-1",
            "https://sms-voice.cn-north-1.amazonaws.com.cn"
        )));
        assert!(!aws.admits(&at(
            "eu-west-1",
            "https://sms-voice.us-east-1.amazonaws.com"
        )));
        assert!(!aws.admits(&at("eu-west-1", "https://sms-voice.s3.amazonaws.com")));
        assert!(!aws.admits(&at(
            "a.example/x",
            "https://sms-voice.a.example/x.amazonaws.com"
        )));

        assert!(aliyun.admits(&at("cn-beijing", "https://dysmsapi.aliyuncs.com")));
        assert!(aliyun.admits(&at(
            "ap-southeast-1",
            "https://dysmsapi.ap-southeast-1.aliyuncs.com"
        )));
        assert!(!aliyun.admits(&at(
            "ap-southeast-1",
            "http://dysmsapi.ap-southeast-1.aliyuncs.com"
        )));
        assert!(!aliyun.admits(&at(
            "oss-cn-hangzhou",
            "https://dysmsapi.oss-cn-hangzhou.aliyun.com"
        )));
    }
}
