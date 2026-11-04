// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Alertmanager 的 webhook 与 Grafana 告警的 Webhook 联系点：一次 POST 是一个通知分组，一个分组
//! 开一个事件。
//!
//! - 去重键是发送方地址（`externalURL`，Grafana 再加 `orgId`）加 `groupKey`：好几个 region 的
//!   Alertmanager 用同一套路由时，分组的键一样也不会并成一个事件。同一个 Alertmanager 集群的各个
//!   副本要配同一个 `--web.external-url`。
//! - 分组的 `status` 为 firing 时触发，为 resolved（组里的告警都恢复了）时解决。
//! - 严重度取组里正在触发的告警里 `severity` 标签最高的一档。
//! - 发送方的时间：触发取在触发的告警里最晚的 `startsAt`，解决取最晚的 `endsAt`。

use std::collections::BTreeMap;

use serde::Deserialize;

use super::{Parsed, rfc3339, web_address};
use crate::{
    domain::alerting::source::{
        AlertMember, ExternalAction, ExternalAlert, MemberStatus, severity_from_word,
    },
    shared::{Error, Result},
};

/// 谁发来的：两家格式一样，链接与身份略有不同。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Sender {
    Alertmanager,
    Grafana,
}

impl Sender {
    const fn name(self) -> &'static str {
        match self {
            Self::Alertmanager => "Alertmanager",
            Self::Grafana => "Grafana",
        }
    }

    /// 回到发送方首页的注解名。
    const fn home_annotation(self) -> &'static str {
        match self {
            Self::Alertmanager => "alertmanager_url",
            Self::Grafana => "grafana_url",
        }
    }
}

#[derive(Debug, Deserialize)]
struct Notification {
    #[serde(rename = "groupKey", default)]
    group_key: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(rename = "groupLabels", default)]
    group_labels: BTreeMap<String, String>,
    #[serde(rename = "commonLabels", default)]
    common_labels: BTreeMap<String, String>,
    #[serde(rename = "commonAnnotations", default)]
    common_annotations: BTreeMap<String, String>,
    #[serde(rename = "externalURL", default)]
    external_url: Option<String>,
    /// Grafana 的组织 id。
    #[serde(rename = "orgId", default)]
    org_id: Option<serde_json::Value>,
    #[serde(default)]
    alerts: Vec<Alert>,
}

#[derive(Debug, Deserialize)]
struct Alert {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    labels: BTreeMap<String, String>,
    #[serde(default)]
    annotations: BTreeMap<String, String>,
    #[serde(rename = "startsAt", default)]
    starts_at: Option<String>,
    #[serde(rename = "endsAt", default)]
    ends_at: Option<String>,
    #[serde(rename = "generatorURL", default)]
    generator_url: Option<String>,
    #[serde(rename = "silenceURL", default)]
    silence_url: Option<String>,
    #[serde(rename = "dashboardURL", default)]
    dashboard_url: Option<String>,
    #[serde(rename = "panelURL", default)]
    panel_url: Option<String>,
    #[serde(rename = "imageURL", default)]
    image_url: Option<String>,
    #[serde(rename = "valueString", default)]
    value_string: Option<String>,
}

impl Alert {
    fn firing(&self) -> bool {
        self.status.as_deref() != Some("resolved")
    }
}

