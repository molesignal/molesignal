// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 腾讯云可观测平台（原云监控）的告警回调：通知模板里的接口回调，指标告警和事件告警是同一个格式。
//!
//! - `alarmStatus`：`1` 告警、`0` 恢复、`5` 无状态（事件告警）。文档说大多是字符串，样例是数字，都认。
//! - 去重键是账号（`uin`）、策略、指标或事件名、级别和告警对象（`dimensions`，不算会改名的 `objName`、
//!   `deviceName`）。不用 `sessionId`：文档没说恢复带的是不是同一个。分级别的策略腾讯云各级各算（`alertId` 是
//!   `sessionId_<级别>`），这里也一样。
//! - 严重度看 `alarmLevel`：`Serious` 紧急、`Warn` 严重、`Remind` 提示。
//! - 时间不带时区，按北京时间。恢复的时间是 `recoverTime`；告警的时间是首次触发加上已持续的秒数，重复告警时
//!   往后走，不会被当成晚到的旧消息。回调可能新增字段，不认得的不管。

use std::collections::BTreeMap;

use chrono::{FixedOffset, NaiveDateTime, TimeZone};
use serde_json::Value;

use super::{Parsed, field, json, strings, web_address};
use crate::{
    domain::alerting::{
        incident::Severity,
        source::{ExternalAction, ExternalAlert},
    },
    shared::{Error, Result, time::TimestampMicros},
};

/// 会改名的维度：不算进去重键。
const RENAMED_DIMENSIONS: [&str; 2] = ["objName", "deviceName"];

const BEIJING_OFFSET_SECS: i32 = 8 * 3600;

pub(super) fn parse(body: &[u8]) -> Result<Parsed> {
    let payload = json(body)?;

    let Some(status) = field(&payload, "alarmStatus") else {
        return Err(Error::invalid(
            "not a Tencent Cloud alarm callback: the body has no `alarmStatus`",
        ));
    };

    let action = match status.as_str() {
        "1" | "5" => ExternalAction::Trigger,
        "0" => ExternalAction::Resolve,
        other => {
            return Ok(Parsed::nothing(format!(
                "Tencent Cloud alarm status {other} is neither 1, 5 nor 0"
            )));
        }
    };

    let object = payload.get("alarmObjInfo").cloned().unwrap_or(Value::Null);
    let policy = payload
        .get("alarmPolicyInfo")
        .cloned()
        .unwrap_or(Value::Null);
    let conditions = policy.get("conditions").cloned().unwrap_or(Value::Null);
    let dimensions = strings(object.get("dimensions"));

    let metric = field(&conditions, "metricName");
    let event = field(&conditions, "eventName");
    let level = field(&payload, "alarmLevel");

    let account = field(&object, "uin")
        .or_else(|| field(&object, "appId"))
        .unwrap_or_default();

    let dedup_key = format!(
        "{account}|{}|{}|{}|{}",
        field(&policy, "policyId").unwrap_or_default(),
        metric.as_deref().or(event.as_deref()).unwrap_or_default(),
        level.as_deref().unwrap_or_default(),
        object_key(&dimensions),
    );

    Ok(Parsed::alerts(vec![ExternalAlert {
        action,
        dedup_key,
        summary: summary(&policy, &conditions, &dimensions),
        severity: level.as_deref().and_then(severity_of),
        labels: labels(&object, &policy, metric, event, dimensions, &account),
        annotations: annotations(&payload, &policy, &conditions),
        occurred_at: occurred_at(&payload, action),
        no_data: false,
        members: Vec::new(),
        priority: None,
        urgency: None,
    }]))
}

fn severity_of(level: &str) -> Option<Severity> {
    match level.to_ascii_lowercase().as_str() {
        "serious" => Some(Severity::Critical),
        "warn" => Some(Severity::Error),
        "remind" => Some(Severity::Info),
        _ => None,
    }
}

