// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Huawei Cloud Eye (CES) alarms: an alarm rule notifies an SMN topic, and the topic's HTTP(S)
//! subscription pushes the message here. The SMN signature and the subscription confirmation are
//! handled at the intake address first; this only reads the content.
//!
//! - The body is the SMN message (<https://support.huaweicloud.com/usermanual-smn/smn_ug_a9002.html>);
//!   the alarm is the JSON string in `message`.
//! - `alarm_status`: `alarm` triggers, `insufficient_data` triggers as no data, `ok` resolves.
//!   Without it, `template_variable.IsAlarm` says which.
//! - One incident per alarm rule and resource: the dedup key is the project of the topic,
//!   `alarm_id` and the resource (`dimension`, else `template_variable.ResourceId`).
//! - The level: `alarm_level` 1 to 4 when sent, else the words of `template_variable.AlarmLevel`
//!   (Critical/紧急, Major/重要, Minor/次要, Informational/提示); critical, error, warning, info.
//! - `time` is Unix milliseconds.

use std::collections::BTreeMap;

use domain::alerting::{
    incident::Severity,
    source::{ExternalAction, ExternalAlert},
};
use kernel::{Error, Result};
use serde_json::Value;

use super::{Parsed, epoch, field, json, rfc3339, web_address};

pub(super) fn parse(body: &[u8]) -> Result<Parsed> {
    let notice = json(body)?;

    match field(&notice, "type").as_deref() {
        Some("Notification") => notification(&notice),
        Some("SubscriptionConfirmation") => {
            Ok(Parsed::nothing("the SMN subscription is confirmed"))
        }
        Some("UnsubscribeConfirmation") => Ok(Parsed::nothing("the SMN subscription was removed")),
        _ => Err(Error::invalid(
            "not an SMN message: the body has no `type`. Subscribe the intake address to the \
             SMN topic with the HTTPS protocol",
        )),
    }
}

fn notification(notice: &Value) -> Result<Parsed> {
    let not_an_alarm = || Parsed::nothing("the SMN message is not a Cloud Eye alarm");

    let Some(alarm) = notice
        .get("message")
        .and_then(Value::as_str)
        .and_then(|message| serde_json::from_str::<Value>(message).ok())
    else {
        return Ok(not_an_alarm());
    };

    let Some(alarm_id) = field(&alarm, "alarm_id") else {
        return Ok(not_an_alarm());
    };

    let template = alarm
        .get("template_variable")
        .cloned()
        .unwrap_or(Value::Null);

    let state = field(&alarm, "alarm_status").or_else(|| {
        template
            .get("IsAlarm")
            .and_then(Value::as_bool)
            .map(|firing| if firing { "alarm" } else { "ok" }.to_string())
    });

    let (action, no_data) = match state.as_deref().map(str::to_ascii_lowercase).as_deref() {
        Some("alarm") => (ExternalAction::Trigger, false),
        Some("insufficient_data") => (ExternalAction::Trigger, true),
        Some("ok") => (ExternalAction::Resolve, false),
        other => {
            return Ok(Parsed::nothing(format!(
                "Cloud Eye alarm status {} is neither alarm, insufficient_data nor ok",
                other.unwrap_or("(none)")
            )));
        }
    };

    let topic = TopicUrn::of(&field(notice, "topic_urn").unwrap_or_default());

    let resource = field(&alarm, "dimension")
        .or_else(|| field(&template, "ResourceId"))
        .unwrap_or_default();

    let dedup_key = format!(
        "{}|{alarm_id}|{resource}",
        topic
            .as_ref()
            .map(|topic| topic.project.as_str())
            .unwrap_or_default()
    );

    let occurred_at = alarm
        .get("time")
        .and_then(epoch)
        .or_else(|| field(notice, "timestamp").as_deref().and_then(rfc3339));

    Ok(Parsed::alerts(vec![ExternalAlert {
        action,
        dedup_key,
        summary: summary(&alarm, &template),
        severity: severity_of(&alarm, &template),
        labels: labels(&alarm, &template, topic.as_ref()),
        annotations: annotations(&alarm, &template),
        occurred_at,
        no_data,
        members: Vec::new(),
        priority: None,
        urgency: None,
    }]))
}

/// `urn:smn:<region>:<project id>:<topic name>`.
struct TopicUrn {
    region: String,
    project: String,
}

