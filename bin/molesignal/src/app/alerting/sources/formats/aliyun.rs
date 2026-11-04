// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 阿里云云监控：阈值报警回调（表单编码，也认 JSON）与事件订阅的推送渠道（JSON）。
//!
//! - 阈值报警回调：`alertState` 为 ALERT、INSUFFICIENT_DATA（无数据）时触发，为 OK 时解决；严重度
//!   取 `triggerLevel`（CRITICAL、WARN、INFO）。去重键是账号（`userId`）、规则（`ruleId`）与对象
//!   （`dimensions`）。`timestamp` 有的是秒、有的是毫秒。
//! - 事件订阅：`alert.alertStatus` 为 TRIGGERED 时触发，为 RESOLVED 时解决；去重键是账号加
//!   `alert.dedupId`。事件时间形如 `20210705T113013.398+0800`，带时区偏移。
//! - 地域写进标签 `aliyun.region`；回调可能新增字段，不认得的字段不管。

use std::collections::BTreeMap;

use serde_json::Value;

use super::{Parsed, epoch, field, json, strings};
use crate::{
    domain::alerting::source::{ExternalAction, ExternalAlert, severity_from_word},
    shared::{Error, Result, time::TimestampMicros},
};

pub(super) fn parse(content_type: Option<&str>, body: &[u8]) -> Result<Parsed> {
    let first = body.iter().find(|byte| !byte.is_ascii_whitespace());
    let is_json = content_type.is_some_and(|value| value.contains("json")) || first == Some(&b'{');

    if is_json {
        let payload = json(body)?;

        return match payload.get("alert") {
            Some(alert) if alert.is_object() => event(&payload, alert),
            _ => threshold(&strings(Some(&payload))),
        };
    }

    let fields: BTreeMap<String, String> = url::form_urlencoded::parse(body)
        .map(|(key, value)| (key.into_owned(), value.trim().to_string()))
        .collect();

    // 事件订阅选了表单：顶层各字段一个参数，`alert` 是 JSON 串。
    if let Some(alert) = fields.get("alert") {
        let alert: Value = serde_json::from_str(alert)
            .map_err(|error| Error::invalid(format!("the `alert` field is not JSON: {error}")))?;

        let payload = Value::Object(
            fields
                .iter()
                .map(|(key, value)| (key.clone(), Value::String(value.clone())))
                .collect(),
        );

        return event(&payload, &alert);
    }

    threshold(&fields)
}

/// 阈值报警回调。
fn threshold(fields: &BTreeMap<String, String>) -> Result<Parsed> {
    let value = |key: &str| {
        fields
            .get(key)
            .map(|value| value.trim())
            .filter(|value| !value.is_empty() && *value != "null")
            .map(str::to_string)
    };

    let Some(state) = value("alertState") else {
        return Err(Error::invalid(
            "not a CloudMonitor callback: the body has no `alertState`",
        ));
    };

    let (action, no_data) = match state.to_ascii_uppercase().as_str() {
        "OK" => (ExternalAction::Resolve, false),
        "ALERT" => (ExternalAction::Trigger, false),
        "INSUFFICIENT_DATA" => (ExternalAction::Trigger, true),
        other => {
            return Err(Error::invalid(format!(
                "alertState `{other}` must be OK, ALERT or INSUFFICIENT_DATA"
            )));
        }
    };

    let dimensions = dimensions(value("dimensions").as_deref().unwrap_or_default());
    let user = value("userId").unwrap_or_default();
    let rule = value("ruleId")
        .or_else(|| value("alertName"))
        .unwrap_or_default();
    let name = value("alertName").unwrap_or_else(|| "CloudMonitor alert".into());
    let instance = value("instanceName");

    let summary = match &instance {
        Some(instance) => format!("{name} · {instance}"),
        None => name,
    };

    let objects: Vec<String> = dimensions
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect();

    let mut labels = dimensions;

    let extra = [
        ("aliyun.region", value("regionId")),
        ("aliyun.namespace", value("namespace")),
        ("aliyun.metric", value("metricName")),
        ("aliyun.rule_id", value("ruleId")),
        ("aliyun.user_id", value("userId")),
        ("aliyun.instance", instance),
    ];

    labels.extend(
        extra
            .into_iter()
            .filter_map(|(key, value)| value.map(|value| (key.to_string(), value))),
    );

    let reading = value("curValue").map(|reading| match value("unit") {
        Some(unit) => format!("{reading} {unit}"),
        None => reading,
    });

    let annotations = [
        ("expression", value("expression")),
        ("value", reading),
        ("last_time", value("lastTime")),
        ("region_name", value("regionName")),
        ("group", value("productGroupName")),
        ("trans_id", value("transId")),
    ]
    .into_iter()
    .filter_map(|(key, value)| value.map(|value| (key.to_string(), value)))
    .collect();

    Ok(Parsed::alerts(vec![ExternalAlert {
        action,
        dedup_key: format!("{user}|{rule}|{}", objects.join(",")),
        summary,
        severity: value("triggerLevel").and_then(|level| severity_from_word(&level)),
        labels,
        annotations,
        occurred_at: value("timestamp").and_then(|at| epoch(&Value::String(at))),
        no_data,
        members: Vec::new(),
        priority: None,
        urgency: None,
    }]))
}

