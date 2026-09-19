// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 计费域：试用（trial）状态机。
//!
//! 每个 org 在 SaaS 形态下进入一段试用期。状态由纯函数从 `ends_at` / `now` / 是否已转
//! 付费推导，后台 sweeper 把推导结果持久化并推进通知阶段；读路径也可直接 lazy 评估。

use serde::{Deserialize, Serialize};

use crate::shared::{ids::Id, time::TimestampMicros};

/// 一天的微秒数。
const DAY_MICROS: i64 = 86_400_000_000;

/// 默认试用时长（天）。
pub const DEFAULT_TRIAL_DAYS: i64 = 14;

/// 试用状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrialState {
    /// 试用进行中。
    Active,
    /// 试用到期、未转付费。
    Expired,
    /// 已转为有效订阅（试用终结的正向终态）。
    Converted,
}

impl TrialState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Expired => "expired",
            Self::Converted => "converted",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "expired" => Self::Expired,
            "converted" => Self::Converted,
            _ => Self::Active,
        }
    }
}

/// 到期通知阶段。`Ord` 保证单调推进（只前进、不回退）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotifyStage {
    None,
    /// 距到期 ≤7 天。
    ExpiringSoon,
    /// 距到期 ≤1 天。
    LastDay,
    /// 已到期。
    Expired,
}

impl NotifyStage {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ExpiringSoon => "expiring_soon",
            Self::LastDay => "last_day",
            Self::Expired => "expired",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "expiring_soon" => Self::ExpiringSoon,
            "last_day" => Self::LastDay,
            "expired" => Self::Expired,
            _ => Self::None,
        }
    }
}

/// 一个 org 的试用记录。
#[derive(Debug, Clone)]
pub struct Trial {
    pub org_id: Id,
    pub started_at: TimestampMicros,
    pub ends_at: TimestampMicros,
    pub state: TrialState,
    pub notified_stage: NotifyStage,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

/// 从起点推 `days` 天后的到期时刻。
pub fn trial_ends_after(start: TimestampMicros, days: i64) -> TimestampMicros {
    TimestampMicros(start.0 + days * DAY_MICROS)
}

/// 评估 trial 在 `now` 应处的状态。已有有效订阅 → [`TrialState::Converted`]；
/// 否则按是否过 `ends_at` 分 Active / Expired。
pub fn evaluate_trial_state(
    ends_at: TimestampMicros,
    now: TimestampMicros,
    has_active_subscription: bool,
) -> TrialState {
    if has_active_subscription {
        TrialState::Converted
    } else if now.0 >= ends_at.0 {
        TrialState::Expired
    } else {
        TrialState::Active
    }
}

/// 距到期剩余天数（向上取整）；已过期返回 0。
pub fn trial_days_remaining(ends_at: TimestampMicros, now: TimestampMicros) -> i64 {
    let remaining = ends_at.0 - now.0;
    if remaining <= 0 {
        0
    } else {
        (remaining + DAY_MICROS - 1) / DAY_MICROS
    }
}

/// 依据剩余时间应推进到的通知阶段。
pub fn trial_notify_stage(ends_at: TimestampMicros, now: TimestampMicros) -> NotifyStage {
    if now.0 >= ends_at.0 {
        return NotifyStage::Expired;
    }
    match trial_days_remaining(ends_at, now) {
        d if d <= 1 => NotifyStage::LastDay,
        d if d <= 7 => NotifyStage::ExpiringSoon,
        _ => NotifyStage::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(d: i64) -> TimestampMicros {
        TimestampMicros(d * DAY_MICROS)
    }

    #[test]
    fn state_active_until_end_then_expired() {
        let ends = ts(14);
        assert_eq!(
            evaluate_trial_state(ends, ts(13), false),
            TrialState::Active
        );
        assert_eq!(
            evaluate_trial_state(ends, ts(14), false),
            TrialState::Expired
        );
        assert_eq!(
            evaluate_trial_state(ends, ts(20), false),
            TrialState::Expired
        );
    }

    #[test]
    fn active_subscription_converts_regardless_of_time() {
        let ends = ts(14);
        assert_eq!(
            evaluate_trial_state(ends, ts(2), true),
            TrialState::Converted
        );
        assert_eq!(
            evaluate_trial_state(ends, ts(99), true),
            TrialState::Converted
        );
    }

    #[test]
    fn days_remaining_rounds_up_and_floors_at_zero() {
        let ends = ts(14);
        assert_eq!(trial_days_remaining(ends, ts(7)), 7);
        // 半天剩余 → 向上取整为 1。
        assert_eq!(
            trial_days_remaining(ends, TimestampMicros(14 * DAY_MICROS - DAY_MICROS / 2)),
            1
        );
        assert_eq!(trial_days_remaining(ends, ts(14)), 0);
        assert_eq!(trial_days_remaining(ends, ts(20)), 0);
    }

    #[test]
    fn notify_stage_progression() {
        let ends = ts(14);
        assert_eq!(trial_notify_stage(ends, ts(0)), NotifyStage::None); // 14d 左
        assert_eq!(trial_notify_stage(ends, ts(7)), NotifyStage::ExpiringSoon); // 7d
        assert_eq!(trial_notify_stage(ends, ts(13)), NotifyStage::LastDay); // 1d
        assert_eq!(trial_notify_stage(ends, ts(14)), NotifyStage::Expired);
        // Ord 单调：阶段只前进。
        assert!(NotifyStage::None < NotifyStage::ExpiringSoon);
        assert!(NotifyStage::ExpiringSoon < NotifyStage::LastDay);
        assert!(NotifyStage::LastDay < NotifyStage::Expired);
    }
}