pub(super) fn parse(body: &[u8], sender: Sender) -> Result<Parsed> {
    let notification: Notification = serde_json::from_slice(body).map_err(|error| {
        Error::invalid(format!("not a {} notification: {error}", sender.name()))
    })?;

    if notification.alerts.is_empty() {
        return Err(Error::invalid(format!(
            "the {} notification holds no alert",
            sender.name()
        )));
    }

    let firing: Vec<&Alert> = notification
        .alerts
        .iter()
        .filter(|alert| alert.firing())
        .collect();

    let action = match notification.status.as_deref() {
        Some("resolved") => ExternalAction::Resolve,
        Some("firing") => ExternalAction::Trigger,
        _ if firing.is_empty() => ExternalAction::Resolve,
        _ => ExternalAction::Trigger,
    };

    let severity = firing
        .iter()
        .filter_map(|alert| alert.labels.get("severity"))
        .filter_map(|word| severity_from_word(word))
        .max()
        .or_else(|| {
            notification
                .common_labels
                .get("severity")
                .and_then(|word| severity_from_word(word))
        });

    let occurred_at = match action {
        ExternalAction::Resolve => notification
            .alerts
            .iter()
            .filter_map(|alert| alert.ends_at.as_deref().and_then(rfc3339))
            .max(),
        _ => firing
            .iter()
            .filter_map(|alert| alert.starts_at.as_deref().and_then(rfc3339))
            .max(),
    };

    let no_data = sender == Sender::Grafana
        && notification
            .common_labels
            .get("alertname")
            .map(String::as_str)
            == Some("DatasourceNoData");

    let alert = ExternalAlert {
        action,
        dedup_key: dedup_key(&notification, sender),
        summary: summary(&notification, sender),
        severity,
        labels: notification.common_labels.clone(),
        annotations: annotations(&notification, sender),
        occurred_at,
        no_data,
        members: notification.alerts.iter().map(member).collect(),
        priority: None,
        urgency: None,
    };

    Ok(Parsed::alerts(vec![alert]))
}

/// 发送方地址（Grafana 再加组织）加分组的键；没有 `groupKey` 时用分组标签代替。
fn dedup_key(notification: &Notification, sender: Sender) -> String {
    let group = notification.group_key.clone().unwrap_or_else(|| {
        let labels: Vec<String> = notification
            .group_labels
            .iter()
            .map(|(key, value)| format!("{key}={value:?}"))
            .collect();

        format!("{{{}}}", labels.join(","))
    });

    let address = notification
        .external_url
        .as_deref()
        .map(|url| url.trim().trim_end_matches('/'))
        .unwrap_or_default();

    match sender {
        Sender::Alertmanager => format!("{address}|{group}"),
        Sender::Grafana => {
            let organization = notification
                .org_id
                .as_ref()
                .and_then(super::text)
                .unwrap_or_default();

            format!("{address}|{organization}|{group}")
        }
    }
}

/// 组里只有一条时用它自己的 summary 或 alertname；多条时用公共的 summary、公共的 alertname，
/// 再不行列出组里的 alertname。
fn summary(notification: &Notification, sender: Sender) -> String {
    if let [only] = notification.alerts.as_slice()
        && let Some(title) = only
            .annotations
            .get("summary")
            .or_else(|| only.labels.get("alertname"))
    {
        return title.clone();
    }

    if let Some(title) = notification
        .common_annotations
        .get("summary")
        .or_else(|| notification.common_labels.get("alertname"))
    {
        return title.clone();
    }

    let mut names: Vec<&str> = notification
        .alerts
        .iter()
        .filter_map(|alert| alert.labels.get("alertname").map(String::as_str))
        .collect();

    names.sort_unstable();
    names.dedup();

    match names.len() {
        0 => format!("{} alert", sender.name()),
        1..=3 => names.join(", "),
        more => format!("{} +{}", names[..3].join(", "), more - 3),
    }
}

/// 公共的注解，加上回到发送方的链接：组里只有一条告警时再带上它自己的链接。
fn annotations(notification: &Notification, sender: Sender) -> BTreeMap<String, String> {
    let mut annotations = notification.common_annotations.clone();

    if let Some(url) = notification.external_url.as_deref().and_then(web_address) {
        annotations.insert(sender.home_annotation().into(), url);
    }

    if let [only] = notification.alerts.as_slice() {
        let links = [
            ("generator_url", &only.generator_url),
            ("silence_url", &only.silence_url),
            ("dashboard_url", &only.dashboard_url),
            ("panel_url", &only.panel_url),
            ("image_url", &only.image_url),
        ];

        for (name, url) in links {
            if let Some(url) = url.as_deref().and_then(web_address) {
                annotations.insert(name.into(), url);
            }
        }

        if let Some(values) = only
            .value_string
            .as_deref()
            .filter(|text| !text.trim().is_empty())
        {
            annotations.insert("values".into(), values.trim().to_string());
        }
    }

    annotations
}

