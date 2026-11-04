// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Datadog 的 Webhooks 集成。请求体由 Datadog 里填的模板决定，文档给出这个模板：字段名是我们定的，
//! 值是 Datadog 的变量。
//!
//! ```json
//! {"alert_id": "$ALERT_ID", "alert_cycle_key": "$ALERT_CYCLE_KEY",
//!  "transition": "$ALERT_TRANSITION", "alert_type": "$ALERT_TYPE", "priority": "$ALERT_PRIORITY",
//!  "title": "$ALERT_TITLE", "event_title": "$EVENT_TITLE", "message": "$TEXT_ONLY_MSG",
//!  "scope": "$ALERT_SCOPE", "tags": "$TAGS", "hostname": "$HOSTNAME", "link": "$LINK",
//!  "snapshot": "$SNAPSHOT", "query": "$ALERT_QUERY", "org_id": "$ORG_ID",
//!  "last_updated": "$LAST_UPDATED"}
//! ```
//!
//! - 一个监控一个事件，多告警监控按分组（scope）各开各的。
//! - 去重键是站点（`$LINK` 的域名：datadoghq.com、datadoghq.eu、us3、us5、ap1 等）、组织、监控 id
//!   与 scope：不同站点、不同组织的监控 id 会重复。
//! - `Recovered` 解决，其余（Triggered、Warn、No Data、Renotify 及各自的 Re-）都算触发。
//! - 严重度先看 `$ALERT_PRIORITY`（P1…P5），没有再看是 Warn、No Data 还是 `$ALERT_TYPE`。

use std::collections::BTreeMap;

use super::{Parsed, epoch, host_of, json, text, web_address};
use crate::{
    domain::alerting::{
        incident::Severity,
        priority::Priority,
        source::{ExternalAction, ExternalAlert, severity_from_word},
    },
    shared::{Error, Result},
};

pub(super) fn parse(body: &[u8]) -> Result<Parsed> {
    let payload = json(body)?;

    if !payload.is_object() {
        return Err(Error::invalid("the Datadog payload must be a JSON object"));
    }

    let value = |key: &str| payload.get(key).and_then(text).and_then(expanded);

    let (Some(monitor), Some(transition)) = (value("alert_id"), value("transition")) else {
        return Err(Error::invalid(
            "the Datadog payload has no `alert_id` or `transition`: use the webhook payload \
             given in the MoleSignal docs",
        ));
    };

    let link = value("link").and_then(|link| web_address(&link));
    let site = link.as_deref().and_then(site_of).unwrap_or_default();
    let organization = value("org_id").unwrap_or_default();
    let scope = value("scope")
        .filter(|scope| scope != "*")
        .unwrap_or_default();
    let lowered = transition.to_ascii_lowercase();

    let action = if lowered.starts_with("recovered") {
        ExternalAction::Resolve
    } else {
        ExternalAction::Trigger
    };

    let severity = value("priority")
        .and_then(|priority| severity_from_word(&priority))
        .or_else(|| transition_severity(&lowered, value("alert_type").as_deref()));
    let priority = value("priority").and_then(|priority| Priority::from_label(&priority));

    let mut labels = tags(value("tags").as_deref().unwrap_or_default());
    labels.extend(tags(&scope));

    if let Some(host) = value("hostname") {
        labels.entry("host".into()).or_insert(host);
    }

    labels.insert("datadog.monitor_id".into(), monitor.clone());

    if !site.is_empty() {
        labels.insert("datadog.site".into(), site.clone());
    }

    let mut annotations = BTreeMap::new();
    annotations.insert("transition".to_string(), transition.clone());

    let notes = [
        ("message", value("message")),
        ("query", value("query")),
        ("event_title", value("event_title")),
        ("datadog_url", link),
        (
            "snapshot_url",
            value("snapshot").and_then(|url| web_address(&url)),
        ),
    ];

    for (key, note) in notes {
        if let Some(note) = note {
            annotations.insert(key.to_string(), note);
        }
    }

    let summary = value("title")
        .or_else(|| value("event_title"))
        .unwrap_or_else(|| format!("Datadog monitor {monitor}"));

    let occurred_at = payload
        .get("last_updated")
        .and_then(epoch)
        .or_else(|| payload.get("date").and_then(epoch));

    Ok(Parsed::alerts(vec![ExternalAlert {
        action,
        dedup_key: format!("{site}|{organization}|{monitor}|{scope}"),
        summary,
        severity,
        labels,
        annotations,
        occurred_at,
        no_data: lowered.contains("no data"),
        members: Vec::new(),
        priority,
        urgency: None,
    }]))
}