impl TopicUrn {
    fn of(urn: &str) -> Option<Self> {
        let mut parts = urn.split(':');

        if parts.next() != Some("urn") || parts.next() != Some("smn") {
            return None;
        }

        let region = parts.next()?;
        let project = parts.next()?;

        Some(Self {
            region: region.to_string(),
            project: project.to_string(),
        })
    }
}

fn severity_of(alarm: &Value, template: &Value) -> Option<Severity> {
    let level = field(alarm, "alarm_level").or_else(|| field(template, "AlarmLevel"))?;

    match level.trim().to_ascii_lowercase().as_str() {
        "1" | "critical" | "紧急" => Some(Severity::Critical),
        "2" | "major" | "重要" => Some(Severity::Error),
        "3" | "minor" | "次要" => Some(Severity::Warning),
        "4" | "informational" | "info" | "提示" => Some(Severity::Info),
        _ => None,
    }
}

/// `rule · resource`: the rule's name and the resource's name or ID.
fn summary(alarm: &Value, template: &Value) -> String {
    let rule = field(alarm, "alarm_name")
        .or_else(|| field(template, "AlarmRuleName"))
        .unwrap_or_else(|| "Cloud Eye alarm".into());

    let resource = field(template, "ResourceName").or_else(|| field(template, "ResourceId"));

    match resource {
        Some(resource) => format!("{rule} · {resource}"),
        None => rule,
    }
}

fn labels(alarm: &Value, template: &Value, topic: Option<&TopicUrn>) -> BTreeMap<String, String> {
    let mut labels = BTreeMap::new();

    // `instance_id:6b42c3f4-…`, several joined by commas.
    for pair in field(alarm, "dimension").unwrap_or_default().split(',') {
        if let Some((name, value)) = pair.split_once(':') {
            labels.insert(name.trim().to_string(), value.trim().to_string());
        }
    }

    let extra = [
        (
            "huawei.region",
            field(template, "RegionId").or_else(|| topic.map(|topic| topic.region.clone())),
        ),
        (
            "huawei.project_id",
            topic.map(|topic| topic.project.clone()),
        ),
        ("huawei.namespace", field(alarm, "namespace")),
        ("huawei.metric", field(alarm, "metric_name")),
        ("huawei.alarm_id", field(alarm, "alarm_id")),
        ("huawei.resource_id", field(template, "ResourceId")),
    ];

    labels.extend(
        extra
            .into_iter()
            .filter_map(|(key, value)| value.map(|value| (key.to_string(), value))),
    );

    labels
}

fn annotations(alarm: &Value, template: &Value) -> BTreeMap<String, String> {
    let threshold = field(alarm, "comparison_operator")
        .zip(field(alarm, "value"))
        .map(|(comparison, value)| {
            format!(
                "{comparison} {value}{}",
                field(alarm, "unit").unwrap_or_default()
            )
        });

    [
        (
            "message",
            field(alarm, "default_content").or_else(|| field(alarm, "sms_content")),
        ),
        ("value", field(template, "CurrentData")),
        ("threshold", threshold),
        ("region_name", field(template, "Region")),
        ("alarm_record_id", field(alarm, "AlarmRecordID")),
        (
            "huawei_url",
            field(template, "Link").and_then(|url| web_address(&url)),
        ),
    ]
    .into_iter()
    .filter_map(|(key, value)| value.map(|value| (key.to_string(), value)))
    .collect()
}

#[cfg(test)]
mod tests {
    use kernel::time::TimestampMicros;
    use serde_json::json;

    use super::*;

    /// A metric alarm as in the Cloud Eye docs' HTTP(S) notification example.
    fn alarm(status: &str) -> Value {
        json!({
            "message_type": "alarm",
            "alarm_id": "al260512T094501SwVt70001",
            "alarm_name": "alarm-cpu",
            "alarm_status": status,
            "time": 1778550457194_i64,
            "namespace": "SYS.ECS",
            "metric_name": "cpu_util",
            "dimension": "instance_id:6b42c3f4-0000-0000-0000-000000000001",
            "period": 1,
            "filter": "average",
            "comparison_operator": ">=",
            "value": 10,
            "unit": "%",
            "count": 1,
            "alarmValue": [{"time": 1778550457000_i64, "value": 10.04}],
            "sms_content": "[Huawei Cloud] alarm-cpu: CPU usage >= 10%",
            "template_variable": {
                "AlarmNotificationType": "指标",
                "Namespace": "弹性云服务器",
                "ResourceName": "web-1",
                "MetricName": "CPU使用率",
                "IsAlarm": status == "alarm",
                "AlarmLevel": "重要",
                "Region": "华北-北京四",
                "RegionId": "cn-north-4",
                "ResourceId": "6b42c3f4-0000-0000-0000-000000000001",
                "CurrentData": "10.04 %",
                "AlarmRuleName": "alarm-cpu",
                "AlarmId": "al260512T094501SwVt70001",
                "Link": "https://console.huaweicloud.com/ces/?region=cn-north-4#/alarm/histories"
            },
            "AlarmRecordID": "ah260512T094737SSSej0001"
        })
    }