/// 告警对象：维度按名字排好；只有会改名的维度时也只好用它们。
fn object_key(dimensions: &BTreeMap<String, String>) -> String {
    let stable: Vec<String> = dimensions
        .iter()
        .filter(|(key, _)| !RENAMED_DIMENSIONS.contains(&key.as_str()))
        .map(|(key, value)| format!("{key}={value}"))
        .collect();

    if !stable.is_empty() {
        return stable.join(",");
    }

    dimensions
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// `CPU 利用率 > 80% · web-1`；事件是事件名加对象。
fn summary(policy: &Value, conditions: &Value, dimensions: &BTreeMap<String, String>) -> String {
    let metric = field(conditions, "metricShowName").or_else(|| field(conditions, "metricName"));

    let what = match metric {
        Some(metric) => match threshold(conditions) {
            Some(threshold) => format!("{metric} {threshold}"),
            None => metric,
        },
        None => field(conditions, "eventShowName")
            .or_else(|| field(conditions, "eventName"))
            .or_else(|| field(policy, "policyName"))
            .unwrap_or_else(|| "Tencent Cloud alarm".into()),
    };

    let object = dimensions
        .get("objName")
        .or_else(|| dimensions.get("unInstanceId"))
        .or_else(|| dimensions.get("objId"));

    match object {
        Some(object) => format!("{what} · {object}"),
        None => what,
    }
}

/// `> 80%`；没有阈值的指标没有。
fn threshold(conditions: &Value) -> Option<String> {
    let comparison = field(conditions, "calcType")?;
    let value = field(conditions, "calcValue")?;
    let unit = field(conditions, "calcUnit").unwrap_or_default();

    Some(format!("{comparison} {value}{unit}"))
}

fn labels(
    object: &Value,
    policy: &Value,
    metric: Option<String>,
    event: Option<String>,
    dimensions: BTreeMap<String, String>,
    account: &str,
) -> BTreeMap<String, String> {
    let mut labels = dimensions;

    // 策略的标签：指标告警是 `key`/`value`，事件告警是 `name`/`value`。
    for tag in policy
        .get("policyTags")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let name = field(tag, "key").or_else(|| field(tag, "name"));

        if let (Some(name), Some(value)) = (name, field(tag, "value")) {
            labels.entry(name).or_insert(value);
        }
    }

    let extra = [
        ("tencent.region", field(object, "region")),
        ("tencent.namespace", field(object, "namespace")),
        ("tencent.metric", metric),
        ("tencent.event", event),
        ("tencent.policy_id", field(policy, "policyId")),
        ("tencent.policy_type", field(policy, "policyType")),
        (
            "tencent.uin",
            Some(account.to_string()).filter(|uin| !uin.is_empty()),
        ),
    ];

    labels.extend(
        extra
            .into_iter()
            .filter_map(|(key, value)| value.map(|value| (key.to_string(), value))),
    );

    labels
}

fn annotations(payload: &Value, policy: &Value, conditions: &Value) -> BTreeMap<String, String> {
    let reading = field(conditions, "currentValue")
        .map(|reading| format!("{reading}{}", field(conditions, "unit").unwrap_or_default()));

    [
        ("policy", field(policy, "policyName")),
        ("value", reading),
        ("threshold", threshold(conditions)),
        ("product", field(conditions, "productShowName")),
        ("session_id", field(payload, "sessionId")),
        (
            "tencent_url",
            field(payload, "policyDetailURL").and_then(|url| web_address(&url)),
        ),
    ]
    .into_iter()
    .filter_map(|(key, value)| value.map(|value| (key.to_string(), value)))
    .collect()
}

fn occurred_at(payload: &Value, action: ExternalAction) -> Option<TimestampMicros> {
    let first = field(payload, "firstOccurTime")
        .as_deref()
        .and_then(beijing);

    let recovered = field(payload, "recoverTime").as_deref().and_then(beijing);

    if action == ExternalAction::Resolve && recovered.is_some() {
        return recovered;
    }

    let lasted = field(payload, "durationTime")
        .and_then(|secs| secs.parse::<i64>().ok())
        .unwrap_or(0)
        .max(0);

    first.map(|first| TimestampMicros(first.0.saturating_add(lasted * 1_000_000)))
}

