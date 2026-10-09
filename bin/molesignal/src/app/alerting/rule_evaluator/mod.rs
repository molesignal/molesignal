// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! AlertRule 评估器：
//!
//! `tick(now)` 遍历 `rules.list_enabled()`，对本节点拥有的每条规则（多个 alert 节点按 `rule_id`
//! 分担，见 [`WorkOwnership`]）：
//! 1. 在 `[now - period, now]` 上执行规则查询（SQL 与 PromQL 各走自己的引擎），
//!    包 `tokio::time::timeout(eval_timeout_secs)`。
//! 2. 把结果首行首列与阈值比较（单档 / 多档 / anomaly）得到一轮结论 `Decision`，并推进
//!    `eval_state` 的连续命中计数。
//! 3. 落到事件生命周期（见 `incident_flow`）：
//!    - 连续命中 >= `for_periods` 且同指纹没有活跃事件、也不在 `silence_secs` 静默期 → 创建；
//!    - 不再命中且存在 open 事件 → 标记 Resolved 并 `eval_state.reset`。
//! 4. 把本轮的 `last_eval_at` / `last_state` 写回规则。
//!
//! 成员变化的短暂窗口里两个节点可能同时拥有一条规则。接上周期守卫
//! （[`with_cycle_guard`](RuleEvaluator::with_cycle_guard)）后，评估前先认领本轮：同一个周期只有
//! 一个节点能评估，命中不会被重复计数，`for_periods` 的去抖也不会被提前耗尽。

use std::{sync::Arc, time::Duration};

use super::realtime::is_matched_at_intake;
use crate::{
    app::{
        alerting::AlertIncidentLifecycleSink,
        cluster::{PeerRole, WorkOwnership, record_overlap_skipped},
        notify::NotifyEngine,
    },
    domain::{
        alerting::{
            incident_group::IncidentGroupRepository,
            repositories::{AlertRuleEvalStateRepository, AlertRuleRepository, IncidentRepository},
            rule::AlertRule,
            semantic_group::SemanticGroupRepository,
        },
        query::QueryEngine,
    },
    shared::{Result, time::TimestampMicros},
};

mod decision;
mod evaluation;
mod events;
mod incident_context;
mod incident_flow;
mod manual;
mod realtime;
mod window;
use evaluation::compute_fingerprint;
pub use manual::{AlertRuleAnomalyTest, AlertRuleTestResult};

/// 一天的微秒数；anomaly 历史窗口按整天回看。
const DAY_MICROS: i64 = 86_400_000_000;

/// 归属与重叠指标里评估这项工作的名字。
const WORK: &str = "rule_evaluation";

/// incident grouping 的合并时间窗：新 incident 与同 (scope, group key) 上一个的
/// last_at 在此窗口内则并入同组（count++）、否则另起一组。
const GROUP_WINDOW_SECS: i64 = 6 * 3600;

pub struct RuleEvaluator {
    rules: Arc<dyn AlertRuleRepository>,
    incidents: Arc<dyn IncidentRepository>,
    eval_state: Arc<dyn AlertRuleEvalStateRepository>,
    query_engine: Arc<dyn QueryEngine>,
    eval_timeout_secs: u32,
    /// incident grouping（可选）：仅当 [`with_grouping`](Self::with_grouping) 接上时启用。
    incident_groups: Option<Arc<dyn IncidentGroupRepository>>,
    semantic_groups: Option<Arc<dyn SemanticGroupRepository>>,
    notify_engine: Option<Arc<NotifyEngine>>,
    lifecycle_sink: Option<Arc<dyn AlertIncidentLifecycleSink>>,
    /// 多个 alert 节点按 `rule_id` 分担评估；默认不分担（本节点评估全部规则）。
    ownership: Arc<WorkOwnership>,
    /// 同一规则两次评估至少相隔多久才算新的一个周期；`None` 不去重。
    cycle_gap_micros: Option<i64>,
}

impl RuleEvaluator {
    pub fn new(
        rules: Arc<dyn AlertRuleRepository>,
        incidents: Arc<dyn IncidentRepository>,
        eval_state: Arc<dyn AlertRuleEvalStateRepository>,
        query_engine: Arc<dyn QueryEngine>,
        eval_timeout_secs: u32,
    ) -> Self {
        Self {
            rules,
            incidents,
            eval_state,
            query_engine,
            eval_timeout_secs,
            incident_groups: None,
            semantic_groups: None,
            notify_engine: None,
            lifecycle_sink: None,
            ownership: WorkOwnership::solo(PeerRole::Alert),
            cycle_gap_micros: None,
        }
    }

    /// 与其余 alert 节点按 `rule_id` 分担评估：`tick` 只评估本节点拥有的规则。
    pub fn with_ownership(mut self, ownership: Arc<WorkOwnership>) -> Self {
        self.ownership = ownership;
        self
    }

