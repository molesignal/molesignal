// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! AWS End User Messaging 的语音（[`voice`]）与短信（[`sms`]）：SMS and Voice v2 API（AWS JSON 1.0
//! 协议），按 SigV4 签名，不引 AWS SDK。凭证与数据接入的 AWS 连接器是同一套（`auth`：访问密钥、
//! 临时密钥、节点身份、扮演角色或 Web Identity），扮演角色时用的是组织的 External ID，所以发送要
//! 知道是哪个组织的哪个连接器（[`ConnectorAdapter::send_as`]）。
//!
//! 能选的区域用连接器的凭证向 AWS 查（[`regions`]），不写死。
//!
//! [`ConnectorAdapter::send_as`]: crate::domain::notify::connector::ConnectorAdapter::send_as

mod sms;
mod voice;

use serde::Deserialize;
use serde_json::Value;
pub use sms::{AWS_SMS_CONNECTOR_TYPE, AwsSmsConnectorAdapter};
use url::Url;
pub use voice::{AWS_VOICE_CONNECTOR_TYPE, AwsVoiceConnectorAdapter};

use super::common::{self, default_timeout_secs};
use crate::{
    domain::notify::connector::{ConnectorAddress, ConnectorRegion, NotifyConnector, RegionHost},
    infra::connectors::{
        aws::{
            self as aws_identity, CredentialSource, Signer,
            sigv4::{self, SigningKey},
        },
        config::aws::AwsAuth,
    },
    shared::{
        Error, Result,
        http_trace::{self, HttpTarget},
    },
};

/// SigV4 签名用的服务名与 `X-Amz-Target` 的前缀。
const SERVICE: &str = "sms-voice";
const TARGET_PREFIX: &str = "PinpointSMSVoiceV2";

/// 列区域用的 EC2 查询接口的版本。
const EC2_VERSION: &str = "2016-11-15";

/// 像 `us-east-1` 这样的区域名：小写字母、数字与连字符。
fn well_formed_region(region: &str) -> bool {
    !region.is_empty()
        && region
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// 请求发往区域的 End User Messaging 端点 `https://sms-voice.{区域}.{分区的域名}`；空着由区域推出来。
fn address() -> ConnectorAddress {
    ConnectorAddress::by_region(
        "apiBaseUrl",
        RegionHost {
            service: "sms-voice",
            domains: &aws_identity::PARTITION_DOMAINS,
        },
        None,
    )
}

/// 两种连接器共用的：区域、发送方（号码、发送方 ID 或号码池）与传输设置。凭证另由 `auth` 读出。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Account {
    #[serde(default)]
    region: String,
    #[serde(default)]
    origination_identity: String,
    #[serde(default)]
    api_base_url: String,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
}

impl Account {
    fn parse(value: &Value, connector: &str) -> Result<Self> {
        serde_json::from_value(value.clone())
            .map_err(|error| Error::invalid(format!("invalid {connector} config: {error}")))
    }

    /// `config` 是整份配置：凭证的写法由 [`AwsAuth::parse`] 检查。
    fn validate(&self, config: &Value, connector: &str) -> Result<()> {
        if !well_formed_region(self.region.trim()) {
            return Err(Error::invalid(format!(
                "{connector} region must be an AWS region, like us-east-1"
            )));
        }

        AwsAuth::parse(config)?;

        if !self.api_base_url.trim().is_empty() {
            common::validate_http_url(&self.api_base_url, &format!("{connector} apiBaseUrl"))?;
        }

        common::validate_timeout(self.timeout_secs, connector)
    }

    /// 请求发往的地址：没有另填就是区域的 End User Messaging 端点。
    fn endpoint(&self) -> String {
        match self.api_base_url.trim() {
            "" => format!(
                "https://{}/",
                aws_identity::sms_voice_host(self.region.trim())
            ),
            base => format!("{}/", base.trim_end_matches('/')),
        }
    }
}