/// 事件订阅推送的一条事件。
fn event(payload: &Value, alert: &Value) -> Result<Parsed> {
    let action = match field(alert, "alertStatus").map(|status| status.to_ascii_uppercase()) {
        Some(status) if status == "TRIGGERED" => ExternalAction::Trigger,
        Some(status) if status == "RESOLVED" => ExternalAction::Resolve,
        Some(other) => {
            return Ok(Parsed::nothing(format!(
                "CloudMonitor event status {other} is neither TRIGGERED nor RESOLVED"
            )));
        }
        None => {
            return Err(Error::invalid(
                "the CloudMonitor event has no `alertStatus`",
            ));
        }
    };

    let meta = alert
        .get("meta")
        .and_then(|meta| meta.get("sysEventMeta"))
        .cloned()
        .unwrap_or(Value::Null);

    let user = field(alert, "userId")
        .or_else(|| field(payload, "userId"))
        .unwrap_or_default();

    let event_name = field(alert, "eventName").or_else(|| field(&meta, "name"));
    let resource = field(&meta, "resourceId");
    let instance = field(&meta, "instanceName");

    let dedup = field(alert, "dedupId").unwrap_or_else(|| {
        format!(
            "{}|{}",
            event_name.clone().unwrap_or_default(),
            resource.clone().unwrap_or_default()
        )
    });

    let name = field(&meta, "eventNameZh")
        .or_else(|| field(&meta, "eventNameEn"))
        .or_else(|| event_name.clone())
        .unwrap_or_else(|| "CloudMonitor event".into());

    let summary = match &instance {
        Some(instance) => format!("{name} · {instance}"),
        None => name,
    };

    let labels = [
        ("aliyun.region", field(&meta, "regionId")),
        (
            "aliyun.product",
            field(alert, "product").or_else(|| field(&meta, "product")),
        ),
        ("aliyun.event", event_name),
        ("aliyun.resource_id", resource),
        (
            "aliyun.user_id",
            Some(user.clone()).filter(|user| !user.is_empty()),
        ),
        ("aliyun.instance", instance),
    ]
    .into_iter()
    .filter_map(|(key, value)| value.map(|value| (key.to_string(), value)))
    .collect();

    let annotations = [
        ("subscription", field(payload, "strategyName")),
        ("event_type", field(alert, "eventType")),
        ("event_status", field(&meta, "status")),
        ("event_time", field(&meta, "eventTime")),
    ]
    .into_iter()
    .filter_map(|(key, value)| value.map(|value| (key.to_string(), value)))
    .collect();

    let severity = field(alert, "severity")
        .or_else(|| field(payload, "severity"))
        .and_then(|level| severity_from_word(&level));

    let occurred_at = alert
        .get("timestamp")
        .and_then(epoch)
        .or_else(|| field(&meta, "eventTime").as_deref().and_then(event_time))
        .or_else(|| payload.get("time").and_then(epoch));

    Ok(Parsed::alerts(vec![ExternalAlert {
        action,
        dedup_key: format!("{user}|{dedup}"),
        summary,
        severity,
        labels,
        annotations,
        occurred_at,
        no_data: false,
        members: Vec::new(),
        priority: None,
        urgency: None,
    }]))
}

/// `{instanceId=i-bp1, userId=123}` 这样的对象写成标签。
fn dimensions(value: &str) -> BTreeMap<String, String> {
    value
        .trim()
        .trim_start_matches('{')
        .trim_end_matches('}')
        .split(',')
        .filter_map(|pair| pair.split_once('='))
        .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
        .filter(|(key, _)| !key.is_empty())
        .collect()
}

