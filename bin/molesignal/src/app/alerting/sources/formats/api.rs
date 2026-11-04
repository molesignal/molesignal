// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! MoleSignal 自己的接口：一个对象，或最多 [`MAX_API_ALERTS`] 个对象的数组。
//!
//! ```json
//! {"action": "trigger", "dedup_key": "disk-full/web-1", "summary": "web-1 磁盘使用率 95%",
//!  "severity": "critical", "priority": "P2", "urgency": "high", "labels": {"host.name": "web-1"},
//!  "annotations": {"description": "…"}, "links": [{"name": "Runbook", "url": "https://…"}]}
//! ```
//!
//! 接口比各家格式严：不认得的字段、保留的标签键、超出上限的标签都直接拒收，免得写错了没人发现。

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

use super::{Parsed, json, reason, web_address};
use crate::{
    domain::alerting::{
        incident::Severity,
        priority::Priority,
        source::{
            ExternalAction, ExternalAlert, MAX_ALERT_ANNOTATIONS, MAX_ALERT_LABELS,
            MAX_DEDUP_KEY_CHARS, MAX_SUMMARY_CHARS, is_reserved_label,
        },
        urgency::Urgency,
    },
    shared::{Error, Result},
};

/// 一次请求最多几条告警。
pub const MAX_API_ALERTS: usize = 100;
/// 一条告警最多几个链接。
const MAX_LINKS: usize = 10;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApiAlert {
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    dedup_key: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    severity: Option<String>,
    #[serde(default)]
    priority: Option<String>,
    #[serde(default)]
    urgency: Option<String>,
    #[serde(default)]
    labels: BTreeMap<String, String>,
    #[serde(default)]
    annotations: BTreeMap<String, String>,
    #[serde(default)]
    links: Vec<ApiLink>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApiLink {
    #[serde(default)]
    name: Option<String>,
    url: String,
}

pub(super) fn parse(body: &[u8]) -> Result<Parsed> {
    let items = match json(body)? {
        Value::Array(items) => items,
        object @ Value::Object(_) => vec![object],
        _ => {
            return Err(Error::invalid(
                "the request body must be an object or an array",
            ));
        }
    };

    if items.is_empty() {
        return Err(Error::invalid("the request body holds no alert"));
    }

    if items.len() > MAX_API_ALERTS {
        return Err(Error::invalid(format!(
            "a request holds at most {MAX_API_ALERTS} alerts"
        )));
    }

    let alerts = items
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            let alert: ApiAlert = serde_json::from_value(item)
                .map_err(|error| Error::invalid(format!("alert #{index}: {error}")))?;

            alert
                .into_external()
                .map_err(|error| Error::invalid(format!("alert #{index}: {}", reason(&error))))
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(Parsed::alerts(alerts))
}

impl ApiAlert {
    fn into_external(self) -> Result<ExternalAlert> {
        let action = match self.action.as_deref().map(str::trim) {
            None | Some("trigger") => ExternalAction::Trigger,
            Some("acknowledge") => ExternalAction::Acknowledge,
            Some("resolve") => ExternalAction::Resolve,
            Some(other) => {
                return Err(Error::invalid(format!(
                    "action `{other}` must be trigger, acknowledge or resolve"
                )));
            }
        };

        let summary = self.summary.unwrap_or_default().trim().to_string();

        if action == ExternalAction::Trigger && summary.is_empty() {
            return Err(Error::invalid("a trigger needs a summary"));
        }

        if summary.chars().count() > MAX_SUMMARY_CHARS {
            return Err(Error::invalid(format!(
                "summary has at most {MAX_SUMMARY_CHARS} characters"
            )));
        }

        let severity = self.severity.as_deref().map(parse_severity).transpose()?;
        let priority = self.priority.as_deref().map(Priority::parse).transpose()?;

        let urgency = self
            .urgency
            .as_deref()
            .map(|word| {
                Urgency::from_label(word)
                    .ok_or_else(|| Error::invalid(format!("urgency `{word}` must be high or low")))
            })
            .transpose()?;
        check_labels(&self.labels)?;

        let dedup_key = match self.dedup_key.map(|key| key.trim().to_string()) {
            Some(key) if !key.is_empty() => key,
            _ if action == ExternalAction::Trigger => derived_key(&summary, &self.labels),
            _ => return Err(Error::invalid("acknowledge and resolve need a dedup_key")),
        };

        if dedup_key.chars().count() > MAX_DEDUP_KEY_CHARS {
            return Err(Error::invalid(format!(
                "dedup_key has at most {MAX_DEDUP_KEY_CHARS} characters"
            )));
        }

        let annotations = with_links(self.annotations, self.links)?;

        Ok(ExternalAlert {
            action,
            dedup_key,
            summary,
            severity,
            labels: self.labels,
            annotations,
            occurred_at: None,
            no_data: false,
            members: Vec::new(),
            priority,
            urgency,
        })
    }
}

fn parse_severity(value: &str) -> Result<Severity> {
    match value.trim() {
        "critical" => Ok(Severity::Critical),
        "error" => Ok(Severity::Error),
        "warning" => Ok(Severity::Warning),
        "info" => Ok(Severity::Info),
        other => Err(Error::invalid(format!(
            "severity `{other}` must be critical, error, warning or info"
        ))),
    }
}

fn check_labels(labels: &BTreeMap<String, String>) -> Result<()> {
    if labels.len() > MAX_ALERT_LABELS {
        return Err(Error::invalid(format!(
            "an alert has at most {MAX_ALERT_LABELS} labels"
        )));
    }

    match labels
        .keys()
        .find(|key| key.trim().is_empty() || is_reserved_label(key))
    {
        Some(key) => Err(Error::invalid(format!(
            "label key `{key}` is empty or kept for MoleSignal"
        ))),
        None => Ok(()),
    }
}

/// 没带 `dedup_key` 的触发按标题和标签算一个：同样的告警重复发来还是同一个事件。
fn derived_key(summary: &str, labels: &BTreeMap<String, String>) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(summary.as_bytes());

    for (key, value) in labels {
        hasher.update(b"\x00");
        hasher.update(key.as_bytes());
        hasher.update(b"=");
        hasher.update(value.as_bytes());
    }

    hasher.finalize().to_hex()[..32].to_string()
}