/// 以 `connector` 的身份调一次 `operation`，返回消息的 `MessageId`。
async fn send_message(
    connector_type: &str,
    connector: &NotifyConnector,
    account: &Account,
    operation: &str,
    body: &Value,
) -> Result<Option<String>> {
    let auth = AwsAuth::parse(&connector.config)?;
    let region = account.region.trim();

    let signer = Signer {
        id: &connector.id,
        org_id: &connector.organization_id,
        updated_at: connector.updated_at,
    };

    let credential =
        aws_identity::signer_credentials(signer, &auth, region, CredentialSource::Kept)?
            .get_credential()
            .await
            .map_err(|error| Error::unauthorized(format!("aws credentials: {error}")))?;

    let url = Url::parse(&account.endpoint()).map_err(|_| {
        Error::invalid(format!(
            "{connector_type} apiBaseUrl must be an absolute URL"
        ))
    })?;

    let host = match (url.host_str(), url.port()) {
        (Some(host), Some(port)) => format!("{host}:{port}"),
        (Some(host), None) => host.to_string(),
        (None, _) => {
            return Err(Error::invalid(format!(
                "{connector_type} apiBaseUrl must name a host"
            )));
        }
    };

    let target = format!("{TARGET_PREFIX}.{operation}");
    let body = body.to_string();

    let headers = [
        ("content-type", "application/x-amz-json-1.0"),
        ("x-amz-target", target.as_str()),
    ];

    let signed = sigv4::sign_post(
        &SigningKey {
            access_key: &credential.key_id,
            secret_key: &credential.secret_key,
            session_token: credential.token.as_deref(),
        },
        region,
        SERVICE,
        &host,
        &headers,
        body.as_bytes(),
        &sigv4::amz_date_now(),
    );

    let client = common::client(account.timeout_secs, connector_type)?;

    let mut request = client
        .post(url)
        .header("content-type", headers[0].1)
        .header("x-amz-target", &target)
        .header("x-amz-date", &signed.amz_date)
        .header("authorization", &signed.authorization);

    if let Some(token) = credential
        .token
        .as_deref()
        .filter(|token| !token.is_empty())
    {
        request = request.header("x-amz-security-token", token);
    }

    let response = http_trace::send(&client, request.body(body), HttpTarget::ThirdParty)
        .await
        .map_err(|_| Error::unavailable(format!("{connector_type} request failed")))?;

    let status = response.status();
    let answer = response.json::<Value>().await.unwrap_or(Value::Null);

    if status.is_success() {
        return Ok(answer
            .get("MessageId")
            .and_then(Value::as_str)
            .map(str::to_string));
    }

    // `__type` 可能带命名空间，如 `com.amazonaws.pinpointsmsvoicev2#ValidationException`。
    let kind = answer
        .get("__type")
        .and_then(Value::as_str)
        .and_then(|kind| kind.rsplit('#').next())
        .unwrap_or("error");

    let message = answer
        .get("message")
        .or_else(|| answer.get("Message"))
        .and_then(Value::as_str)
        .unwrap_or_default();

    Err(Error::unavailable(format!(
        "{connector_type} was refused: HTTP {} {kind}: {}",
        status.as_u16(),
        common::provider_message(message)
    )))
}

