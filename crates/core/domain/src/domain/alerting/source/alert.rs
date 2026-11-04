// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 外部告警：各家格式读出来以后的统一样子。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::is_reserved_label;
use crate::{
    domain::alerting::{incident::Severity, priority::Priority, urgency::Urgency},
    shared::time::TimestampMicros,
};

/// 接口里 `dedup_key` 最多几个字符。
pub const MAX_DEDUP_KEY_CHARS: usize = 255;
/// 事件标题最多几个字符；更长的截断。
pub const MAX_SUMMARY_CHARS: usize = 1024;
/// 一条告警最多带几个标签；多出的丢掉。
pub const MAX_ALERT_LABELS: usize = 64;
/// 一条告警最多带几个注解；多出的丢掉。
pub const MAX_ALERT_ANNOTATIONS: usize = 64;
/// 注解值最多几个字符；更长的截断。
pub const MAX_ANNOTATION_CHARS: usize = 4096;
/// 一个通知分组最多记几条告警。
pub const MAX_MEMBERS: usize = 100;

/// 发送方要对一条告警做什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalAction {
    Trigger,
    Acknowledge,
    Resolve,
}

impl ExternalAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Trigger => "trigger",
            Self::Acknowledge => "acknowledge",
            Self::Resolve => "resolve",
        }
    }
}

/// 通知分组里一条告警的状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberStatus {
    Firing,
    Resolved,
}

/// 通知分组里的一条告警（Alertmanager、Grafana）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertMember {
    pub status: MemberStatus,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub starts_at: Option<TimestampMicros>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ends_at: Option<TimestampMicros>,
    /// 回到发送方看这条告警的地址。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// 一条外部告警。
#[derive(Debug, Clone, PartialEq)]
pub struct ExternalAlert {
    pub action: ExternalAction,
    /// 来源内的去重键：同一个键同时只有一个活跃事件。各家格式把发送方的账号、站点或实例算在
    /// 里面，一个来源接好几个发送方时不会撞。
    pub dedup_key: String,
    pub summary: String,
    pub severity: Option<Severity>,
    pub labels: BTreeMap<String, String>,
    pub annotations: BTreeMap<String, String>,
    /// 发送方这条消息的时间。比这个去重键上次用到的早，就是晚到的旧消息。
    pub occurred_at: Option<TimestampMicros>,
    /// 发送方报的是没有数据。
    pub no_data: bool,
    /// 通知分组里的各条告警；不按分组的类型为空。
    pub members: Vec<AlertMember>,
    /// 发送方给的优先级（我们的接口、Datadog 的 P1–P5、PagerDuty 的 priority）；没有就不设。
    pub priority: Option<Priority>,
    /// 发送方给的紧急度（我们的接口、PagerDuty 的 urgency）；没有时按来源的升级策略定。
    pub urgency: Option<Urgency>,
}

impl ExternalAlert {
    /// 各家格式读出来以后统一收拾：去掉保留的和空的标签键，标题、注解超长的截断，标签、注解、
    /// 分组成员超出上限的丢掉。
    pub fn tidy(mut self) -> Self {
        self.summary = truncate(self.summary.trim(), MAX_SUMMARY_CHARS);
        self.labels = tidy_labels(self.labels);

        self.annotations = self
            .annotations
            .into_iter()
            .map(|(key, value)| (key.trim().to_string(), value))
            .filter(|(key, value)| !key.is_empty() && !value.trim().is_empty())
            .take(MAX_ALERT_ANNOTATIONS)
            .map(|(key, value)| (key, truncate(&value, MAX_ANNOTATION_CHARS)))
            .collect();

        self.members.truncate(MAX_MEMBERS);

        for member in &mut self.members {
            member.labels = tidy_labels(std::mem::take(&mut member.labels));
        }

        self
    }
}

/// 去掉保留的和空的键，最多留 [`MAX_ALERT_LABELS`] 个，值超长的截断。
fn tidy_labels(labels: BTreeMap<String, String>) -> BTreeMap<String, String> {
    labels
        .into_iter()
        .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
        .filter(|(key, _)| !key.is_empty() && !is_reserved_label(key))
        .take(MAX_ALERT_LABELS)
        .map(|(key, value)| (key, truncate(&value, super::MAX_LABEL_VALUE_CHARS)))
        .collect()
}

fn truncate(value: &str, max_chars: usize) -> String {
    match value.char_indices().nth(max_chars) {
        Some((end, _)) => value[..end].to_string(),
        None => value.to_string(),
    }
}

/// 各家写严重度的词换成 [`Severity`]，不区分大小写；认不出的返回 `None`，用来源的默认严重度。
pub fn severity_from_word(word: &str) -> Option<Severity> {
    match word.trim().to_ascii_lowercase().as_str() {
        "critical" | "crit" | "fatal" | "emergency" | "emerg" | "disaster" | "page" | "p1"
        | "sev1" | "sev-1" => Some(Severity::Critical),
        "error" | "err" | "major" | "high" | "p2" | "sev2" | "sev-2" => Some(Severity::Error),
        "warning" | "warn" | "minor" | "medium" | "average" | "p3" | "sev3" | "sev-3" => {
            Some(Severity::Warning)
        }
        "info" | "informational" | "information" | "notice" | "low" | "none" | "p4" | "p5"
        | "sev4" | "sev-4" | "sev5" | "sev-5" => Some(Severity::Info),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alert() -> ExternalAlert {
        ExternalAlert {
            action: ExternalAction::Trigger,
            dedup_key: "key".into(),
            summary: "  Disk full  ".into(),
            severity: None,
            labels: BTreeMap::new(),
            annotations: BTreeMap::new(),
            occurred_at: None,
            no_data: false,
            members: Vec::new(),
            priority: None,
            urgency: None,
        }
    }

    #[test]
    fn words_of_every_sender_map_to_a_severity() {
        assert_eq!(severity_from_word("CRITICAL"), Some(Severity::Critical));
        assert_eq!(severity_from_word("page"), Some(Severity::Critical));
        assert_eq!(severity_from_word("P2"), Some(Severity::Error));
        assert_eq!(severity_from_word("warn"), Some(Severity::Warning));
        assert_eq!(severity_from_word("low"), Some(Severity::Info));
        assert_eq!(severity_from_word("spicy"), None);
    }

    #[test]
    fn tidying_drops_reserved_labels_and_cuts_what_is_too_long() {
        let mut alert = alert();

        alert.labels = [
            ("alert.source".to_string(), "x".to_string()),
            ("external.kind".to_string(), "x".to_string()),
            ("__molesignal_incident_id".to_string(), "x".to_string()),
            (" ".to_string(), "x".to_string()),
            ("host".to_string(), " web-1 ".to_string()),
        ]
        .into();

        alert.annotations = [
            (
                "description".to_string(),
                "x".repeat(MAX_ANNOTATION_CHARS + 10),
            ),
            ("empty".to_string(), "  ".to_string()),
        ]
        .into();

        let tidy = alert.tidy();

        assert_eq!(tidy.summary, "Disk full");
        assert_eq!(
            tidy.labels,
            BTreeMap::from([("host".to_string(), "web-1".to_string())])
        );
        assert_eq!(
            tidy.annotations["description"].chars().count(),
            MAX_ANNOTATION_CHARS
        );
        assert!(!tidy.annotations.contains_key("empty"));
    }

    #[test]
    fn a_long_summary_is_cut_on_a_character_boundary() {
        let mut alert = alert();
        alert.summary = "磁".repeat(MAX_SUMMARY_CHARS + 1);

        assert_eq!(alert.tidy().summary.chars().count(), MAX_SUMMARY_CHARS);
    }
}
