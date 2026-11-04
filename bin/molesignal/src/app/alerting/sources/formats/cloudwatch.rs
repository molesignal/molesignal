// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Amazon CloudWatch 告警：告警发到 SNS 主题，主题的 HTTPS 订阅把消息推过来。SNS 的签名和订阅确认在接入地址
//! 那里做过了，这里只读内容。
//!
//! - 请求体是 SNS 的信封，告警在 `Message` 里，是一段 JSON 串。开了 Raw message delivery 就没有信封和签名，
//!   所以不收。
//! - `NewStateValue`：`ALARM`、`INSUFFICIENT_DATA`（无数据）触发，`OK` 解决。
//! - 一个告警一个事件：去重键是 `AlarmArn`，里面有账号和地域。组合告警没有 `Trigger`，有 `AlarmRule`。
//! - CloudWatch 的告警没有严重度，用来源的默认严重度。
//! - `StateChangeTime` 形如 `2024-03-07T10:12:00.000+0000`，时区偏移没有冒号。

use std::collections::BTreeMap;

use serde_json::Value;

use super::{Parsed, field, json, rfc3339};
use crate::{
    domain::alerting::source::{ExternalAction, ExternalAlert},
    shared::{Error, Result, time::TimestampMicros},
};

pub(super) fn parse(body: &[u8]) -> Result<Parsed> {
    let notice = json(body)?;

    match field(&notice, "Type").as_deref() {
        Some("Notification") => notification(&notice),
        Some("SubscriptionConfirmation") => {
            Ok(Parsed::nothing("the SNS subscription is confirmed"))
        }
        Some("UnsubscribeConfirmation") => Ok(Parsed::nothing("the SNS subscription was removed")),
        _ => Err(Error::invalid(
            "not an SNS message: the body has no `Type`. Keep Raw message delivery off on the \
             subscription",
        )),
    }
}

fn notification(notice: &Value) -> Result<Parsed> {
    let not_an_alarm = || Parsed::nothing("the SNS message is not a CloudWatch alarm");

    let Some(alarm) = notice
        .get("Message")
        .and_then(Value::as_str)
        .and_then(|message| serde_json::from_str::<Value>(message).ok())
    else {
        return Ok(not_an_alarm());
    };

    let (Some(name), Some(state)) = (field(&alarm, "AlarmName"), field(&alarm, "NewStateValue"))
    else {
        return Ok(not_an_alarm());
    };

    let (action, no_data) = match state.as_str() {
        "ALARM" => (ExternalAction::Trigger, false),
        "INSUFFICIENT_DATA" => (ExternalAction::Trigger, true),
        "OK" => (ExternalAction::Resolve, false),
        other => {
            return Ok(Parsed::nothing(format!(
                "CloudWatch alarm state {other} is neither ALARM, INSUFFICIENT_DATA nor OK"
            )));
        }
    };

    let arn = field(&alarm, "AlarmArn");
    let place = arn.as_deref().and_then(AlarmPlace::of);
    let account = field(&alarm, "AWSAccountId");

    let dedup_key = arn.clone().unwrap_or_else(|| {
        format!(
            "{}|{}|{name}",
            account.as_deref().unwrap_or_default(),
            field(&alarm, "Region").unwrap_or_default(),
        )
    });

    let occurred_at = field(&alarm, "StateChangeTime")
        .as_deref()
        .and_then(state_change_time)
        .or_else(|| field(notice, "Timestamp").as_deref().and_then(rfc3339));

    Ok(Parsed::alerts(vec![ExternalAlert {
        action,
        dedup_key,
        labels: labels(&alarm, place.as_ref(), account),
        annotations: annotations(&alarm, place.as_ref(), &name),
        summary: name,
        severity: None,
        occurred_at,
        no_data,
        members: Vec::new(),
        priority: None,
        urgency: None,
    }]))
}

/// `AlarmArn` 里的分区和地域：`arn:aws:cloudwatch:us-east-1:123456789012:alarm:name`。
struct AlarmPlace {
    partition: String,
    region: String,
}

impl AlarmPlace {
    fn of(arn: &str) -> Option<Self> {
        let mut parts = arn.split(':');
        let partition = parts.nth(1)?;
        let region = parts.nth(1)?;

        (!partition.is_empty() && !region.is_empty()).then(|| Self {
            partition: partition.to_string(),
            region: region.to_string(),
        })
    }