/// 没展开的变量（模板里写了 `$ALERT_SCOPE`，这类监控却没有这个值）算没有。
fn expanded(value: String) -> Option<String> {
    let unexpanded = value.strip_prefix('$').is_some_and(|name| {
        !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
    });

    (!unexpanded).then_some(value)
}

/// 站点：`$LINK` 的域名去掉开头的 `app.`。
fn site_of(link: &str) -> Option<String> {
    let host = host_of(link)?;
    Some(host.strip_prefix("app.").unwrap_or(&host).to_string())
}

/// `env:prod, host:web-1, monitor` 这样的标签串：`键:值` 写成标签，只有名字的值为空。
fn tags(list: &str) -> BTreeMap<String, String> {
    list.split(',')
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(|tag| match tag.split_once(':') {
            Some((key, value)) => (key.trim().to_string(), value.trim().to_string()),
            None => (tag.to_string(), String::new()),
        })
        .collect()
}

/// 没有优先级时按 transition 与 `$ALERT_TYPE` 定严重度。
fn transition_severity(transition: &str, alert_type: Option<&str>) -> Option<Severity> {
    if transition.contains("warn") || transition.contains("no data") {
        return Some(Severity::Warning);
    }

    match alert_type.map(str::to_ascii_lowercase).as_deref() {
        Some("warning") => Some(Severity::Warning),
        Some("info") => Some(Severity::Info),
        Some("success") => None,
        _ => Some(Severity::Error),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;
    use crate::shared::time::TimestampMicros;

    fn payload(overrides: &[(&str, &str)]) -> Vec<u8> {
        let mut payload = serde_json::json!({
            "alert_id": "1234567",
            "alert_cycle_key": "7015912345678901234",
            "transition": "Triggered",
            "alert_type": "error",
            "priority": "P2",
            "title": "High error rate on checkout",
            "event_title": "[P2] [Triggered on {service:checkout}] High error rate on checkout",
            "message": "Error rate above 5% for 10 minutes",
            "scope": "service:checkout,env:prod",
            "tags": "env:prod,monitor,service:checkout,team:payments",
            "hostname": "",
            "link": "https://app.datadoghq.eu/monitors/1234567?group=service%3Acheckout",
            "snapshot": "$SNAPSHOT",
            "query": "sum(last_10m):sum:trace.http.request.errors{env:prod} > 5",
            "org_id": "321",
            "last_updated": "1761552000000"
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
    fn a_monitor_group_opens_one_alert_keyed_by_site_and_organization() {
        let alert = one(&[]);

        assert_eq!(alert.action, ExternalAction::Trigger);
        assert_eq!(
            alert.dedup_key,
            "datadoghq.eu|321|1234567|service:checkout,env:prod"
        );
        assert_eq!(alert.summary, "High error rate on checkout");
        assert_eq!(alert.severity, Some(Severity::Error));
        assert_eq!(alert.labels["service"], "checkout");
        assert_eq!(alert.labels["monitor"], "");
        assert_eq!(alert.labels["datadog.site"], "datadoghq.eu");
        assert_eq!(alert.labels["datadog.monitor_id"], "1234567");
        assert!(!alert.annotations.contains_key("snapshot_url"));
        assert_eq!(
            alert.occurred_at,
            Some(TimestampMicros(1_761_552_000_000_000))
        );
    }

    #[test]
    fn the_same_monitor_on_another_site_is_another_alert() {
        let us = one(&[("link", "https://app.datadoghq.com/monitors/1234567")]);
        let us5 = one(&[("link", "https://us5.datadoghq.com/monitors/1234567")]);

        assert_ne!(one(&[]).dedup_key, us.dedup_key);
        assert_ne!(us.dedup_key, us5.dedup_key);
        assert_eq!(us5.labels["datadog.site"], "us5.datadoghq.com");
    }

    #[test]
    fn transitions_decide_the_action_and_the_severity() {
        assert_eq!(
            one(&[("transition", "Recovered")]).action,
            ExternalAction::Resolve
        );

        let warn = one(&[("transition", "Warn"), ("priority", "$ALERT_PRIORITY")]);
        assert_eq!(warn.severity, Some(Severity::Warning));

        let no_data = one(&[("transition", "No Data"), ("priority", "")]);
        assert!(no_data.no_data);
        assert_eq!(no_data.action, ExternalAction::Trigger);

        assert_eq!(
            one(&[("priority", "P1")]).severity,
            Some(Severity::Critical)
        );
    }

    #[test]
    fn a_payload_without_the_template_fields_is_refused() {
        let error = parse(br#"{"body": "x", "title": "y"}"#)
            .unwrap_err()
            .to_string();
        assert!(error.contains("MoleSignal docs"));
        assert!(parse(&payload(&[("alert_id", "$ALERT_ID")])).is_err());
    }
}