fn member(alert: &Alert) -> AlertMember {
    let status = if alert.firing() {
        MemberStatus::Firing
    } else {
        MemberStatus::Resolved
    };

    AlertMember {
        status,
        labels: alert.labels.clone(),
        summary: alert
            .annotations
            .get("summary")
            .or_else(|| alert.annotations.get("description"))
            .cloned(),
        starts_at: alert.starts_at.as_deref().and_then(rfc3339),
        ends_at: alert
            .ends_at
            .as_deref()
            .and_then(rfc3339)
            .filter(|_| status == MemberStatus::Resolved),
        url: alert.generator_url.as_deref().and_then(web_address),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{domain::alerting::incident::Severity, shared::time::TimestampMicros};

    /// 一个分组里最晚的开始时间。
    fn latest_start(alert: &ExternalAlert) -> Option<TimestampMicros> {
        alert
            .members
            .iter()
            .filter_map(|member| member.starts_at)
            .max()
    }

    const ALERTMANAGER: &str = r#"{
      "version": "4",
      "groupKey": "{}:{alertname=\"HighCPU\"}",
      "truncatedAlerts": 0,
      "status": "firing",
      "receiver": "molesignal",
      "groupLabels": {"alertname": "HighCPU"},
      "commonLabels": {"alertname": "HighCPU", "job": "node"},
      "commonAnnotations": {},
      "externalURL": "http://alertmanager.example.com:9093/",
      "alerts": [
        {"status": "firing",
         "labels": {"alertname": "HighCPU", "instance": "web-1:9100", "job": "node", "severity": "warning"},
         "annotations": {"summary": "CPU above 90% on web-1"},
         "startsAt": "2026-10-27T08:00:00Z", "endsAt": "0001-01-01T00:00:00Z",
         "generatorURL": "http://prometheus.example.com:9090/graph?g0.expr=cpu", "fingerprint": "a1"},
        {"status": "firing",
         "labels": {"alertname": "HighCPU", "instance": "web-2:9100", "job": "node", "severity": "critical"},
         "annotations": {"summary": "CPU above 90% on web-2"},
         "startsAt": "2026-10-27T08:01:00Z", "endsAt": "0001-01-01T00:00:00Z",
         "generatorURL": "http://prometheus.example.com:9090/graph?g0.expr=cpu", "fingerprint": "b2"}
      ]
    }"#;

    const GRAFANA: &str = r#"{
      "receiver": "My Super Webhook",
      "status": "firing",
      "orgId": 1,
      "alerts": [
        {"status": "firing",
         "labels": {"alertname": "High memory usage", "team": "blue", "zone": "us-1"},
         "annotations": {"description": "The system has high memory usage",
                         "runbook_url": "https://myrunbook.com/runbook/1234",
                         "summary": "This alert was triggered for zone us-1"},
         "startsAt": "2021-10-12T09:51:03.157076+02:00", "endsAt": "0001-01-01T00:00:00Z",
         "generatorURL": "https://play.grafana.org/alerting/1afz29v7z/edit",
         "fingerprint": "c6eadffa33fcdf37",
         "silenceURL": "https://play.grafana.org/alerting/silence/new?alertmanager=grafana",
         "dashboardURL": "", "panelURL": "",
         "values": {"B": 44.23943737541908, "C": 1},
         "valueString": "[ var='B' labels={} value=44.2 ]"}
      ],
      "groupLabels": {},
      "commonLabels": {"alertname": "High memory usage", "team": "blue", "zone": "us-1"},
      "commonAnnotations": {"summary": "This alert was triggered for zone us-1"},
      "externalURL": "https://play.grafana.org/",
      "version": "1",
      "groupKey": "{}:{}",
      "truncatedAlerts": 0,
      "title": "[FIRING:1]  (High memory usage blue us-1)",
      "state": "alerting",
      "message": "**Firing**"
    }"#;

    fn one(body: &str, sender: Sender) -> ExternalAlert {
        parse(body.as_bytes(), sender).unwrap().alerts.remove(0)
    }

    #[test]
    fn an_alertmanager_group_is_one_alert_with_its_members() {
        let alert = one(ALERTMANAGER, Sender::Alertmanager);

        assert_eq!(alert.action, ExternalAction::Trigger);
        assert_eq!(
            alert.dedup_key,
            r#"http://alertmanager.example.com:9093|{}:{alertname="HighCPU"}"#
        );
        assert_eq!(alert.summary, "HighCPU");
        assert_eq!(alert.severity, Some(Severity::Critical));
        assert_eq!(alert.labels.len(), 2);
        assert_eq!(alert.members.len(), 2);
        assert_eq!(alert.members[1].labels["instance"], "web-2:9100");
        assert_eq!(alert.members[0].ends_at, None);
        assert_eq!(
            alert.annotations["alertmanager_url"],
            "http://alertmanager.example.com:9093/"
        );
        assert_eq!(alert.occurred_at, latest_start(&alert));
        assert_eq!(alert.occurred_at, rfc3339("2026-10-27T08:01:00Z"));
    }

    #[test]
    fn the_same_group_from_two_alertmanagers_has_two_keys() {
        let other = ALERTMANAGER.replace("alertmanager.example.com", "alertmanager.eu.example.com");

        assert_ne!(
            one(ALERTMANAGER, Sender::Alertmanager).dedup_key,
            one(&other, Sender::Alertmanager).dedup_key
        );
    }

    #[test]
    fn a_resolved_group_is_dated_by_its_last_end() {
        let resolved = ALERTMANAGER
            .replace(r#""status": "firing""#, r#""status": "resolved""#)
            .replacen(
                r#""endsAt": "0001-01-01T00:00:00Z""#,
                r#""endsAt": "2026-10-27T09:00:00Z""#,
                1,
            )
            .replacen(
                r#""endsAt": "0001-01-01T00:00:00Z""#,
                r#""endsAt": "2026-10-27T09:05:00Z""#,
                1,
            );

        let alert = one(&resolved, Sender::Alertmanager);

        assert_eq!(alert.action, ExternalAction::Resolve);
        assert_eq!(alert.occurred_at, rfc3339("2026-10-27T09:05:00Z"));
        assert!(
            alert
                .members
                .iter()
                .all(|member| member.status == MemberStatus::Resolved)
        );
        assert_eq!(alert.members[1].ends_at, rfc3339("2026-10-27T09:05:00Z"));
    }

    #[test]
    fn a_grafana_alert_brings_its_links_and_its_organization() {
        let alert = one(GRAFANA, Sender::Grafana);

        assert_eq!(alert.dedup_key, "https://play.grafana.org|1|{}:{}");
        assert_eq!(alert.summary, "This alert was triggered for zone us-1");
        assert_eq!(alert.severity, None);
        assert_eq!(
            alert.annotations["grafana_url"],
            "https://play.grafana.org/"
        );
        assert_eq!(
            alert.annotations["generator_url"],
            "https://play.grafana.org/alerting/1afz29v7z/edit"
        );
        assert!(alert.annotations.contains_key("silence_url"));
        assert!(!alert.annotations.contains_key("dashboard_url"));
        assert_eq!(
            alert.annotations["values"],
            "[ var='B' labels={} value=44.2 ]"
        );
        // 带时区偏移的时间换成 UTC。
        assert_eq!(alert.occurred_at, rfc3339("2021-10-12T07:51:03.157076Z"));
    }

    #[test]
    fn something_else_is_refused() {
        assert!(parse(br#"{"alerts": []}"#, Sender::Alertmanager).is_err());
        assert!(parse(b"[1,2]", Sender::Grafana).is_err());
    }
}
