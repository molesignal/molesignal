// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 规则的条件：一轮没有值时怎么算、开着的事件怎样才算恢复，以及这两样和触发条件说得通。

use serde::{Deserialize, Serialize};

use super::rule::{AlertRuleKind, AlertTrigger, ComparisonOp, SeverityThreshold};
use crate::shared::{Error, Result};

impl AlertTrigger {
    /// `kind` 的规则没有值时实际怎么算。
    pub fn no_data_action(&self, kind: AlertRuleKind) -> NoDataAction {
        match &self.no_data {
            Some(no_data) => no_data.action,
            None if kind == AlertRuleKind::Anomaly => NoDataAction::Keep,
            None => NoDataAction::Resolve,
        }
    }

    /// 连续多少轮没有值才开无数据事件。
    pub fn no_data_periods(&self) -> u32 {
        self.no_data
            .as_ref()
            .map_or(1, |no_data| no_data.for_periods.max(1))
    }

    /// 连续多少轮在恢复线健康的一侧才恢复。
    pub fn recovery_periods(&self) -> u32 {
        self.recovery
            .as_ref()
            .map_or(1, |recovery| recovery.for_periods.max(1))
    }

    pub fn recovery_threshold(&self) -> Option<f64> {
        self.recovery
            .as_ref()
            .and_then(|recovery| recovery.threshold)
    }
}

/// 一轮没有值时（查不到、首格不是数值，或按列拆分时某一组不在结果里）怎么算。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoDataAction {
    /// 当作恢复：算一轮不命中，开着的事件按恢复条件解决。
    Resolve,
    /// 保持原状态：这一轮不算命中，也不算恢复。
    Keep,
    /// 保持原状态；连续 `for_periods` 轮没有值时另开一个无数据事件，有值后它自动解决。
    Alert,
    /// 当作 0 参与比较。
    Zero,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoDataCondition {
    pub action: NoDataAction,
    /// `alert` 时连续多少轮没有值才开事件；0 与 1 都是一轮。
    #[serde(default)]
    pub for_periods: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryCondition {
    /// 回差：开着的事件要等值越过这条线、回到健康的一侧才恢复；不填就是触发阈值本身。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f64>,
    /// 连续多少轮在健康的一侧才恢复；0 与 1 都是一轮。
    #[serde(default)]
    pub for_periods: u32,
}

/// 没有值与恢复条件最多数多少轮。
pub const MAX_CONDITION_PERIODS: u32 = 1440;

/// 没有值时的处理与恢复条件说得通：轮数有上限，实时规则不带它们，回差线在健康的一侧。
pub fn validate_conditions(
    kind: AlertRuleKind,
    trigger: &AlertTrigger,
    thresholds: &[SeverityThreshold],
) -> Result<()> {
    if kind.judges_itself() && kind != AlertRuleKind::Anomaly && !thresholds.is_empty() {
        return Err(Error::invalid(format!(
            "a {} rule fires at one severity: set severity instead of thresholds",
            kind.as_str()
        )));
    }

    if kind.without_query() && trigger.no_data.is_some() {
        return Err(Error::invalid(format!(
            "a {} rule always has a value and takes no no_data condition",
            kind.as_str()
        )));
    }

    if kind == AlertRuleKind::RealTime && (trigger.no_data.is_some() || trigger.recovery.is_some())
    {
        return Err(Error::invalid(
            "a real-time rule resolves as soon as its count drops and takes no no_data or \
             recovery condition",
        ));
    }

    let periods = [
        trigger.no_data.as_ref().map(|no_data| no_data.for_periods),
        trigger
            .recovery
            .as_ref()
            .map(|recovery| recovery.for_periods),
    ];

    if periods
        .into_iter()
        .flatten()
        .any(|periods| periods > MAX_CONDITION_PERIODS)
    {
        return Err(Error::invalid(format!(
            "no_data.for_periods and recovery.for_periods are at most {MAX_CONDITION_PERIODS}"
        )));
    }

    let Some(line) = trigger.recovery_threshold() else {
        return Ok(());
    };

    if !line.is_finite() {
        return Err(Error::invalid("recovery.threshold must be a number"));
    }

    if kind.judges_itself() {
        return Err(Error::invalid(format!(
            "a {} rule has no threshold to recover past",
            kind.as_str()
        )));
    }

    let bands = fire_bands(trigger, thresholds);
    let above = bands
        .iter()
        .all(|(operator, _)| matches!(operator, ComparisonOp::Gt | ComparisonOp::Gte));
    let below = bands
        .iter()
        .all(|(operator, _)| matches!(operator, ComparisonOp::Lt | ComparisonOp::Lte));

    let healthy = if above {
        bands.iter().all(|(_, threshold)| line <= *threshold)
    } else if below {
        bands.iter().all(|(_, threshold)| line >= *threshold)
    } else {
        return Err(Error::invalid(
            "a recovery threshold needs every threshold to fire on one side: above (gt, gte) \
             or below (lt, lte)",
        ));
    };

    if !healthy {
        return Err(Error::invalid(
            "recovery.threshold has to be on the healthy side of every threshold: at or below \
             one the value fires above, at or above one it fires below",
        ));
    }

    Ok(())
}

/// 规则触发的各档（运算符、阈值）：有多档时是各档，否则是 `trigger` 本身。
fn fire_bands(
    trigger: &AlertTrigger,
    thresholds: &[SeverityThreshold],
) -> Vec<(ComparisonOp, f64)> {
    if thresholds.is_empty() {
        return vec![(trigger.operator, trigger.threshold)];
    }

    thresholds
        .iter()
        .map(|tier| (tier.operator, tier.threshold))
        .collect()
}

/// 跟恢复线比较时用的运算符：最先触发、也最晚恢复的那一档的。
pub fn recovery_operator(trigger: &AlertTrigger, thresholds: &[SeverityThreshold]) -> ComparisonOp {
    let bands = fire_bands(trigger, thresholds);
    let above = matches!(bands[0].0, ComparisonOp::Gt | ComparisonOp::Gte);

    bands
        .into_iter()
        .min_by(|left, right| {
            let order = left.1.total_cmp(&right.1);
            if above { order } else { order.reverse() }
        })
        .map_or(trigger.operator, |(operator, _)| operator)
}
