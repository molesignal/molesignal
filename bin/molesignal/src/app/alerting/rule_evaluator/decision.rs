// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 把一轮查询结果判定成 [`Decision`]：读出值，交给 [`advance`] 推进去抖状态并写回。

use std::collections::BTreeMap;

use super::{
    DAY_MICROS, RuleEvaluator,
    evaluation::first_cell_f64,
    round::{Reading, advance, with_no_data_value},
};
use crate::{
    domain::{
        alerting::{
            anomaly::{AnomalyDecision, MAX_ANOMALY_LOOKBACK_DAYS, detector_for},
            incident::Severity,
            repositories::AlertRuleEvalState,
            rule::{AlertRule, AlertRuleKind, RuleState},
        },
        query::{QueryResult, StreamHint},
    },
    shared::{Result, time::TimestampMicros},
};

/// 一轮评估的结论。
#[derive(Debug, Clone)]
pub(super) struct Decision {
    /// 命中，且已连续命中满 `for_periods`。
    pub(super) firing: bool,
    /// 多档规则里已触发的最高档；单档规则与 anomaly 为 `None`。
    pub(super) severity: Option<Severity>,
    /// 本轮命中（含尚未满 `for_periods` 的 pending）。
    pub(super) matched: bool,
    /// 本轮在恢复线健康的一侧，且已连续满恢复条件的轮数：开着的事件该解决了。
    pub(super) recovered: bool,
    /// 本轮没有值，规则保持原状态（`no_data` 为 `keep` 或 `alert`）。
    pub(super) no_data: bool,
    /// 没有值已连续满 `no_data.for_periods` 轮，规则要为此开无数据事件。
    pub(super) no_data_firing: bool,
    /// 这一段没有值从哪一轮开始。
    pub(super) no_data_since: Option<TimestampMicros>,
    /// 前几轮没有值，这一轮有了：无数据事件该解决了。
    pub(super) data_returned: bool,
    /// 仅 anomaly：偏离基线的比例。
    pub(super) deviation_ratio: Option<f64>,
    /// 已连续命中的轮数。
    pub(super) consecutive: u32,
    /// 自己判定的规则写进这时开的事件的说明（预计越线时间、偏离程度、最后一次打卡……）。
    pub(super) annotations: BTreeMap<String, String>,
}

impl Decision {
    /// 什么都没发生的一轮。
    pub(super) fn quiet() -> Self {
        Self {
            firing: false,
            severity: None,
            matched: false,
            recovered: false,
            no_data: false,
            no_data_firing: false,
            no_data_since: None,
            data_returned: false,
            deviation_ratio: None,
            consecutive: 0,
            annotations: BTreeMap::new(),
        }
    }

    /// 按列拆分的规则：把一组的结论并进整条规则的结论（有组在触发就是触发，连续轮数取命中各组里
    /// 最长的）。
    pub(super) fn absorb(&mut self, group: &Decision) {
        self.firing |= group.firing;
        self.no_data |= group.no_data;

        if group.matched {
            self.matched = true;
            self.consecutive = self.consecutive.max(group.consecutive);
        }
    }

    /// 写到 `alert_rules.last_state` 的规则状态。
    pub(super) fn rule_state(&self) -> RuleState {
        if self.firing {
            RuleState::Firing
        } else if self.no_data {
            RuleState::NoData
        } else if self.matched {
            RuleState::Pending(self.consecutive)
        } else {
            RuleState::Healthy
        }
    }
}

impl RuleEvaluator {
    /// 整条规则（不按列拆分）的一轮：读出值，推进状态并写回。`None` 表示本轮 inconclusive
    /// （anomaly 基线不可信），调用方跳过本轮，不动状态，绝不能把"评估不了"当成"恢复正常"。
    /// 没有值不算 inconclusive，按规则的 `no_data` 处理。
    pub(super) async fn decide(
        &self,
        rule: &AlertRule,
        stream: &StreamHint,
        query_result: &QueryResult,
        now: TimestampMicros,
        period_us: i64,
    ) -> Result<Option<Decision>> {
        let value = with_no_data_value(
            rule,
            first_cell_f64(query_result).filter(|value| !value.is_nan()),
        );

        let reading = match value {
            None => Reading::Missing,
            Some(current) if rule.kind == AlertRuleKind::Anomaly => {
                let Some(decision) = self
                    .eval_anomaly(rule, stream, current, now, period_us)
                    .await
                else {
                    return Ok(None);
                };

                Reading::Anomaly {
                    firing: decision.firing,
                    deviation_ratio: decision.deviation_ratio(),
                }
            }
            Some(value) => Reading::Value(value),
        };

        let mut state = self
            .eval_state
            .get(&rule.id, "")
            .await?
            .unwrap_or_else(|| AlertRuleEvalState::fresh(&rule.id, "", BTreeMap::new(), now));

        let decision = advance(rule, &mut state, &reading, now);
        self.eval_state.upsert_state(state).await?;
        Ok(Some(decision))
    }

