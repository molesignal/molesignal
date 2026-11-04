// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Zabbix 的 Webhook 媒介类型。Zabbix 不定请求体，由媒介类型的脚本发，文档给出这个媒介类型：字段名是
//! 我们定的，值是 Zabbix 的宏。
//!
//! - 一个问题一个事件：去重键是 Zabbix 的地址（`{$ZABBIX.URL}`，一个来源接几台 Zabbix 时不撞）和问题的
//!   事件 id。恢复的 `{EVENT.ID}` 还是问题的那个。
//! - `{EVENT.VALUE}` 是 `0` 算恢复；`{EVENT.UPDATE.STATUS}` 是 `1` 是有人更新了问题：关闭了算恢复，确认了算
//!   确认，其余（改严重度、写评论）算触发，严重度变了事件跟着变。
//! - 严重度看 `{EVENT.NSEVERITY}`：`0` 未分类用来源的默认严重度。
//! - 没展开的宏（`{EVENT.UPDATE.ACTION}` 这样原样发来的）算没有。

use std::collections::BTreeMap;

use serde_json::Value;

use super::{Parsed, epoch, host_of, json, text, web_address};
use crate::{
    domain::alerting::{
        incident::Severity,
        source::{ExternalAction, ExternalAlert, severity_from_word},
    },
    shared::{Error, Result},
};

pub(super) fn parse(body: &[u8]) -> Result<Parsed> {
    let payload = json(body)?;

    if !payload.is_object() {
        return Err(Error::invalid("the Zabbix payload must be a JSON object"));
    }

    let value = |key: &str| payload.get(key).and_then(text).and_then(expanded);

    let (Some(event_id), Some(event_value)) = (value("event_id"), value("event_value")) else {
        return Err(Error::invalid(
            "the Zabbix payload has no `event_id` or `event_value`: use the media type given in \
             the MoleSignal docs, and test it with real values for its macros",
        ));
    };

    let updated = value("update_status").as_deref() == Some("1");
    let update_action = value("update_action").unwrap_or_default().to_lowercase();
    let acknowledged = value("ack_status").is_some_and(|ack| ack.eq_ignore_ascii_case("yes"));

    let action = if event_value == "0" || (updated && update_action.contains("closed")) {
        ExternalAction::Resolve
    } else if updated && acknowledged && update_action.contains("acknowledged") {
        ExternalAction::Acknowledge
    } else {
        ExternalAction::Trigger
    };

    let address = value("zabbix_url").and_then(|url| web_address(&url));
    let instance = address.as_deref().and_then(host_of).unwrap_or_default();

    let mut labels = tags(payload.get("tags"));

    if let Some(host) = value("host_name") {
        labels.entry("host".into()).or_insert(host);
    }

    if let Some(ip) = value("host_ip").filter(|ip| ip != "UNKNOWN") {
        labels.entry("host.ip".into()).or_insert(ip);
    }

    labels.insert("zabbix.event_id".into(), event_id.clone());

    if let Some(trigger) = value("trigger_id") {
        labels.insert("zabbix.trigger_id".into(), trigger);
    }

    if !instance.is_empty() {
        labels.insert("zabbix.instance".into(), instance.clone());
    }

    let mut annotations = BTreeMap::new();

    if let Some(message) = value("message") {
        annotations.insert("message".to_string(), message);
    }

    if let Some(data) = value("operational_data") {
        annotations.insert("operational_data".to_string(), data);
    }

    if let (Some(address), Some(trigger)) = (&address, value("trigger_id")) {
        annotations.insert(
            "zabbix_url".to_string(),
            format!(
                "{}/tr_events.php?triggerid={trigger}&eventid={event_id}",
                address.trim_end_matches('/')
            ),
        );
    }

    // 恢复和更新有自己的时间：比问题晚，不会被当成晚到的旧消息。
    let occurred_at = [
        (event_value == "0").then_some("recovery_time"),
        updated.then_some("update_time"),
        Some("event_time"),
    ]
    .into_iter()
    .flatten()
    .find_map(|key| {
        payload
            .get(key)
            .filter(|time| !unexpanded(time))
            .and_then(epoch)
    });

    let summary = value("event_name")
        .or_else(|| value("subject"))
        .unwrap_or_else(|| format!("Zabbix problem {event_id}"));

    Ok(Parsed::alerts(vec![ExternalAlert {
        action,
        dedup_key: format!("{instance}|{event_id}"),
        summary,
        severity: value("severity").as_deref().and_then(severity_of),
        labels,
        annotations,
        occurred_at,
        no_data: false,
        members: Vec::new(),
        priority: None,
        urgency: None,
    }]))
}

