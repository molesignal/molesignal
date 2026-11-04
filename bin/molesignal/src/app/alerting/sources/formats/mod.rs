// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 各家告警格式：把一次请求的请求体读成 [`ExternalAlert`]。
//!
//! 去重键里带上发送方的账号、站点或实例：Alertmanager 与 Grafana 的地址、Datadog 的站点和组织、
//! PagerDuty 的账号域名、阿里云和腾讯云的账号、CloudWatch 告警的 ARN。一个来源同时接好几个 region、
//! 账号或站点时不会撞。发送方的时间不管带不带时区、是秒还是毫秒，都换成 UTC 微秒。

use std::collections::BTreeMap;

use serde_json::Value;

use crate::{
    domain::alerting::source::{AlertSource, AlertSourceKind, ExternalAlert},
    shared::{Error, Result, time::TimestampMicros},
};

mod alertmanager;
mod aliyun;
mod api;
mod cloudwatch;
mod datadog;
mod email;
mod pagerduty;
mod tencent;
mod zabbix;

/// 一次请求读出来的东西。
#[derive(Debug, Default)]
pub struct Parsed {
    pub alerts: Vec<ExternalAlert>,
    /// 读懂了但没有要处理的告警时，说明为什么（例如 PagerDuty 的测试事件）。
    pub message: Option<String>,
}

impl Parsed {
    fn alerts(alerts: Vec<ExternalAlert>) -> Self {
        Self {
            alerts,
            message: None,
        }
    }

    fn nothing(message: impl Into<String>) -> Self {
        Self {
            alerts: Vec::new(),
            message: Some(message.into()),
        }
    }
}

/// 按来源的类型读请求体。读不懂时返回 `Invalid`，说明哪里不对。
pub fn parse(source: &AlertSource, content_type: Option<&str>, body: &[u8]) -> Result<Parsed> {
    let parsed = match source.kind {
        AlertSourceKind::Api => api::parse(body)?,
        AlertSourceKind::Alertmanager => {
            alertmanager::parse(body, alertmanager::Sender::Alertmanager)?
        }
        AlertSourceKind::Grafana => alertmanager::parse(body, alertmanager::Sender::Grafana)?,
        AlertSourceKind::Datadog => datadog::parse(body)?,
        AlertSourceKind::Pagerduty => pagerduty::parse(body)?,
        AlertSourceKind::AliyunCms => aliyun::parse(content_type, body)?,
        AlertSourceKind::TencentMonitor => tencent::parse(body)?,
        AlertSourceKind::AwsCloudwatch => cloudwatch::parse(body)?,
        AlertSourceKind::Zabbix => zabbix::parse(body)?,
        AlertSourceKind::Email => email::parse(&source.email, body)?,
    };

    Ok(Parsed {
        alerts: parsed.alerts.into_iter().map(ExternalAlert::tidy).collect(),
        message: parsed.message,
    })
}

/// 错误里给人看的那句话，不带 `invalid argument:` 这样的前缀。
fn reason(error: &Error) -> String {
    match error {
        Error::InvalidArgument(message) => message.clone(),
        other => other.to_string(),
    }
}

fn json(body: &[u8]) -> Result<Value> {
    serde_json::from_slice(body)
        .map_err(|error| Error::invalid(format!("the request body is not JSON: {error}")))
}

/// RFC 3339 时间（带时区偏移也行）换成 UTC 微秒。Go 的零值时间（`0001-01-01T00:00:00Z`，
/// 还没结束的告警的 `endsAt`）算没有。
fn rfc3339(value: &str) -> Option<TimestampMicros> {
    let parsed = chrono::DateTime::parse_from_rfc3339(value.trim()).ok()?;
    let micros = parsed.timestamp_micros();

    (micros > 0).then_some(TimestampMicros(micros))
}

/// 秒或毫秒的 Unix 时间（数字或数字字符串）换成微秒：小于 10^11 的按秒算。
fn epoch(value: &Value) -> Option<TimestampMicros> {
    let number = match value {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.trim().parse::<i64>().ok(),
        _ => None,
    }?;

    if number <= 0 {
        return None;
    }

    if number < 100_000_000_000 {
        Some(TimestampMicros::from_secs(number))
    } else {
        Some(TimestampMicros::from_millis(number))
    }
}

/// 字符串或数字的值，去掉首尾空白，空的算没有。
fn text(value: &Value) -> Option<String> {
    let text = match value {
        Value::String(text) => text.trim().to_string(),
        Value::Number(number) => number.to_string(),
        Value::Bool(flag) => flag.to_string(),
        _ => return None,
    };

    (!text.is_empty()).then_some(text)
}

/// `object` 里 `key` 的字符串值。
fn field(object: &Value, key: &str) -> Option<String> {
    object.get(key).and_then(text)
}

/// JSON 对象里的每个值都写成字符串；不是对象就是空的。
fn strings(value: Option<&Value>) -> BTreeMap<String, String> {
    value
        .and_then(Value::as_object)
        .map(|object| {
            object
                .iter()
                .filter_map(|(key, value)| text(value).map(|value| (key.clone(), value)))
                .collect()
        })
        .unwrap_or_default()
}

/// 只认 http 与 https 的地址；去掉首尾空白。
fn web_address(value: &str) -> Option<String> {
    let value = value.trim();
    let url = url::Url::parse(value).ok()?;

    matches!(url.scheme(), "http" | "https").then(|| value.to_string())
}

/// 地址的主机名，小写。
fn host_of(value: &str) -> Option<String> {
    url::Url::parse(value.trim())
        .ok()?
        .host_str()
        .map(|host| host.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn times_with_offsets_seconds_and_millis_become_utc_micros() {
        assert_eq!(
            rfc3339("2021-10-12T09:51:03+02:00"),
            rfc3339("2021-10-12T07:51:03Z")
        );
        assert_eq!(rfc3339("0001-01-01T00:00:00Z"), None);
        assert_eq!(rfc3339("yesterday"), None);

        assert_eq!(
            epoch(&json!("1508136760")),
            Some(TimestampMicros(1_508_136_760_000_000))
        );
        assert_eq!(
            epoch(&json!(1_625_455_812_126_i64)),
            Some(TimestampMicros(1_625_455_812_126_000))
        );
        assert_eq!(epoch(&json!("soon")), None);
    }

    #[test]
    fn only_web_addresses_count_as_links() {
        assert_eq!(
            web_address(" https://app.datadoghq.eu/monitors/1 ").as_deref(),
            Some("https://app.datadoghq.eu/monitors/1")
        );
        assert_eq!(web_address("javascript:alert(1)"), None);
        assert_eq!(
            host_of("https://Acme.EU.pagerduty.com/incidents/P1").as_deref(),
            Some("acme.eu.pagerduty.com")
        );
    }
}