    /// Anomaly 求值：取 `lookback_days` 个同时刻历史窗口的首格值作基线，
    /// 跑 [`detector_for`] 选出的 detector 判定当前值是否离群。
    ///
    /// 返回 `Some(firing)`（接入与 scheduled 相同的去抖 / incident 生命周期），或
    /// `None` 表示 **inconclusive**——基线不可信（历史窗口抓取失败、缺 params、或一个
    /// 有效历史样本都没有）。inconclusive 时 caller 跳过本轮且不动状态，避免把"评估不了"
    /// 误判成"恢复正常"而 resolve 掉真在 firing 的 incident。当前窗口没有值由调用方按
    /// `no_data` 处理，不走到这里。
    ///
    /// 代价：每个 tick 会按 `lookback_days` 各跑一次历史窗口查询（route 与此处都按
    /// `MAX_ANOMALY_LOOKBACK_DAYS` 收口；evaluator 再 clamp 一次，不信任持久化越界值），
    /// 后续可换成单条按天分桶的 SQL / 基线缓存优化（follow-up）。
    pub(super) async fn eval_anomaly(
        &self,
        rule: &AlertRule,
        stream: &StreamHint,
        current: f64,
        now: TimestampMicros,
        period_us: i64,
    ) -> Option<AnomalyDecision> {
        let Some(params) = rule.anomaly_params.as_ref() else {
            tracing::warn!(
                rule_id = %rule.id,
                "anomaly rule missing anomaly_params; tick skipped (inconclusive)"
            );
            return None;
        };

        // evaluator 不信任持久化里的 lookback_days（route 收口前写入 / 旁路写入都可能越界）。
        let lookback = params.lookback_days.clamp(1, MAX_ANOMALY_LOOKBACK_DAYS) as i64;
        let mut history: Vec<f64> = Vec::with_capacity(lookback as usize);
        let mut fetch_failures = 0u32;
        // 周季节性 opt-in：仅保留与当前「同星期几」的同时刻历史点（d 为 7 的倍数），
        // 消解工作日/周末模式差异。默认逐日（step=1，`d % 1 == 0` 恒真，行为不变）。
        let step_days = if params.weekly_seasonality { 7 } else { 1 };

        // 按时间升序（最早 → 最近）遍历，满足 AnomalyDetector::detect 的入参契约：
        // MAD 与顺序无关，但 EWMA 依赖时序。d=lookback 是最早一天，d=1 是昨天。
        for d in (1..=lookback).rev() {
            if d % step_days != 0 {
                continue;
            }

            let center = now.0 - d * DAY_MICROS;
            let from = TimestampMicros(center - period_us);
            let to = TimestampMicros(center);

            match self.run_window(rule, stream, from, to).await {
                // Some(q) 但无数值 = 那天确实没数据（空桶），不算失败，只是少一个样本。
                Some(q) => {
                    if let Some(v) = first_cell_f64(&q) {
                        history.push(v);
                    }
                }
                // None = 该历史窗口超时 / 查询出错（transient）。
                None => fetch_failures += 1,
            }
        }

        if fetch_failures > 0 || history.is_empty() {
            tracing::warn!(
                rule_id = %rule.id,
                fetch_failures,
                samples = history.len(),
                "anomaly baseline incomplete; tick skipped (inconclusive)"
            );
            return None;
        }

        let decision = detector_for(params).detect(current, &history);

        if decision.firing {
            tracing::info!(
                rule_id = %rule.id,
                current = decision.current,
                baseline = decision.baseline,
                deviation = decision.deviation,
                score = decision.score,
                reason = %decision.reason,
                "anomaly detector fired"
            );
        }

        Some(decision)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decision(firing: bool, matched: bool, consecutive: u32) -> Decision {
        Decision {
            firing,
            matched,
            consecutive,
            ..Decision::quiet()
        }
    }

    #[test]
    fn rule_state_distinguishes_healthy_pending_no_data_and_firing() {
        assert_eq!(decision(false, false, 0).rule_state(), RuleState::Healthy);
        assert_eq!(decision(false, true, 1).rule_state(), RuleState::Pending(1));
        assert_eq!(decision(true, true, 2).rule_state(), RuleState::Firing);

        let kept = Decision {
            no_data: true,
            ..decision(false, false, 1)
        };
        assert_eq!(kept.rule_state(), RuleState::NoData);
    }

    #[test]
    fn a_split_rule_fires_while_any_group_fires_and_counts_its_longest_streak() {
        let mut rule = Decision::quiet();
        rule.absorb(&decision(false, true, 2));
        rule.absorb(&decision(false, true, 4));
        assert_eq!(rule.rule_state(), RuleState::Pending(4));

        rule.absorb(&Decision {
            no_data: true,
            ..Decision::quiet()
        });
        assert_eq!(rule.rule_state(), RuleState::NoData);

        rule.absorb(&decision(true, true, 1));
        assert_eq!(rule.rule_state(), RuleState::Firing);
    }
}