/// `{EVENT.NSEVERITY}`：0 未分类、1 信息、2 警告、3 一般严重、4 严重、5 灾难。
fn severity_of(number: &str) -> Option<Severity> {
    let word = match number {
        "1" => "information",
        "2" => "warning",
        "3" => "average",
        "4" => "high",
        "5" => "disaster",
        _ => return None,
    };

    severity_from_word(word)
}

/// 没展开的宏算没有。
fn expanded(value: String) -> Option<String> {
    (!is_macro(&value)).then_some(value)
}

fn unexpanded(value: &Value) -> bool {
    value.as_str().is_some_and(is_macro)
}

/// `{EVENT.ID}`、`{$ZABBIX.URL}`、`{#LLD}` 这样的宏。
fn is_macro(value: &str) -> bool {
    value.trim().strip_prefix('{').is_some_and(|rest| {
        rest.strip_suffix('}').is_some_and(|name| {
            !name.is_empty()
                && name.chars().all(|c| {
                    c.is_ascii_uppercase()
                        || c.is_ascii_digit()
                        || matches!(c, '.' | '_' | '$' | '#')
                })
        })
    })
}

/// `{EVENT.TAGSJSON}`：`[{"tag":"service","value":"checkout"}]`，脚本可能原样当字符串发。同名的标签值用逗号
/// 连起来；没有值的标签值为空。
fn tags(value: Option<&Value>) -> BTreeMap<String, String> {
    let parsed = match value {
        Some(Value::String(raw)) => serde_json::from_str::<Value>(raw).unwrap_or(Value::Null),
        Some(other) => other.clone(),
        None => Value::Null,
    };

    let mut labels: BTreeMap<String, String> = BTreeMap::new();

    for tag in parsed.as_array().into_iter().flatten() {
        let Some(name) = tag.get("tag").and_then(text) else {
            continue;
        };

        let tag_value = tag.get("value").and_then(text).unwrap_or_default();

        labels
            .entry(name)
            .and_modify(|current| {
                if !current.split(',').any(|part| part == tag_value) {
                    current.push(',');
                    current.push_str(&tag_value);
                }
            })
            .or_insert(tag_value);
    }

    labels
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::shared::time::TimestampMicros;

    fn payload(overrides: &[(&str, &str)]) -> Vec<u8> {
        let mut payload = json!({
            "event_id": "3148",
            "event_value": "1",
            "event_name": "High CPU utilization on web-1",
            "event_time": "1761552000",
            "recovery_id": "{EVENT.RECOVERY.ID}",
            "recovery_time": "{EVENT.RECOVERY.TIMESTAMP}",
            "update_status": "0",
            "update_action": "{EVENT.UPDATE.ACTION}",
            "update_time": "{EVENT.UPDATE.TIMESTAMP}",
            "ack_status": "No",
            "severity": "4",
            "host_name": "web-1",
            "host_ip": "10.0.0.5",
            "trigger_id": "13491",
            "operational_data": "Current utilization: 97 %",
            "message": "Problem started at 10:00:00",
            "tags": "[{\"tag\":\"service\",\"value\":\"checkout\"},{\"tag\":\"role\",\"value\":\"web\"},{\"tag\":\"role\",\"value\":\"api\"},{\"tag\":\"canary\",\"value\":\"\"}]",
            "zabbix_url": "https://zabbix.example.com/",
        });

        for (key, value) in overrides {
            payload[*key] = Value::String((*value).to_string());
        }

        serde_json::to_vec(&payload).unwrap()
    }

    fn one(overrides: &[(&str, &str)]) -> ExternalAlert {
        parse(&payload(overrides)).unwrap().alerts.remove(0)
    }

    #[test]
    fn a_problem_opens_one_alert_keyed_by_the_server_and_its_event() {
        let alert = one(&[]);

        assert_eq!(alert.action, ExternalAction::Trigger);
        assert_eq!(alert.dedup_key, "zabbix.example.com|3148");
        assert_eq!(alert.summary, "High CPU utilization on web-1");
        assert_eq!(alert.severity, Some(Severity::Error));
        assert_eq!(alert.labels["host"], "web-1");
        assert_eq!(alert.labels["host.ip"], "10.0.0.5");
        assert_eq!(alert.labels["service"], "checkout");
        assert_eq!(alert.labels["role"], "web,api");
        assert_eq!(alert.labels["canary"], "");
        assert_eq!(alert.labels["zabbix.instance"], "zabbix.example.com");
        assert_eq!(
            alert.annotations["operational_data"],
            "Current utilization: 97 %"
        );
        assert_eq!(
            alert.annotations["zabbix_url"],
            "https://zabbix.example.com/tr_events.php?triggerid=13491&eventid=3148"
        );
        assert_eq!(
            alert.occurred_at,
            Some(TimestampMicros(1_761_552_000_000_000))
        );
    }

    #[test]
    fn a_recovery_resolves_the_same_alert_at_its_own_time() {
        let alert = one(&[
            ("event_value", "0"),
            ("recovery_id", "3190"),
            ("recovery_time", "1761552600"),
        ]);

        assert_eq!(alert.action, ExternalAction::Resolve);
        assert_eq!(alert.dedup_key, "zabbix.example.com|3148");
        assert_eq!(
            alert.occurred_at,
            Some(TimestampMicros(1_761_552_600_000_000))
        );
    }

    #[test]
    fn updates_acknowledge_close_or_change_the_problem() {
        let acknowledged = one(&[
            ("update_status", "1"),
            ("update_action", "acknowledged, commented"),
            ("ack_status", "Yes"),
            ("update_time", "1761552300"),
        ]);
        assert_eq!(acknowledged.action, ExternalAction::Acknowledge);
        assert_eq!(
            acknowledged.occurred_at,
            Some(TimestampMicros(1_761_552_300_000_000))
        );

        let closed = one(&[("update_status", "1"), ("update_action", "closed")]);
        assert_eq!(closed.action, ExternalAction::Resolve);

        // 已确认的问题上写一条评论：不是又确认一次。
        let commented = one(&[
            ("update_status", "1"),
            ("update_action", "commented"),
            ("ack_status", "Yes"),
        ]);
        assert_eq!(commented.action, ExternalAction::Trigger);

        let raised = one(&[
            ("update_status", "1"),
            ("update_action", "changed severity from High to Disaster"),
            ("severity", "5"),
        ]);
        assert_eq!(raised.action, ExternalAction::Trigger);
        assert_eq!(raised.severity, Some(Severity::Critical));
    }

    #[test]
    fn severities_follow_the_zabbix_scale() {
        assert_eq!(one(&[("severity", "0")]).severity, None);
        assert_eq!(one(&[("severity", "1")]).severity, Some(Severity::Info));
        assert_eq!(one(&[("severity", "2")]).severity, Some(Severity::Warning));
        assert_eq!(one(&[("severity", "3")]).severity, Some(Severity::Warning));
        assert_eq!(one(&[("severity", "5")]).severity, Some(Severity::Critical));
    }

    #[test]
    fn a_test_from_the_zabbix_page_without_real_values_is_refused_and_says_why() {
        let error = parse(&payload(&[("event_id", "{EVENT.ID}")]))
            .unwrap_err()
            .to_string();

        assert!(error.contains("MoleSignal docs"), "{error}");
        assert!(parse(br#"["not", "an", "object"]"#).is_err());
    }

    #[test]
    fn a_server_without_a_known_address_still_opens_alerts() {
        let alert = one(&[("zabbix_url", "{$ZABBIX.URL}"), ("host_ip", "UNKNOWN")]);

        assert_eq!(alert.dedup_key, "|3148");
        assert!(!alert.labels.contains_key("zabbix.instance"));
        assert!(!alert.labels.contains_key("host.ip"));
        assert!(!alert.annotations.contains_key("zabbix_url"));
    }
}
