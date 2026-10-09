// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 把一轮查询结果判定成 [`Decision`]：阈值（单档 / 多档）与 anomaly 两条路径。

use std::collections::BTreeMap;

use super::{
    DAY_MICROS, RuleEvaluator,
    evaluation::{compare_value, first_cell_f64, first_cell_matches},
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
pub(super) struct Decision {
    /// 命中，且已连续命中满 `for_periods`。
    pub(super) firing: bool,
    /// 多档规则里已触发的最高档；单档规则与 anomaly 为 `None`。
    pub(super) severity: Option<Severity>,
    /// 本轮命中（含尚未满 `for_periods` 的 pending），resolve 以它为准。
    pub(super) matched: bool,
    /// 仅 anomaly：偏离基线的比例。
    pub(super) deviation_ratio: Option<f64>,
    /// 已连续命中的轮数。
    pub(super) consecutive: u32,
}

impl Decision {
    /// 写到 `alert_rules.last_state` 的规则状态。
    pub(super) fn rule_state(&self) -> RuleState {
        if self.firing {
            RuleState::Firing
        } else if self.matched {
            RuleState::Pending(self.consecutive)
        } else {
            RuleState::Healthy
        }
    }
}

impl RuleEvaluator {
    /// 判定一轮结果并推进去抖状态。`None` 表示本轮 inconclusive（anomaly 基线不可信），
    /// 调用方跳过本轮，不动状态，绝不能把"评估不了"当成"恢复正常"。
    pub(super) async fn decide(
        &self,
        rule: &AlertRule,
        stream: &StreamHint,
        query_result: &QueryResult,
        now: TimestampMicros,
        period_us: i64,
    ) -> Result<Option<Decision>> {
        // 多档与单档 / anomaly 互斥。
        if !rule.thresholds.is_empty() && rule.kind != AlertRuleKind::Anomaly {
            return self
                .eval_thresholds(rule, query_result, now)
                .await
                .map(Some);
        }
        let (matched, deviation_ratio) = match rule.kind {
            AlertRuleKind::Anomaly => {
                let Some(decision) = self
                    .eval_anomaly(rule, stream, query_result, now, period_us)
                    .await
                else {
                    return Ok(None);
                };
                (decision.firing, Some(decision.deviation_ratio()))
            }
            // 写入路径匹配器不支持的 real_time 规则按 scheduled 周期评估。
            AlertRuleKind::Scheduled | AlertRuleKind::RealTime => (
                first_cell_matches(query_result, &rule.trigger.operator, rule.trigger.threshold),
                None,
            ),
        };
        let state = self.eval_state.upsert_match(&rule.id, matched, now).await?;
        Ok(Some(Decision {
            firing: matched && state.consecutive_matches >= rule.trigger.for_periods,
            severity: None,
            matched,
            deviation_ratio,
            consecutive: state.consecutive_matches,
        }))
    }

    /// 多档阈值评估：逐档比较首格值、每档独立去抖（severity_streaks），取最高命中档。
    /// 读-改-写要求同一周期只有一个节点评估这条规则：多个 alert 节点按 `rule_id` 分担，
    /// 交接时的重叠由周期守卫（`claim_cycle`，见 `RuleEvaluator::eval_one`）挡住。
    async fn eval_thresholds(
        &self,
        rule: &AlertRule,
        query_result: &QueryResult,
        now: TimestampMicros,
    ) -> Result<Decision> {
        let mut state =
            self.eval_state
                .get(&rule.id)
                .await?
                .unwrap_or_else(|| AlertRuleEvalState {
                    rule_id: rule.id.clone(),
                    consecutive_matches: 0,
                    last_eval_at: now,
                    last_matched: false,
                    severity_streaks: BTreeMap::new(),
                });
        let value = first_cell_f64(query_result).filter(|v| !v.is_nan());
        let mut matched_any = false;
        let mut fired: Option<Severity> = None;
        for tier in &rule.thresholds {
            let key = tier.severity.as_str().to_string();
            let hit = value
                .map(|v| compare_value(v, &tier.operator, tier.threshold))
                .unwrap_or(false);
            if hit {
                matched_any = true;
                let c = state.severity_streaks.entry(key).or_insert(0);
                *c += 1;
                if *c >= tier.for_periods {
                    fired = Some(fired.map_or(tier.severity, |f| f.max(tier.severity)));
                }
            } else {
                state.severity_streaks.insert(key, 0);
            }
        }
        state.last_matched = matched_any;
        state.last_eval_at = now;
        state.consecutive_matches = if matched_any {
            state.consecutive_matches.saturating_add(1)
        } else {
            0
        };
        let consecutive = state.consecutive_matches;
        self.eval_state.upsert_state(state).await?;
        Ok(Decision {
            firing: fired.is_some(),
            severity: fired,
            matched: matched_any,
            deviation_ratio: None,
            consecutive,
        })
    }

    /// Anomaly 求值：取 `lookback_days` 个同时刻历史窗口的首格值作基线，
    /// 跑 [`detector_for`] 选出的 detector 判定当前值是否离群。
    ///
    /// 返回 `Some(firing)`（接入与 scheduled 相同的去抖 / incident 生命周期），或
    /// `None` 表示 **inconclusive**——基线不可信（历史窗口抓取失败、缺 params、当前
    /// 窗口无数值、或一个有效历史样本都没有）。inconclusive 时 caller 跳过本轮且
    /// 不动状态，避免把"评估不了"误判成"恢复正常"而 resolve 掉真在 firing 的 incident。
    ///
    /// 代价：每个 tick 会按 `lookback_days` 各跑一次历史窗口查询（route 与此处都按
    /// `MAX_ANOMALY_LOOKBACK_DAYS` 收口；evaluator 再 clamp 一次，不信任持久化越界值），
    /// 后续可换成单条按天分桶的 SQL / 基线缓存优化（follow-up）。
    pub(super) async fn eval_anomaly(
        &self,
        rule: &AlertRule,
        stream: &StreamHint,
        current_result: &QueryResult,
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
        let current = first_cell_f64(current_result)?;
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
            severity: None,
            matched,
            deviation_ratio: None,
            consecutive,
        }
    }

    #[test]
    fn rule_state_distinguishes_healthy_pending_and_firing() {
        assert_eq!(decision(false, false, 0).rule_state(), RuleState::Healthy);
        assert_eq!(decision(false, true, 1).rule_state(), RuleState::Pending(1));
        assert_eq!(decision(true, true, 2).rule_state(), RuleState::Firing);
    }
}