/// 事件时间 `20210705T113013.398+0800` 换成 UTC 微秒。
fn event_time(value: &str) -> Option<TimestampMicros> {
    chrono::DateTime::parse_from_str(value.trim(), "%Y%m%dT%H%M%S%.f%z")
        .ok()
        .map(|at| TimestampMicros(at.timestamp_micros()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::alerting::incident::Severity;

    /// 帮助中心《使用阈值的报警回调》里的表单样例。
    const FORM: &str = "alertName=CPU%E4%BD%BF%E7%94%A8%E7%8E%87&alertState=ALERT&curValue=97.39\
        &dimensions=%7BuserId%3D12****%2C+instanceId%3Di-12****%7D&expression=%24Average%3E%3D90\
        &instanceName=instance-name-****&lastTime=74498+%E7%A7%92&metricName=CPUUtilization\
        &metricProject=acs_ecs_dashboard&namespace=acs_ecs_dashboard&preTriggerLevel=WARN\
        &regionId=cn-hangzhou&regionName=%E5%8D%8E%E4%B8%9C1&ruleId=applyTemplate344cfd42\
        &signature=eEq1zHuCUp0XSmLD8p8VtTKF****&timestamp=1508136760\
        &transId=0e6c4c66-8a1e-4a5c-9d8c-d2a4a3d0****&triggerLevel=WARN&unit=%25&userId=12****";

    fn one(content_type: Option<&str>, body: &str) -> ExternalAlert {
        parse(content_type, body.as_bytes())
            .unwrap()
            .alerts
            .remove(0)
    }

    #[test]
    fn a_form_threshold_callback_opens_one_alert_per_object() {
        let alert = one(
            Some("application/x-www-form-urlencoded; charset=UTF-8"),
            FORM,
        );

        assert_eq!(alert.action, ExternalAction::Trigger);
        assert_eq!(alert.summary, "CPU使用率 · instance-name-****");
        assert_eq!(
            alert.dedup_key,
            "12****|applyTemplate344cfd42|instanceId=i-12****,userId=12****"
        );
        assert_eq!(alert.severity, Some(Severity::Warning));
        assert_eq!(alert.labels["instanceId"], "i-12****");
        assert_eq!(alert.labels["aliyun.region"], "cn-hangzhou");
        assert_eq!(alert.annotations["value"], "97.39 %");
        assert_eq!(alert.annotations["expression"], "$Average>=90");
        // 这里的 timestamp 是秒。
        assert_eq!(
            alert.occurred_at,
            Some(TimestampMicros(1_508_136_760_000_000))
        );
    }

    #[test]
    fn ok_resolves_and_missing_data_is_marked() {
        let ok = FORM.replace("alertState=ALERT", "alertState=OK");
        assert_eq!(one(None, &ok).action, ExternalAction::Resolve);

        let missing = FORM.replace("alertState=ALERT", "alertState=INSUFFICIENT_DATA");
        assert!(one(None, &missing).no_data);
    }

    #[test]
    fn a_json_threshold_callback_reads_the_same_way() {
        let body = r#"{"alertName":"连接数","alertState":"ALERT","curValue":"4.5",
            "dimensions":"{instanceId=i-bp1d7111111115htda, state=TCP_TOTAL, userId=11596111111355}",
            "expression":"$Average>=1","instanceName":"launch-advisor-20210607",
            "metricName":"Host.tcpconnection","namespace":"acs_ecs","productGroupName":"null",
            "regionId":"cn-hangzhou","ruleId":"i-bp11111111115111_111111","timestamp":"1625455812126",
            "triggerLevel":"CRITICAL","userId":"11596111111355"}"#;

        let alert = one(Some("application/json"), body);

        assert_eq!(alert.severity, Some(Severity::Critical));
        assert_eq!(alert.labels["state"], "TCP_TOTAL");
        assert!(!alert.annotations.contains_key("group"));
        assert_eq!(
            alert.occurred_at,
            Some(TimestampMicros(1_625_455_812_126_000))
        );
    }

    #[test]
    fn an_event_subscription_push_is_keyed_by_its_dedup_id() {
        let body = r#"{"batchId":"b-1","severity":"WARN","strategyName":"ecs-events","userId":"115111",
            "alert":{"alertStatus":"TRIGGERED","dedupId":"dedup-1","eventName":"Instance:StateChange",
              "product":"ECS","severity":"WARN","userId":"115111",
              "meta":{"sysEventMeta":{"regionId":"cn-hangzhou","resourceId":"acs:ecs:cn-hangzhou:115111:instance/i-bp1",
                "instanceName":"launch-advisor","eventNameZh":"实例状态变化","eventTime":"20210705T113013.398+0800",
                "status":"Normal"}}}}"#;

        let alert = one(None, body);

        assert_eq!(alert.action, ExternalAction::Trigger);
        assert_eq!(alert.dedup_key, "115111|dedup-1");
        assert_eq!(alert.summary, "实例状态变化 · launch-advisor");
        assert_eq!(alert.labels["aliyun.product"], "ECS");
        // +0800 的事件时间换成 UTC：2021-07-05T03:30:13.398Z。
        assert_eq!(
            alert.occurred_at,
            Some(TimestampMicros(1_625_455_813_398_000))
        );

        let resolved = body.replace("TRIGGERED", "RESOLVED");
        assert_eq!(one(None, &resolved).action, ExternalAction::Resolve);
    }

    #[test]
    fn a_body_without_a_state_is_refused() {
        assert!(parse(None, b"foo=bar").is_err());
        assert!(parse(Some("application/json"), br#"{"alertState":"MAYBE"}"#).is_err());
    }
}
