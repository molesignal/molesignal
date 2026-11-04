// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 人对事件的处理：暂缓、手动指定的负责人与严重度、合并，以及升级通知的轮次；还有事件的紧急度和
//! 优先级（开事件时定下，之后可以手动改），它们和这些处理一起存在 `incidents.handling` 里。

use serde::{Deserialize, Serialize};

use super::{priority::Priority, urgency::Urgency};
use crate::shared::{ids::Id, time::TimestampMicros};

/// 一次最多暂缓多久：7 天。
pub const MAX_SNOOZE_SECS: u32 = 7 * 24 * 60 * 60;

/// 一次最多暂缓多短：1 分钟。
pub const MIN_SNOOZE_SECS: u32 = 60;

/// 一次最多合并几个事件。
pub const MAX_MERGE_SOURCES: usize = 50;

/// 为什么通知：升级通知的 `escalation.reason`。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EscalationReason {
    /// 照升级策略：事件刚开时的第一步，或上一步超时没人确认。
    #[default]
    Step,
    /// 有人把事件立即升级到这一步。
    Manual,
    /// 有人把事件转派了：转给团队或升级策略时从第一步重新开始，转给成员时通知被选中的人。
    Reassigned,
    /// 有人增派了成员。
    ResponderAdded,
    /// 暂缓到点，事件退回未确认。
    SnoozeEnded,
    /// 确认后一直没解决，到了升级策略设的时间，事件退回未确认。
    AckTimeout,
    /// 严重度调高了，按新的严重度从第一步重新开始。
    SeverityRaised,
    /// 紧急度从低升到高（有人手动调高，或支持时段开始），从第一步重新开始。
    UrgencyRaised,
}

impl EscalationReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Step => "step",
            Self::Manual => "manual",
            Self::Reassigned => "reassigned",
            Self::ResponderAdded => "responder_added",
            Self::SnoozeEnded => "snooze_ended",
            Self::AckTimeout => "ack_timeout",
            Self::SeverityRaised => "severity_raised",
            Self::UrgencyRaised => "urgency_raised",
        }
    }
}

/// 事件正被抑制（见 [`super::inhibit`]）：哪条规则、因为哪个事件。规则和源事件的名字一起存下，规则删了、
/// 源事件解决了之后事件上还看得懂。事件解决后留在事件上，说明它当时没有通知。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Inhibition {
    pub rule_id: Id,
    pub rule_name: String,
    pub source_incident_id: Id,
    pub source_summary: String,
    pub since: TimestampMicros,
    /// 源事件没了，还压到这一刻（规则的 `linger_secs`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ending_at: Option<TimestampMicros>,
}

/// 事件上和人的处理有关的状态。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncidentHandling {
    /// 暂缓到这一刻：到点退回未确认，重新通知负责人。
    #[serde(default)]
    pub snoozed_until: Option<TimestampMicros>,
    #[serde(default)]
    pub snoozed_by: Option<Id>,
    /// 负责人是有人手动指定的：在这一步里，调度器不再把负责人对齐到值班人，升到下一步才换。
    #[serde(default)]
    pub assignees_pinned: bool,
    /// 有人手动改过严重度：评估器不再改它。
    #[serde(default)]
    pub severity_set_by: Option<Id>,
    /// 并入了哪个事件。并入的事件照旧算进行中（它的规则或来源不会再开新事件），但不再升级、
    /// 不再通知，主事件解决时一并解决。
    #[serde(default)]
    pub merged_into: Option<Id>,
    /// 升级通知的轮次：手动升级、转派给团队或升级策略、调高严重度时换一轮，同一步可以再通知一次。
    #[serde(default)]
    pub dispatch_round: u32,
    /// 这一轮通知当前这一步的原因；照常升级到下一步时回到 [`EscalationReason::Step`]。
    #[serde(default)]
    pub dispatch_reason: EscalationReason,
    /// 紧急度：开事件时定下（见 [`super::urgency`]）；空着的是这个字段出现以前开的事件，按高算。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub urgency: Option<Urgency>,
    /// 有人手动改过紧急度：支持时段不再改它。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub urgency_set_by: Option<Id>,
    /// 紧急度是按升级策略的规则定的（不是规则、来源、流水线或人明确指定的）：只有这样的低紧急度
    /// 事件会在支持时段开始时升为高。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub urgency_by_policy: bool,
    /// 优先级，可以不设。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    /// 正被抑制：通知压着，升级不往下走。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inhibition: Option<Inhibition>,
    /// 符合抑制规则的目标条件：通知先等到这一刻，看源事件会不会来。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inhibition_wait_until: Option<TimestampMicros>,
    /// 有人说了照常通知：这个事件不再被抑制。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inhibition_lifted_by: Option<Id>,
}

impl IncidentHandling {
    /// 通知此刻被抑制压着：正被抑制，或还在等可能来的源事件。
    pub fn inhibition_holds(&self, now: TimestampMicros) -> bool {
        self.inhibition.is_some()
            || self
                .inhibition_wait_until
                .is_some_and(|until| until.0 > now.0)
    }

    /// 有人说了照常通知：清掉抑制，之后也不再被抑制。原来就没压着时返回 `false`。
    pub fn lift_inhibition(&mut self, by: &Id) -> bool {
        let held = self.inhibition.is_some() || self.inhibition_wait_until.is_some();

        self.inhibition = None;
        self.inhibition_wait_until = None;
        self.inhibition_lifted_by = Some(by.clone());
        held
    }

    /// 换一轮通知，从 `reason` 说起。
    pub fn next_round(&mut self, reason: EscalationReason) {
        self.dispatch_round = self.dispatch_round.saturating_add(1);
        self.dispatch_reason = reason;
    }

    /// 暂缓到点了没有。
    pub fn snooze_due(&self, now: TimestampMicros) -> bool {
        self.snoozed_until.is_some_and(|until| until.0 <= now.0)
    }

    /// 去掉暂缓。
    pub fn clear_snooze(&mut self) {
        self.snoozed_until = None;
        self.snoozed_by = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_round_counts_up_and_keeps_its_reason() {
        let mut handling = IncidentHandling::default();
        assert_eq!(handling.dispatch_reason, EscalationReason::Step);

        handling.next_round(EscalationReason::Manual);
        handling.next_round(EscalationReason::Reassigned);
        assert_eq!(handling.dispatch_round, 2);
        assert_eq!(handling.dispatch_reason.as_str(), "reassigned");
    }

    #[test]
    fn a_snooze_is_due_at_its_end() {
        let mut handling = IncidentHandling {
            snoozed_until: Some(TimestampMicros(100)),
            snoozed_by: Some(Id::from_string("u1")),
            ..IncidentHandling::default()
        };

        assert!(!handling.snooze_due(TimestampMicros(99)));
        assert!(handling.snooze_due(TimestampMicros(100)));

        handling.clear_snooze();
        assert!(!handling.snooze_due(TimestampMicros(200)));
        assert!(handling.snoozed_by.is_none());
    }

    #[test]
    fn an_old_row_without_handling_reads_as_the_default() {
        let handling: IncidentHandling = serde_json::from_str("{}").expect("empty handling");
        assert_eq!(handling, IncidentHandling::default());

        let reason: EscalationReason =
            serde_json::from_str("\"snooze_ended\"").expect("reason word");
        assert_eq!(reason, EscalationReason::SnoozeEnded);
    }
}
