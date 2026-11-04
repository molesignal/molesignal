// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 按这一轮算出的结果改事件上的抑制状态。

use super::{InhibitRule, resolve::Inhibited};
use crate::{
    domain::alerting::{
        handling::{IncidentHandling, Inhibition},
        incident::Incident,
    },
    shared::{ids::Id, time::TimestampMicros},
};

/// 抑制状态怎么变了。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InhibitionChange {
    /// 开始被压：时间线上记一条。
    Started,
    /// 还压着，但换了原因或开始倒计时：写回就行。
    Updated,
    /// 不压了，压着的通知该发了：`reason` 见时间线的 `inhibition_ended`。
    Ended { reason: &'static str },
    /// 等完了也没有源事件：这一步从现在起算。
    WaitOver,
}

/// 按这一轮算出的结果改事件上的抑制状态。`matched` 是 [`super::resolve()`] 给这个事件的结果，`rules` 是启用的规则，
/// `is_active` 说一个事件还在不在进行中。没有变化时返回 `None`。
pub fn settle(
    incident: &mut Incident,
    matched: Option<&Inhibited>,
    rules: &[InhibitRule],
    is_active: impl Fn(&Id) -> bool,
    now: TimestampMicros,
) -> Option<InhibitionChange> {
    let handling = &mut incident.handling;

    let change = match (handling.inhibition.clone(), matched) {
        (None, Some(matched)) => {
            handling.inhibition = Some(Inhibition {
                rule_id: matched.rule_id.clone(),
                rule_name: matched.rule_name.clone(),
                source_incident_id: matched.source_incident_id.clone(),
                source_summary: matched.source_summary.clone(),
                since: now,
                ending_at: None,
            });
            handling.inhibition_wait_until = None;
            Some(InhibitionChange::Started)
        }
        (Some(current), Some(matched)) => {
            let same = current.rule_id == matched.rule_id
                && current.source_incident_id == matched.source_incident_id
                && current.ending_at.is_none();

            (!same).then(|| {
                handling.inhibition = Some(Inhibition {
                    rule_id: matched.rule_id.clone(),
                    rule_name: matched.rule_name.clone(),
                    source_incident_id: matched.source_incident_id.clone(),
                    source_summary: matched.source_summary.clone(),
                    since: current.since,
                    ending_at: None,
                });
                InhibitionChange::Updated
            })
        }
        (Some(current), None) => release(handling, current, rules, is_active, now),
        (None, None) => wait_over(handling, now),
    };

    if matches!(
        change,
        Some(InhibitionChange::Ended { .. } | InhibitionChange::WaitOver)
    ) {
        // 通知才刚要发出去：这一步的确认超时从现在算，不然一放开就升到下一步。
        incident.current_step_started_at = now;
    }

    change
}

/// 这一轮没有谁再压它：规则设了 `linger_secs` 就先倒计时，到点才放开。
fn release(
    handling: &mut IncidentHandling,
    current: Inhibition,
    rules: &[InhibitRule],
    is_active: impl Fn(&Id) -> bool,
    now: TimestampMicros,
) -> Option<InhibitionChange> {
    let source_gone = !is_active(&current.source_incident_id);

    let linger = rules
        .iter()
        .find(|rule| source_gone && rule.id == current.rule_id)
        .map_or(0, |rule| rule.linger_secs);

    let ends_at = match current.ending_at {
        Some(at) => at,
        None if linger > 0 => {
            let at = TimestampMicros(now.0.saturating_add(i64::from(linger) * 1_000_000));

            handling.inhibition = Some(Inhibition {
                ending_at: Some(at),
                ..current
            });

            return Some(InhibitionChange::Updated);
        }
        None => now,
    };

    if ends_at.0 > now.0 {
        return None;
    }

    handling.inhibition = None;
    handling.inhibition_wait_until = None;

    Some(InhibitionChange::Ended {
        reason: if source_gone {
            "source_recovered"
        } else {
            "rule_changed"
        },
    })
}

fn wait_over(handling: &mut IncidentHandling, now: TimestampMicros) -> Option<InhibitionChange> {
    let until = handling.inhibition_wait_until?;

    if until.0 > now.0 {
        return None;
    }

    handling.inhibition_wait_until = None;
    Some(InhibitionChange::WaitOver)
}
