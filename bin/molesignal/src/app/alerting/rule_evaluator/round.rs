// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一轮读数怎样推进一组（或整条规则）的去抖状态：命中、恢复与没有值各数各的连续轮数。
//! 纯函数，状态的读写由调用方负责。
//!
//! - 命中：单档比 `trigger`，多档逐档去抖（`severity_streaks`），连续满 `for_periods` 轮触发。
//! - 恢复：值在恢复线健康的一侧（没有回差时就是没命中），连续满 `recovery.for_periods` 轮。
//! - 没有值：按 `no_data` 当作恢复、保持原状态（`keep`，`alert` 再数没有值的轮数），或当作 0。

use std::collections::BTreeMap;

use super::{decision::Decision, evaluation::compare_value};
use crate::{
    domain::alerting::{
        incident::Severity,
        repositories::AlertRuleEvalState,
        rule::{AlertRule, NoDataAction, recovery_operator},
    },
    shared::time::TimestampMicros,
};

/// 一轮读到的东西。没有值按 `zero` 处理的规则已经换成了 0（见 [`with_no_data_value`]）。
#[derive(Debug, Clone)]
pub(super) enum Reading {
    Missing,
    Value(f64),
    /// 异常规则：当前值偏离了基线没有。
    Anomaly {
        firing: bool,
        deviation_ratio: f64,
    },
    /// 组合、预测、离群、心跳规则：自己的算法判定了这一轮触发没有；`annotations` 写进这时开的事件。
    Verdict {
        firing: bool,
        annotations: BTreeMap<String, String>,
    },
}

/// 没有值按 `zero` 处理的规则把没有值换成 0，其余照旧。
pub(super) fn with_no_data_value(rule: &AlertRule, value: Option<f64>) -> Option<f64> {
    match value {
        None if rule.trigger.no_data_action(rule.kind) == NoDataAction::Zero => Some(0.0),
        value => value,
    }
}

/// 把一轮记进 `state`，得出这一轮的结论。
pub(super) fn advance(
    rule: &AlertRule,
    state: &mut AlertRuleEvalState,
    reading: &Reading,
    now: TimestampMicros,
) -> Decision {
    state.last_eval_at = now;

    let (matched, severity, deviation_ratio, healthy) = match reading {
        Reading::Missing => return missing(rule, state, now),
        Reading::Anomaly {
            firing,
            deviation_ratio,
        } => (*firing, None, Some(*deviation_ratio), !*firing),
        Reading::Verdict { firing, .. } => (*firing, None, None, !*firing),
        Reading::Value(value) if rule.thresholds.is_empty() => {
            let matched = compare_value(*value, &rule.trigger.operator, rule.trigger.threshold);
            (matched, None, None, healthy_side(rule, *value, matched))
        }
        Reading::Value(value) => {
            let (matched, fired) = tiers(rule, state, *value);
            (matched, fired, None, healthy_side(rule, *value, matched))
        }
    };

    let data_returned = state.consecutive_no_data > 0;
    state.consecutive_no_data = 0;
    state.no_data_since = None;
    state.last_matched = matched;

    state.consecutive_matches = if matched {
        state.consecutive_matches.saturating_add(1)
    } else {
        0
    };

    state.consecutive_clears = if healthy {
        state.consecutive_clears.saturating_add(1)
    } else {
        0
    };

    // 多档各档自己去抖；单档、异常与自己判定的规则看连续命中的轮数。
    let firing = if rule.thresholds.is_empty()
        || matches!(reading, Reading::Anomaly { .. } | Reading::Verdict { .. })
    {
        matched && state.consecutive_matches >= rule.trigger.for_periods
    } else {
        severity.is_some()
    };

    Decision {
        firing,
        severity,
        matched,
        recovered: healthy && state.consecutive_clears >= rule.trigger.recovery_periods(),
        data_returned,
        deviation_ratio,
        consecutive: state.consecutive_matches,
        annotations: match reading {
            Reading::Verdict { annotations, .. } => annotations.clone(),
            _ => BTreeMap::new(),
        },
        ..Decision::quiet()
    }
}

/// 没有值的一轮。
fn missing(rule: &AlertRule, state: &mut AlertRuleEvalState, now: TimestampMicros) -> Decision {
    if state.consecutive_no_data == 0 {
        state.no_data_since = Some(now);
    }

    state.consecutive_no_data = state.consecutive_no_data.saturating_add(1);

    let mut decision = Decision {
        no_data_since: state.no_data_since,
        ..Decision::quiet()
    };

    match rule.trigger.no_data_action(rule.kind) {
        // 当作不命中：开着的事件按恢复条件解决。`zero` 已在读数时换成 0，到不了这里。
        NoDataAction::Resolve | NoDataAction::Zero => {
            state.last_matched = false;
            state.consecutive_matches = 0;
            state
                .severity_streaks
                .values_mut()
                .for_each(|streak| *streak = 0);
            state.consecutive_clears = state.consecutive_clears.saturating_add(1);
            decision.recovered = state.consecutive_clears >= rule.trigger.recovery_periods();
        }
        // 保持原状态：命中与恢复都不往前数。
        action @ (NoDataAction::Keep | NoDataAction::Alert) => {
            decision.no_data = true;
            decision.consecutive = state.consecutive_matches;
            decision.no_data_firing = action == NoDataAction::Alert
                && state.consecutive_no_data >= rule.trigger.no_data_periods();
        }
    }

    decision
}