/// 链接按名字写进注解（事件详情把网址显示成链接）；没名字的叫 `link`，重名的加上序号。
fn with_links(
    mut annotations: BTreeMap<String, String>,
    links: Vec<ApiLink>,
) -> Result<BTreeMap<String, String>> {
    if links.len() > MAX_LINKS {
        return Err(Error::invalid(format!(
            "an alert has at most {MAX_LINKS} links"
        )));
    }

    for link in links {
        let url = web_address(&link.url).ok_or_else(|| {
            Error::invalid(format!("link `{}` is not an http(s) address", link.url))
        })?;

        let name = link
            .name
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "link".to_string());

        let key = (1..)
            .map(|n| {
                if n == 1 {
                    name.clone()
                } else {
                    format!("{name} {n}")
                }
            })
            .find(|key| !annotations.contains_key(key))
            .unwrap_or(name);

        annotations.insert(key, url);
    }

    if annotations.len() > MAX_ALERT_ANNOTATIONS {
        return Err(Error::invalid(format!(
            "an alert has at most {MAX_ALERT_ANNOTATIONS} annotations and links"
        )));
    }

    Ok(annotations)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(body: &str) -> Result<ExternalAlert> {
        parse(body.as_bytes()).map(|parsed| parsed.alerts.into_iter().next().unwrap())
    }

    #[test]
    fn a_trigger_carries_everything_it_was_given() {
        let alert = one(
            r#"{"dedup_key":"disk-full/web-1","summary":"web-1 磁盘使用率 95%","severity":"critical",
                "labels":{"host.name":"web-1"},"annotations":{"description":"/data"},
                "links":[{"name":"Runbook","url":"https://wiki.example.com/disk"},
                         {"url":"https://grafana.example.com/d/1"}]}"#,
        )
        .unwrap();

        assert_eq!(alert.action, ExternalAction::Trigger);
        assert_eq!(alert.dedup_key, "disk-full/web-1");
        assert_eq!(alert.severity, Some(Severity::Critical));
        assert_eq!(alert.labels["host.name"], "web-1");
        assert_eq!(
            alert.annotations["Runbook"],
            "https://wiki.example.com/disk"
        );
        assert_eq!(alert.annotations["link"], "https://grafana.example.com/d/1");
        assert_eq!(alert.annotations["description"], "/data");
    }

    #[test]
    fn a_trigger_without_a_key_gets_the_same_key_every_time() {
        let body = r#"{"summary":"Queue backlog","labels":{"queue":"orders"}}"#;

        assert_eq!(one(body).unwrap().dedup_key, one(body).unwrap().dedup_key);
        assert_ne!(
            one(body).unwrap().dedup_key,
            one(r#"{"summary":"Queue backlog","labels":{"queue":"refunds"}}"#)
                .unwrap()
                .dedup_key
        );
    }

    #[test]
    fn mistakes_are_refused_with_the_alert_they_are_in() {
        let refused = |body: &str| parse(body.as_bytes()).unwrap_err().to_string();

        assert!(refused(r#"{"action":"resolve"}"#).contains("need a dedup_key"));
        assert!(refused(r#"{"dedup_key":"x"}"#).contains("needs a summary"));
        assert!(refused(r#"{"summary":"x","severity":"high"}"#).contains("severity"));
        assert!(refused(r#"{"summary":"x","dedupKey":"y"}"#).contains("unknown field"));
        assert!(refused(r#"{"summary":"x","labels":{"alert.source":"y"}}"#).contains("kept"));
        assert!(refused(r#"{"summary":"x","links":[{"url":"ftp://x"}]}"#).contains("http(s)"));
        assert!(refused(r#"[{"summary":"x"},{"action":"stop"}]"#).contains("alert #1: action"));
        assert!(refused("[]").contains("no alert"));
    }

    #[test]
    fn a_request_holds_at_most_a_hundred_alerts() {
        let body = format!(
            "[{}]",
            vec![r#"{"summary":"x"}"#; MAX_API_ALERTS + 1].join(",")
        );
        assert!(parse(body.as_bytes()).is_err());
    }
}