    /// 控制台里这个告警的地址。别的分区的控制台地址各不相同，不写。
    fn console_url(&self, name: &str) -> Option<String> {
        let region = &self.region;
        let name: String = url::form_urlencoded::byte_serialize(name.as_bytes()).collect();

        let host = match self.partition.as_str() {
            "aws" => format!("{region}.console.aws.amazon.com"),
            "aws-cn" => format!("{region}.console.amazonaws.cn"),
            _ => return None,
        };

        Some(format!(
            "https://{host}/cloudwatch/home?region={region}#alarmsV2:alarm/{name}"
        ))
    }
}

fn labels(
    alarm: &Value,
    place: Option<&AlarmPlace>,
    account: Option<String>,
) -> BTreeMap<String, String> {
    let trigger = alarm.get("Trigger").cloned().unwrap_or(Value::Null);
    let mut labels = BTreeMap::new();

    for dimension in trigger
        .get("Dimensions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let (Some(name), Some(value)) = (field(dimension, "name"), field(dimension, "value")) {
            labels.insert(name, value);
        }
    }

    let extra = [
        ("aws.account_id", account),
        ("aws.region", place.map(|place| place.region.clone())),
        ("aws.namespace", field(&trigger, "Namespace")),
        ("aws.metric", field(&trigger, "MetricName")),
    ];

    labels.extend(
        extra
            .into_iter()
            .filter_map(|(key, value)| value.map(|value| (key.to_string(), value))),
    );

    labels
}

fn annotations(alarm: &Value, place: Option<&AlarmPlace>, name: &str) -> BTreeMap<String, String> {
    let trigger = alarm.get("Trigger").cloned().unwrap_or(Value::Null);

    let threshold = field(&trigger, "ComparisonOperator")
        .zip(field(&trigger, "Threshold"))
        .map(|(comparison, value)| format!("{comparison} {value}"));

    [
        ("message", field(alarm, "AlarmDescription")),
        ("reason", field(alarm, "NewStateReason")),
        ("region_name", field(alarm, "Region")),
        ("threshold", threshold),
        ("alarm_rule", field(alarm, "AlarmRule")),
        (
            "cloudwatch_url",
            place.and_then(|place| place.console_url(name)),
        ),
    ]
    .into_iter()
    .filter_map(|(key, value)| value.map(|value| (key.to_string(), value)))
    .collect()
}

