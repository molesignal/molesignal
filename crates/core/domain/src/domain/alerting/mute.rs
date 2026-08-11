// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 告警屏蔽（mute）：matchers 全命中且时间窗 active 时，dispatcher 暂停对该 incident 的
//! 派发与升级推进（incident 仍照常创建/记录，仅静音通知）。窗口结束后若仍 firing，恢复派发。

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{schedule::ActiveWindow, semantic_group::LabelMatcher};
use crate::shared::{Result, ids::Id, time::TimestampMicros};

/// Reserved matcher label injected only while evaluating mute rules. It lets
/// the API create a silence that targets exactly one incident without
/// persisting implementation labels into the incident itself.
pub const INCIDENT_ID_MATCHER_LABEL: &str = "__molesignal_incident_id";

/// Build the label view used by the dispatcher when evaluating mute rules.
/// User labels cannot spoof the reserved incident id because it is always
/// overwritten here.
pub fn match_labels_for_incident(
    labels: &BTreeMap<String, String>,
    incident_id: &Id,
) -> BTreeMap<String, String> {
    let mut output = labels.clone();
    output.insert(INCIDENT_ID_MATCHER_LABEL.to_string(), incident_id.0.clone());
    output
}

/// 屏蔽时间窗：一次性固定窗或周期性窗（周期窗复用 [`ActiveWindow`] 的 weekday/hour 判定）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MuteWindow {
    /// 一次性维护窗 `[start, end)`。
    Fixed {
        start: TimestampMicros,
        end: TimestampMicros,
    },
    /// 周期性窗口（如每周工作日 22:00–06:00），按 `timezone` 本地时间判定。
    Recurring {
        timezone: String,
        /// bit0=周日 … bit6=周六。
        weekday_mask: u8,
        hour_start: u8,
        hour_end: u8,
    },
}

impl MuteWindow {
    /// `now` 是否落在窗口内。
    pub fn active(&self, now: TimestampMicros) -> bool {
        match self {
            MuteWindow::Fixed { start, end } => now.0 >= start.0 && now.0 < end.0,
            MuteWindow::Recurring {
                timezone,
                weekday_mask,
                hour_start,
                hour_end,
            } => {
                let tz = timezone.parse::<chrono_tz::Tz>().unwrap_or(chrono_tz::UTC);
                let w = ActiveWindow {
                    weekday_mask: *weekday_mask,
                    hour_start: *hour_start,
                    hour_end: *hour_end,
                };
                w.contains(now.to_datetime(), tz)
            }
        }
    }
}

/// 告警屏蔽规则：`matchers` 全命中且 `window` active 时屏蔽匹配的 incident 通知。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MuteRule {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub enabled: bool,
    /// 全部命中才屏蔽；空 = catch-all（任意 incident）。
    #[serde(default)]
    pub matchers: Vec<LabelMatcher>,
    pub window: MuteWindow,
    #[serde(default)]
    pub comment: String,
    #[serde(default)]
    pub created_by: Option<Id>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

impl MuteRule {
    /// 给定 incident 标签与当前时间，本规则是否正在屏蔽。
    pub fn is_muting(&self, labels: &BTreeMap<String, String>, now: TimestampMicros) -> bool {
        self.enabled && self.window.active(now) && self.matchers.iter().all(|m| m.matches(labels))
    }
}

#[async_trait]
pub trait MuteRuleRepository: Send + Sync {
    async fn create(&self, rule: MuteRule) -> Result<MuteRule>;
    async fn update(&self, rule: MuteRule) -> Result<MuteRule>;
    async fn get(&self, id: &Id) -> Result<MuteRule>;
    async fn list(&self, org_id: &Id) -> Result<Vec<MuteRule>>;
    async fn delete(&self, id: &Id) -> Result<()>;
    /// dispatcher 用：取该 org 所有 enabled 规则。
    async fn list_enabled(&self, org_id: &Id) -> Result<Vec<MuteRule>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn rule(matchers: Vec<LabelMatcher>, window: MuteWindow) -> MuteRule {
        MuteRule {
            id: Id::from_string("m1"),
            org_id: Id::from_string("o1"),
            name: "m".into(),
            enabled: true,
            matchers,
            window,
            comment: String::new(),
            created_by: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    #[test]
    fn fixed_window_mutes_inside_only() {
        use super::super::semantic_group::MatchOp;
        let r = rule(
            vec![LabelMatcher {
                label: "service".into(),
                op: MatchOp::Eq,
                value: "api".into(),
            }],
            MuteWindow::Fixed {
                start: TimestampMicros(1000),
                end: TimestampMicros(2000),
            },
        );
        // 命中 matcher + 在窗口内 → 屏蔽
        assert!(r.is_muting(&labels(&[("service", "api")]), TimestampMicros(1500)));
        // 窗口外 → 不屏蔽
        assert!(!r.is_muting(&labels(&[("service", "api")]), TimestampMicros(2500)));
        // matcher 不命中 → 不屏蔽
        assert!(!r.is_muting(&labels(&[("service", "web")]), TimestampMicros(1500)));
    }

    #[test]
    fn disabled_rule_never_mutes() {
        let mut r = rule(
            vec![],
            MuteWindow::Fixed {
                start: TimestampMicros(0),
                end: TimestampMicros(i64::MAX),
            },
        );
        r.enabled = false;
        assert!(!r.is_muting(&labels(&[]), TimestampMicros(100)));
    }

    #[test]
    fn incident_identity_matcher_targets_only_one_incident() {
        use super::super::semantic_group::MatchOp;

        let r = rule(
            vec![LabelMatcher {
                label: INCIDENT_ID_MATCHER_LABEL.into(),
                op: MatchOp::Eq,
                value: "incident-a".into(),
            }],
            MuteWindow::Fixed {
                start: TimestampMicros(0),
                end: TimestampMicros(2_000),
            },
        );
        let shared = labels(&[("service", "checkout")]);
        let incident_a = match_labels_for_incident(&shared, &Id::from_string("incident-a"));
        let incident_b = match_labels_for_incident(&shared, &Id::from_string("incident-b"));

        assert!(r.is_muting(&incident_a, TimestampMicros(1_000)));
        assert!(!r.is_muting(&incident_b, TimestampMicros(1_000)));
    }
}