    /// 同一规则两次评估相隔不足 `min_gap` 时视为同一个周期，后到的评估跳过。取评估间隔的一半即可：
    /// 正常运行时相邻评估相隔一整个间隔，交接时两个节点的评估才会挤在同一个周期里。
    pub fn with_cycle_guard(mut self, min_gap: Duration) -> Self {
        self.cycle_gap_micros = Some(i64::try_from(min_gap.as_micros()).unwrap_or(i64::MAX));
        self
    }

    /// 接上 incident grouping：evaluator 创建 incident 后按语义分组规则把它收拢进
    /// incident_groups。不接（默认）时 grouping 不发生，告警主流程不受影响。
    pub fn with_grouping(
        mut self,
        incident_groups: Arc<dyn IncidentGroupRepository>,
        semantic_groups: Arc<dyn SemanticGroupRepository>,
    ) -> Self {
        self.incident_groups = Some(incident_groups);
        self.semantic_groups = Some(semantic_groups);
        self
    }

    pub fn with_notify_engine(mut self, notify_engine: Arc<NotifyEngine>) -> Self {
        self.notify_engine = Some(notify_engine);
        self
    }

    pub fn with_lifecycle_sink(mut self, sink: Arc<dyn AlertIncidentLifecycleSink>) -> Self {
        self.lifecycle_sink = Some(sink);
        self
    }

    #[tracing::instrument(
        name = "worker.alert_evaluator",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "alert_evaluator")
    )]
    pub async fn tick(&self, now: TimestampMicros) -> Result<()> {
        let rules = self.rules.list_enabled().await?;
        let mut owned = 0;
        for rule in rules {
            // 写入路径上的匹配器负责的实时规则在记录写入时求值，评估器跳过它们。
            if is_matched_at_intake(&rule) {
                continue;
            }
            // 每条规则开评前判定归属：一轮评估可能很长，成员变化后要及时让出或接管。
            if !self.ownership.owns(rule.id.as_str()).await {
                continue;
            }
            owned += 1;
            if let Err(e) = self.eval_one(&rule, now).await {
                tracing::warn!(rule_id = %rule.id, error = %e, "rule eval failed");
            }
        }
        self.ownership.record_owned(WORK, owned);
        Ok(())
    }

    #[tracing::instrument(
        name = "alert.evaluate",
        skip_all,
        fields(otel.kind = "internal", molesignal.alert.kind = %rule.kind.as_str())
    )]
    async fn eval_one(&self, rule: &AlertRule, now: TimestampMicros) -> Result<()> {
        let Some(stream) = rule.query.stream.clone() else {
            // 缺 stream 字段：底层 query engine 一定会拒（必须显式指定 table）。
            // 直接 short-circuit 并 warn，避免每个 tick 都让 engine 报一遍错。
            tracing::warn!(
                rule_id = %rule.id,
                "alert rule missing query.stream; rule skipped until stream is set"
            );
            return Ok(());
        };
        let period_us = (rule.query.period_secs as i64) * 1_000_000;

        // 同一周期只评估一次：别的节点已经评估过本周期（成员变化时两个节点都拥有这条规则）就跳过。
        if let Some(gap) = self.cycle_gap_micros
            && !self.eval_state.claim_cycle(&rule.id, now, gap).await?
        {
            record_overlap_skipped(WORK);
            return Ok(());
        }

        // 当前窗口 [now - period, now]：阈值 / anomaly 两种 kind 都先取一次当前值，
        // 顺带给 incident 提供跨信号上下文（trace/host/service 列）。
        let Some(query_result) = self
            .run_window(rule, &stream, TimestampMicros(now.0 - period_us), now)
            .await
        else {
            // 超时 / 查询错误：run_window 已 warn，跳过本轮（不动 eval_state，也不记录评估）。
            return Ok(());
        };

        // 基线不可信（anomaly inconclusive）：跳过本轮，绝不误 resolve。
        let Some(decision) = self
            .decide(rule, &stream, &query_result, now, period_us)
            .await?
        else {
            return Ok(());
        };

        let fingerprint = compute_fingerprint(rule);
        let existing = self
            .incidents
            .find_by_fingerprint(&rule.org_id, &fingerprint)
            .await?;

        if decision.firing {
            self.on_firing(rule, &decision, existing, &query_result, &fingerprint, now)
                .await?;
        } else if !decision.matched {
            // 所有档都不命中 → resolve open incident（命中但未到 for_periods 的 pending 不动）。
            self.on_cleared(rule, existing, now).await?;
        }
        self.record_evaluation(rule, &decision, now).await;
        Ok(())
    }

    /// 把本轮评估结果写回规则；失败只 warn，不影响告警主流程。
    async fn record_evaluation(
        &self,
        rule: &AlertRule,
        decision: &decision::Decision,
        now: TimestampMicros,
    ) {
        if let Err(error) = self
            .rules
            .record_evaluation(&rule.id, now, decision.rule_state())
            .await
        {
            tracing::warn!(rule_id = %rule.id, error = %error, "record rule evaluation failed");
        }
    }
}