/// `2024-03-07 18:12:00`，按北京时间；没恢复时的 `"0"` 算没有。
fn beijing(value: &str) -> Option<TimestampMicros> {
    let local = NaiveDateTime::parse_from_str(value.trim(), "%Y-%m-%d %H:%M:%S").ok()?;
    let at = FixedOffset::east_opt(BEIJING_OFFSET_SECS)?
        .from_local_datetime(&local)
        .single()?;

    Some(TimestampMicros(at.timestamp_micros()))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// 腾讯云文档「配置告警回调」的指标告警样例。
    fn metric_alarm() -> Value {
        json!({
            "sessionId": "xxxxxxxx",
            "alertId": "xxxxxxxx_Remind",
            "alarmStatus": 1,
            "alarmType": "metric",
            "alarmLevel": "Remind",
            "alarmObjInfo": {
                "region": "gz",
                "namespace": "qce/cvm",
                "appId": "xxxxxxxxxxxx",
                "uin": "xxxxxxxxxxxx",
                "dimensions": {
                    "deviceName": "web-1",
                    "objId": "xxxxxxxxxxxx",
                    "objName": "10.0.0.8(web-1)",
                    "unInstanceId": "ins-xxxxxxxx"
                }
            },
            "alarmPolicyInfo": {
                "policyId": "policy-n4exeh88",
                "policyType": "cvm_device",
                "policyName": "CPU high",
                "policyTypeCName": "云服务器-基础监控",
                "policyTypeEname": "",
                "conditions": {
                    "metricName": "qemu_vcpu_usage",
                    "metricShowName": "CPU 利用率",
                    "calcType": ">",
                    "calcValue": "80",
                    "currentValue": "96.5",
                    "historyValue": "",
                    "unit": "%",
                    "calcUnit": "%",
                    "period": "60",
                    "periodNum": "1",
                    "alarmNotifyType": "continuousAlarm",
                    "alarmNotifyPeriod": 3600
                },
                "tag": [],
                "policyTags": [{"key": "team", "value": "checkout"}]
            },
            "firstOccurTime": "2024-03-07 18:12:00",
            "durationTime": 0,
            "recoverTime": "0",
            "policyDetailURL": "https://console.cloud.tencent.com/monitor/alarm2/policy/detail/policy-n4exeh88",
            "historyDetailURL": "https://console.cloud.tencent.com/monitor/alarm2/history"
        })
    }

    /// 同一篇文档的事件告警样例，是一条恢复。
    fn event_alarm() -> Value {
        json!({
            "sessionId": "xxxxxxxx",
            "alarmStatus": 0,
            "alarmType": "event",
            "alarmObjInfo": {
                "region": "sh",
                "appId": "xxxxxxxxxxxx",
                "uin": "xxxxxxxxxxxx",
                "dimensions": {"unInstanceId": "ins-xxxxxxxx"}
            },
            "alarmPolicyInfo": {
                "policyId": "policy-xxxxxxxx",
                "policyName": "events",
                "policyType": "cvm_detail",
                "policyTypeCName": "云服务器-基础监控",
                "conditions": {
                    "productName": "cvm",
                    "productShowName": "云服务器",
                    "eventName": "cvm:ErrorEvent:GuestCoreError",
                    "eventShowName": "内核故障",
                    "alarmNotifyType": "singleAlarm",
                    "alarmNotifyPeriod": 0
                },
                "policyTags": [{"name": "env", "value": "prod"}]
            },
            "firstOccurTime": "2023-10-31 15:46:34",
            "durationTime": 500,
            "recoverTime": "2023-10-31 15:50:00"
        })
    }

    fn one(payload: &Value) -> ExternalAlert {
        let parsed = parse(&serde_json::to_vec(payload).unwrap()).unwrap();

        parsed.alerts.into_iter().next().unwrap()
    }

    #[test]
    fn a_metric_alarm_opens_with_its_object_threshold_and_tags() {
        let alert = one(&metric_alarm());

        assert_eq!(alert.action, ExternalAction::Trigger);
        assert_eq!(alert.summary, "CPU 利用率 > 80% · 10.0.0.8(web-1)");
        assert_eq!(alert.severity, Some(Severity::Info));
        assert_eq!(alert.labels["tencent.region"], "gz");
        assert_eq!(alert.labels["tencent.metric"], "qemu_vcpu_usage");
        assert_eq!(alert.labels["unInstanceId"], "ins-xxxxxxxx");
        assert_eq!(alert.labels["team"], "checkout");
        assert_eq!(alert.annotations["value"], "96.5%");
        assert_eq!(alert.annotations["threshold"], "> 80%");
        assert!(alert.annotations["tencent_url"].starts_with("https://console.cloud.tencent.com/"));

        // 北京时间 18:12。
        assert_eq!(
            alert.occurred_at,
            Some(TimestampMicros::from_secs(1_709_806_320))
        );
    }

    #[test]
    fn its_recovery_lands_on_the_same_alert_even_after_a_rename() {
        let mut recovery = metric_alarm();

        recovery["alarmStatus"] = json!("0");
        recovery["sessionId"] = json!("another-session");
        recovery["recoverTime"] = json!("2024-03-07 18:30:00");
        recovery["alarmObjInfo"]["dimensions"]["objName"] = json!("10.0.0.8(renamed)");

        let alert = one(&recovery);

        assert_eq!(alert.action, ExternalAction::Resolve);
        assert_eq!(alert.dedup_key, one(&metric_alarm()).dedup_key);
        assert_eq!(
            alert.occurred_at,
            Some(TimestampMicros::from_secs(1_709_807_400))
        );
    }

    #[test]
    fn a_repeat_moves_on_in_time_and_each_level_is_its_own_alert() {
        let mut repeat = metric_alarm();

        repeat["durationTime"] = json!(600);

        assert_eq!(
            one(&repeat).occurred_at,
            Some(TimestampMicros::from_secs(1_709_806_920))
        );

        let mut serious = metric_alarm();

        serious["alarmLevel"] = json!("Serious");

        assert_eq!(one(&serious).severity, Some(Severity::Critical));
        assert_ne!(one(&serious).dedup_key, one(&metric_alarm()).dedup_key);
    }

    #[test]
    fn an_event_alarm_is_named_by_its_event_and_may_have_no_state() {
        let alert = one(&event_alarm());

        assert_eq!(alert.action, ExternalAction::Resolve);
        assert_eq!(alert.summary, "内核故障 · ins-xxxxxxxx");
        assert_eq!(alert.severity, None);
        assert_eq!(
            alert.labels["tencent.event"],
            "cvm:ErrorEvent:GuestCoreError"
        );
        assert_eq!(alert.labels["env"], "prod");
        assert_eq!(alert.annotations["product"], "云服务器");

        let mut stateless = event_alarm();

        stateless["alarmStatus"] = json!(5);
        stateless["recoverTime"] = json!("0");

        assert_eq!(one(&stateless).action, ExternalAction::Trigger);
    }

    #[test]
    fn what_is_no_alarm_is_refused_and_an_unknown_status_skipped() {
        let error = parse(b"{\"hello\": 1}").unwrap_err().to_string();
        assert!(error.contains("alarmStatus"), "{error}");

        let mut unknown = metric_alarm();
        unknown["alarmStatus"] = json!(9);

        let parsed = parse(&serde_json::to_vec(&unknown).unwrap()).unwrap();
        assert!(parsed.alerts.is_empty());
        assert!(parsed.message.unwrap().contains('9'));
    }
}