/// 多档：逐档比较、逐档去抖，返回有没有档命中，以及已触发的最高档。
fn tiers(rule: &AlertRule, state: &mut AlertRuleEvalState, value: f64) -> (bool, Option<Severity>) {
    let mut matched = false;
    let mut fired: Option<Severity> = None;

    for tier in &rule.thresholds {
        let streak = state
            .severity_streaks
            .entry(tier.severity.as_str().to_string())
            .or_insert(0);

        if compare_value(value, &tier.operator, tier.threshold) {
            matched = true;
            *streak = streak.saturating_add(1);

            if *streak >= tier.for_periods {
                fired = Some(fired.map_or(tier.severity, |fired| fired.max(tier.severity)));
            }
        } else {
            *streak = 0;
        }
    }

    (matched, fired)
}

/// 值是不是在恢复线健康的一侧；没有回差时就是没命中。
fn healthy_side(rule: &AlertRule, value: f64, matched: bool) -> bool {
    match rule.trigger.recovery_threshold() {
        Some(line) => {
            let operator = recovery_operator(&rule.trigger, &rule.thresholds);
            !compare_value(value, &operator, line)
        }
        None => !matched,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::{
        domain::{
            alerting::rule::{
                AlertQuery, AlertRuleKind, AlertTrigger, ComparisonOp, NoDataCondition,
                RecoveryCondition, RuleState, SeverityThreshold,
            },
            query::QueryLanguage,
        },
        shared::ids::Id,
    };

    fn rule(trigger: AlertTrigger, thresholds: Vec<SeverityThreshold>) -> AlertRule {
        AlertRule {
            id: Id::from_string("rule-1"),
            org_id: Id::from_string("org-1"),
            name: "cpu".into(),
            description: String::new(),
            enabled: true,
            kind: AlertRuleKind::Scheduled,
            query: AlertQuery {
                language: QueryLanguage::Promql,
                statement: "avg(cpu)".into(),
                period_secs: 60,
                stream: None,
                apm: None,
                rum: None,
                group_by: Vec::new(),
                composite: None,
                forecast: None,
                outlier: None,
                heartbeat: None,
                delay_secs: None,
                discovery: None,
                home_cluster: None,
            },
            anomaly_params: None,
            trigger,
            thresholds,
            severity: None,
            escalation_policy_id: Id::from_string("default"),
            labels: BTreeMap::new(),
            annotations: BTreeMap::new(),
            last_eval_at: None,
            last_state: RuleState::Healthy,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    fn above(threshold: f64, for_periods: u32) -> AlertTrigger {
        AlertTrigger {
            operator: ComparisonOp::Gt,
            threshold,
            for_periods,
            silence_secs: 0,
            no_data: None,
            recovery: None,
            priority: None,
            urgency: None,
        }
    }

    /// 依次喂入读数，返回每一轮的结论。
    fn run(rule: &AlertRule, readings: Vec<Reading>) -> Vec<Decision> {
        let mut state =
            AlertRuleEvalState::fresh(&rule.id, "", BTreeMap::new(), TimestampMicros(0));

        readings
            .iter()
            .enumerate()
            .map(|(round, reading)| {
                advance(rule, &mut state, reading, TimestampMicros(round as i64))
            })
            .collect()
    }

    fn values(values: &[f64]) -> Vec<Reading> {
        values.iter().copied().map(Reading::Value).collect()
    }

    #[test]
    fn an_open_incident_waits_for_the_recovery_line() {
        let mut trigger = above(90.0, 1);
        trigger.recovery = Some(RecoveryCondition {
            threshold: Some(80.0),
            for_periods: 0,
        });

        let decisions = run(&rule(trigger, Vec::new()), values(&[95.0, 85.0, 79.0]));

        assert!(decisions[0].firing);
        // Between the lines: neither firing nor recovered, so the incident stays as it is.
        assert!(!decisions[1].matched && !decisions[1].recovered);
        assert!(decisions[2].recovered);
    }

    #[test]
    fn recovery_needs_its_rounds_in_a_row() {
        let mut trigger = above(90.0, 1);
        trigger.recovery = Some(RecoveryCondition {
            threshold: None,
            for_periods: 2,
        });

        let decisions = run(
            &rule(trigger, Vec::new()),
            values(&[95.0, 50.0, 95.0, 50.0, 50.0]),
        );

        let recovered = decisions
            .iter()
            .map(|decision| decision.recovered)
            .collect::<Vec<_>>();
        assert_eq!(recovered, [false, false, false, false, true]);
    }

    #[test]
    fn a_missing_value_resolves_by_default() {
        let decisions = run(
            &rule(above(90.0, 1), Vec::new()),
            vec![Reading::Value(95.0), Reading::Missing],
        );

        assert!(decisions[1].recovered);
        assert!(!decisions[1].no_data);
        assert_eq!(decisions[1].rule_state(), RuleState::Healthy);
    }

    #[test]
    fn keep_holds_the_state_through_rounds_without_a_value() {
        let mut trigger = above(90.0, 3);
        trigger.no_data = Some(NoDataCondition {
            action: NoDataAction::Keep,
            for_periods: 0,
        });

        let readings = vec![
            Reading::Value(95.0),
            Reading::Value(95.0),
            Reading::Missing,
            Reading::Value(95.0),
        ];
        let decisions = run(&rule(trigger, Vec::new()), readings);

        assert!(decisions[2].no_data && !decisions[2].recovered && !decisions[2].no_data_firing);
        assert_eq!(decisions[2].rule_state(), RuleState::NoData);
        // The streak carries on where it stopped: the third match fires.
        assert!(decisions[3].firing);
        assert!(decisions[3].data_returned);
    }

    #[test]
    fn alert_counts_the_rounds_without_a_value() {
        let mut trigger = above(90.0, 1);
        trigger.no_data = Some(NoDataCondition {
            action: NoDataAction::Alert,
            for_periods: 2,
        });

        let readings = vec![
            Reading::Missing,
            Reading::Missing,
            Reading::Missing,
            Reading::Value(10.0),
        ];
        let decisions = run(&rule(trigger, Vec::new()), readings);

        let firing = decisions
            .iter()
            .map(|decision| decision.no_data_firing)
            .collect::<Vec<_>>();
        assert_eq!(firing, [false, true, true, false]);
        // The absence is dated from its first round.
        assert_eq!(decisions[2].no_data_since, Some(TimestampMicros(0)));
        assert!(decisions[3].data_returned);
    }

    #[test]
    fn zero_compares_a_missing_value_as_zero() {
        let mut trigger = AlertTrigger {
            operator: ComparisonOp::Lt,
            ..above(1.0, 1)
        };
        trigger.no_data = Some(NoDataCondition {
            action: NoDataAction::Zero,
            for_periods: 0,
        });
        let rule = rule(trigger, Vec::new());

        assert_eq!(with_no_data_value(&rule, None), Some(0.0));
        assert_eq!(with_no_data_value(&rule, Some(3.0)), Some(3.0));

        let decisions = run(
            &rule,
            vec![Reading::Value(with_no_data_value(&rule, None).unwrap())],
        );
        assert!(decisions[0].firing);
    }

    #[test]
    fn tiers_fire_on_their_own_and_recover_below_the_line() {
        let tier = |severity, threshold, for_periods| SeverityThreshold {
            severity,
            operator: ComparisonOp::Gte,
            threshold,
            for_periods,
        };
        let mut trigger = above(80.0, 1);
        trigger.recovery = Some(RecoveryCondition {
            threshold: Some(70.0),
            for_periods: 0,
        });
        let rule = rule(
            trigger,
            vec![
                tier(Severity::Warning, 80.0, 1),
                tier(Severity::Critical, 95.0, 2),
            ],
        );

        let decisions = run(&rule, values(&[96.0, 96.0, 75.0, 70.0, 69.0]));

        assert_eq!(decisions[0].severity, Some(Severity::Warning));
        assert_eq!(decisions[1].severity, Some(Severity::Critical));
        assert!(!decisions[2].recovered);
        // The line is compared with the lowest tier's `gte`: 70 is still at it, 69 is below.
        assert!(!decisions[3].recovered);
        assert!(decisions[4].recovered);
    }

    #[test]
    fn an_anomaly_rule_keeps_its_state_without_a_value_unless_told() {
        let mut rule = rule(above(0.0, 1), Vec::new());
        rule.kind = AlertRuleKind::Anomaly;

        let decisions = run(
            &rule,
            vec![
                Reading::Anomaly {
                    firing: true,
                    deviation_ratio: 4.0,
                },
                Reading::Missing,
            ],
        );

        assert!(decisions[0].firing);
        assert_eq!(decisions[0].deviation_ratio, Some(4.0));
        assert!(decisions[1].no_data && !decisions[1].recovered);
    }
}