/// `2024-03-07T10:12:00.000+0000`。
fn state_change_time(value: &str) -> Option<TimestampMicros> {
    let parsed = chrono::DateTime::parse_from_str(value.trim(), "%Y-%m-%dT%H:%M:%S%.f%z").ok()?;

    Some(TimestampMicros(parsed.timestamp_micros()))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn alarm(state: &str, changed_at: &str) -> Value {
        json!({
            "AlarmName": "checkout-cpu-high",
            "AlarmDescription": "CPU above 80% on the checkout fleet",
            "AWSAccountId": "123456789012",
            "AlarmConfigurationUpdatedTimestamp": "2024-03-01T08:00:00.000+0000",
            "NewStateValue": state,
            "NewStateReason": "Threshold Crossed: 1 out of the last 1 datapoints [96.5] was greater than the threshold (80.0).",
            "StateChangeTime": changed_at,
            "Region": "US East (N. Virginia)",
            "AlarmArn": "arn:aws:cloudwatch:us-east-1:123456789012:alarm:checkout-cpu-high",
            "OldStateValue": "OK",
            "OKActions": ["arn:aws:sns:us-east-1:123456789012:alerts"],
            "AlarmActions": ["arn:aws:sns:us-east-1:123456789012:alerts"],
            "InsufficientDataActions": [],
            "Trigger": {
                "MetricName": "CPUUtilization",
                "Namespace": "AWS/EC2",
                "StatisticType": "Statistic",
                "Statistic": "AVERAGE",
                "Unit": null,
                "Dimensions": [{"value": "i-0123456789abcdef0", "name": "InstanceId"}],
                "Period": 300,
                "EvaluationPeriods": 1,
                "ComparisonOperator": "GreaterThanThreshold",
                "Threshold": 80.0,
                "TreatMissingData": "missing",
                "EvaluateLowSampleCountPercentile": ""
            }
        })
    }

    fn envelope(message: String) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "Type": "Notification",
            "MessageId": "22b80b92-fdea-4c2c-8f9d-bdfb0c7bf324",
            "TopicArn": "arn:aws:sns:us-east-1:123456789012:alerts",
            "Subject": "ALARM: \"checkout-cpu-high\" in US East (N. Virginia)",
            "Message": message,
            "Timestamp": "2024-03-07T10:12:01.123Z",
            "SignatureVersion": "1",
            "Signature": "EXAMPLE",
            "SigningCertURL": "https://sns.us-east-1.amazonaws.com/SimpleNotificationService-0000.pem",
            "UnsubscribeURL": "https://sns.us-east-1.amazonaws.com/?Action=Unsubscribe"
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
    fn an_alarm_opens_with_what_it_watches() {
        let alert = one(&alarm("ALARM", "2024-03-07T10:12:00.000+0000"));

        assert_eq!(alert.action, ExternalAction::Trigger);
        assert!(!alert.no_data);
        assert_eq!(alert.summary, "checkout-cpu-high");
        assert_eq!(alert.severity, None);
        assert_eq!(
            alert.dedup_key,
            "arn:aws:cloudwatch:us-east-1:123456789012:alarm:checkout-cpu-high"
        );
        assert_eq!(alert.labels["InstanceId"], "i-0123456789abcdef0");
        assert_eq!(alert.labels["aws.region"], "us-east-1");
        assert_eq!(alert.labels["aws.account_id"], "123456789012");
        assert_eq!(alert.labels["aws.namespace"], "AWS/EC2");
        assert_eq!(alert.labels["aws.metric"], "CPUUtilization");
        assert_eq!(
            alert.annotations["message"],
            "CPU above 80% on the checkout fleet"
        );
        assert_eq!(alert.annotations["threshold"], "GreaterThanThreshold 80.0");
        assert_eq!(
            alert.annotations["cloudwatch_url"],
            "https://us-east-1.console.aws.amazon.com/cloudwatch/home?region=us-east-1\
             #alarmsV2:alarm/checkout-cpu-high"
        );
        assert_eq!(
            alert.occurred_at,
            Some(TimestampMicros::from_secs(1_709_806_320))
        );
    }

    #[test]
    fn ok_resolves_the_same_alarm_and_missing_data_counts_as_no_data() {
        let opened = one(&alarm("ALARM", "2024-03-07T10:12:00.000+0000"));
        let resolved = one(&alarm("OK", "2024-03-07T10:30:00.000+0000"));

        assert_eq!(resolved.action, ExternalAction::Resolve);
        assert_eq!(resolved.dedup_key, opened.dedup_key);

        let no_data = one(&alarm("INSUFFICIENT_DATA", "2024-03-07T10:40:00.000+0000"));

        assert_eq!(no_data.action, ExternalAction::Trigger);
        assert!(no_data.no_data);
    }

    #[test]
    fn a_composite_alarm_has_a_rule_and_no_trigger() {
        let mut composite = alarm("ALARM", "2024-03-07T10:12:00.000+0000");

        composite.as_object_mut().unwrap().remove("Trigger");
        composite["AlarmRule"] = json!("ALARM(cpu-high) AND ALARM(mem-high)");

        let alert = one(&composite);

        assert_eq!(
            alert.annotations["alarm_rule"],
            "ALARM(cpu-high) AND ALARM(mem-high)"
        );
        assert!(!alert.labels.contains_key("aws.metric"));
    }

    #[test]
    fn the_console_address_follows_the_partition() {
        let china = AlarmPlace::of("arn:aws-cn:cloudwatch:cn-north-1:1:alarm:a b").unwrap();
        assert_eq!(
            china.console_url("a b").unwrap(),
            "https://cn-north-1.console.amazonaws.cn/cloudwatch/home?region=cn-north-1\
             #alarmsV2:alarm/a+b"
        );

        let gov = AlarmPlace::of("arn:aws-us-gov:cloudwatch:us-gov-west-1:1:alarm:a").unwrap();
        assert_eq!(gov.console_url("a"), None);

        assert!(AlarmPlace::of("not-an-arn").is_none());
    }

    #[test]
    fn a_confirmation_or_another_message_opens_nothing() {
        let confirmation = serde_json::to_vec(&json!({
            "Type": "SubscriptionConfirmation",
            "Message": "You have chosen to subscribe to the topic",
            "SubscribeURL": "https://sns.us-east-1.amazonaws.com/?Action=ConfirmSubscription"
        }))
        .unwrap();

        let parsed = parse(&confirmation).unwrap();
        assert!(parsed.alerts.is_empty());
        assert!(parsed.message.unwrap().contains("confirmed"));

        for message in ["hello from the console", "{\"some\": \"other json\"}"] {
            let parsed = parse(&envelope(message.to_string())).unwrap();

            assert!(parsed.alerts.is_empty(), "{message}");
            assert!(parsed.message.unwrap().contains("not a CloudWatch alarm"));
        }
    }

    #[test]
    fn raw_message_delivery_is_refused_and_says_so() {
        let raw = alarm("ALARM", "2024-03-07T10:12:00.000+0000").to_string();
        let error = parse(raw.as_bytes()).unwrap_err().to_string();

        assert!(error.contains("Raw message delivery"), "{error}");
    }
}