    fn envelope(message: String) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "type": "Notification",
            "signature_version": "v1",
            "signature": "EXAMPLE",
            "subject": "SMN",
            "topic_urn": "urn:smn:cn-north-4:0a419ac94f80f2c62f14c01e34000000:ces_alarms",
            "message_id": "d84bd6629ef04513ad2e37bffd6e17cb",
            "message": message,
            "unsubscribe_url": "https://console.huaweicloud.com/smn/subscription/unsubscribe",
            "signing_cert_url": "https://smn.cn-north-4.myhuaweicloud.com/smn/SMN_cn-north-4_0000.pem",
            "timestamp": "2026-05-12T01:47:37Z"
        }))
        .unwrap()
    }

    fn one(alarm: &Value) -> ExternalAlert {
        parse(&envelope(alarm.to_string()))
            .unwrap()
            .alerts
            .into_iter()
            .next()
            .unwrap()
    }

    #[test]
    fn an_alarm_opens_with_its_resource_and_level() {
        let alert = one(&alarm("alarm"));

        assert_eq!(alert.action, ExternalAction::Trigger);
        assert!(!alert.no_data);
        assert_eq!(alert.summary, "alarm-cpu · web-1");
        assert_eq!(alert.severity, Some(Severity::Error));
        assert_eq!(
            alert.dedup_key,
            "0a419ac94f80f2c62f14c01e34000000|al260512T094501SwVt70001|\
             instance_id:6b42c3f4-0000-0000-0000-000000000001"
        );
        assert_eq!(
            alert.labels["instance_id"],
            "6b42c3f4-0000-0000-0000-000000000001"
        );
        assert_eq!(alert.labels["huawei.region"], "cn-north-4");
        assert_eq!(alert.labels["huawei.namespace"], "SYS.ECS");
        assert_eq!(alert.labels["huawei.metric"], "cpu_util");
        assert_eq!(alert.annotations["threshold"], ">= 10%");
        assert_eq!(alert.annotations["value"], "10.04 %");
        assert!(alert.annotations["huawei_url"].starts_with("https://console.huaweicloud.com/"));
        assert_eq!(
            alert.occurred_at,
            Some(TimestampMicros::from_millis(1_778_550_457_194))
        );
    }

    #[test]
    fn ok_resolves_the_same_alarm_and_insufficient_data_is_no_data() {
        let opened = one(&alarm("alarm"));
        let resolved = one(&alarm("ok"));

        assert_eq!(resolved.action, ExternalAction::Resolve);
        assert_eq!(resolved.dedup_key, opened.dedup_key);

        let no_data = one(&alarm("insufficient_data"));

        assert_eq!(no_data.action, ExternalAction::Trigger);
        assert!(no_data.no_data);

        let mut without_status = alarm("ok");
        without_status
            .as_object_mut()
            .unwrap()
            .remove("alarm_status");

        assert_eq!(one(&without_status).action, ExternalAction::Resolve);
    }

    #[test]
    fn a_numeric_level_wins_over_the_words() {
        let mut leveled = alarm("alarm");
        leveled["alarm_level"] = json!(1);

        assert_eq!(one(&leveled).severity, Some(Severity::Critical));

        leveled["alarm_level"] = json!(4);
        assert_eq!(one(&leveled).severity, Some(Severity::Info));
    }

    #[test]
    fn a_confirmation_or_another_message_opens_nothing() {
        let confirmation = br#"{"type": "SubscriptionConfirmation", "message": "You are invited"}"#;
        let parsed = parse(confirmation).unwrap();

        assert!(parsed.alerts.is_empty());
        assert!(parsed.message.unwrap().contains("confirmed"));

        let parsed = parse(&envelope("hello".into())).unwrap();

        assert!(parsed.message.unwrap().contains("not a Cloud Eye alarm"));
        assert!(parse(br#"{"alarm_id": "raw"}"#).is_err());
    }
}