/// 账号开通了的区域与各区域的 End User Messaging 地址。AWS 没有按服务列区域的公开接口，这里用 EC2
/// 的 `DescribeRegions`，凭证要有 `ec2:DescribeRegions`；没开这项服务的区域，测试时会报错。请求发往
/// 配置里的区域（没填是 us-east-1），凭证按还没保存的配置现取。`ec2_base_url` 是测试换成本机服务用的。
async fn regions(
    connector_type: &str,
    connector: &NotifyConnector,
    ec2_base_url: Option<&str>,
) -> Result<Vec<ConnectorRegion>> {
    let account = Account::parse(&connector.config, connector_type)?;
    let auth = AwsAuth::parse(&connector.config)?;

    let region = match account.region.trim() {
        region if well_formed_region(region) => region,
        _ => "us-east-1",
    };

    let signer = Signer {
        id: &connector.id,
        org_id: &connector.organization_id,
        updated_at: connector.updated_at,
    };

    let credential =
        aws_identity::signer_credentials(signer, &auth, region, CredentialSource::Fresh)?
            .get_credential()
            .await
            .map_err(|error| Error::unauthorized(format!("aws credentials: {error}")))?;

    let url = match ec2_base_url {
        Some(base) => format!("{}/", base.trim_end_matches('/')),
        None => format!("https://{}/", aws_identity::ec2_host(region)),
    };

    let parsed = Url::parse(&url)
        .map_err(|_| Error::invalid(format!("{connector_type} regions: invalid EC2 address")))?;

    let host = match (parsed.host_str(), parsed.port()) {
        (Some(host), Some(port)) => format!("{host}:{port}"),
        (Some(host), None) => host.to_string(),
        (None, _) => {
            return Err(Error::invalid(format!(
                "{connector_type} regions: the EC2 address names no host"
            )));
        }
    };

    let body = format!("Action=DescribeRegions&Version={EC2_VERSION}");
    let headers = [(
        "content-type",
        "application/x-www-form-urlencoded; charset=utf-8",
    )];

    let signed = sigv4::sign_post(
        &SigningKey {
            access_key: &credential.key_id,
            secret_key: &credential.secret_key,
            session_token: credential.token.as_deref(),
        },
        region,
        "ec2",
        &host,
        &headers,
        body.as_bytes(),
        &sigv4::amz_date_now(),
    );

    let client = common::client(account.timeout_secs, connector_type)?;

    let mut request = client
        .post(url)
        .header("content-type", headers[0].1)
        .header("x-amz-date", &signed.amz_date)
        .header("authorization", &signed.authorization);

    if let Some(token) = credential
        .token
        .as_deref()
        .filter(|token| !token.is_empty())
    {
        request = request.header("x-amz-security-token", token);
    }

    let response = http_trace::send(&client, request.body(body), HttpTarget::ThirdParty)
        .await
        .map_err(|_| {
            Error::unavailable(format!(
                "{connector_type} could not reach AWS to list its regions"
            ))
        })?;

    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    let document = roxmltree::Document::parse(&text).ok();

    // 第一个叫 `name` 的元素里的字。
    let first = |name: &str| {
        document
            .as_ref()
            .and_then(|document| {
                document
                    .descendants()
                    .find(|node| node.has_tag_name(name))
                    .and_then(|node| node.text())
            })
            .map(str::trim)
            .unwrap_or_default()
            .to_string()
    };

    if !status.is_success() {
        return Err(Error::unavailable(format!(
            "{connector_type} could not list AWS regions (it needs ec2:DescribeRegions): HTTP {} {}: {}",
            status.as_u16(),
            first("Code"),
            common::provider_message(&first("Message"))
        )));
    }

    let mut regions = document
        .as_ref()
        .map(|document| {
            document
                .descendants()
                .filter(|node| node.has_tag_name("regionName"))
                .filter_map(|node| node.text().map(str::trim))
                .filter(|id| well_formed_region(id))
                .map(|id| ConnectorRegion {
                    id: id.to_string(),
                    name: id.to_string(),
                    api_base_url: format!("https://{}", aws_identity::sms_voice_host(id)),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if regions.is_empty() {
        return Err(Error::unavailable(format!(
            "{connector_type} regions: AWS listed none"
        )));
    }

    regions.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(regions)
}

/// 只有配置、不知道是哪个连接器时发不了：扮演角色要用组织的 External ID。
fn needs_the_connector(connector_type: &str) -> Error {
    Error::internal(format!(
        "{connector_type} sends as its connector: the organization decides the role's external ID"
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{
        domain::notify::connector::{ConnectorCapabilities, ConnectorStatus},
        shared::{ids::Id, time::TimestampMicros},
    };

    pub(super) fn connector(id: &str, config: Value) -> NotifyConnector {
        NotifyConnector {
            id: Id::from_string(id),
            organization_id: Id::from_string("org-1"),
            name: "aws".into(),
            connector_type: "aws_sms".into(),
            config,
            capabilities: ConnectorCapabilities::default(),
            enabled: true,
            status: ConnectorStatus::Unknown,
            last_tested_at: None,
            last_test_status: None,
            last_test_error: None,
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
        }
    }

    pub(super) fn key(base: &str) -> Value {
        json!({
            "region": "us-east-1",
            "auth": "access_key",
            "accessKey": "AKIAEXAMPLE",
            "secretKey": "secret",
            "originationIdentity": "+12065550100",
            "apiBaseUrl": base,
        })
    }

    #[test]
    fn the_region_and_the_way_of_signing_are_checked() {
        let account = |config: &Value| Account::parse(config, "aws_sms").unwrap();
        let valid = key("");

        assert!(account(&valid).validate(&valid, "aws_sms").is_ok());
        assert_eq!(
            account(&valid).endpoint(),
            "https://sms-voice.us-east-1.amazonaws.com/"
        );

        let mut no_region = key("");
        no_region["region"] = json!("");
        assert!(account(&no_region).validate(&no_region, "aws_sms").is_err());

        let mut half_key = key("");
        half_key["secretKey"] = json!("");
        assert!(account(&half_key).validate(&half_key, "aws_sms").is_err());

        let role = json!({"region": "cn-north-1", "auth": "assume_role", "roleArn": "arn:aws:iam::123456789012:role/notify"});
        assert!(account(&role).validate(&role, "aws_sms").is_ok());
        assert_eq!(
            account(&role).endpoint(),
            "https://sms-voice.cn-north-1.amazonaws.com.cn/"
        );
    }
}
