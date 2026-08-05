// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! AlertRule 评估器：
//!
//! `tick(rules, query_engine, eval_state_repo, incident_repo)`：
//! 1. 遍历 `rules.list_enabled()`（caller 已按 org 过滤，evaluator 直接迭代）
//! 2. 每条 rule 跑 `query_engine.execute` 包 `tokio::time::timeout(eval_timeout_secs)`
//! 3. 把结果首行首列与 `rule.trigger.threshold` 按 `ComparisonOp` 比较 → matched bool
//! 4. `eval_state_repo.upsert_match(rule_id, matched, now)` → 拿到 state.consecutive_matches
//! 5. open/resolve incident：
//!    - `state.consecutive_matches >= for_periods` 且不存在同 fingerprint 的 open incident → 创建
//!    - matched=false 且存在 open incident → mark Resolved + `eval_state_repo.reset(rule_id)`

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use crate::{
    app::notify::{
        ALERT_RESOLVED_EVENT, ALERT_TRIGGERED_EVENT, NotifyEngine, alert_dispatch,
        triggered_event_id,
    },
    domain::{
        alerting::{
            anomaly::{AnomalyDecision, MAX_ANOMALY_LOOKBACK_DAYS, detector_for},
            incident::{
                Incident, IncidentStatus, Severity, TriggeringQuery, generate_title,
                resolve_incident_severity,
            },
            incident_group::IncidentGroupRepository,
            repositories::{
                AlertRuleEvalState, AlertRuleEvalStateRepository, AlertRuleRepository,
                IncidentRepository,
            },
            rule::{AlertRule, AlertRuleKind, ComparisonOp},
            semantic_group::SemanticGroupRepository,
        },
        query::{QueryEngine, QueryRequest, QueryResult, StreamHint},
    },
    shared::{
        Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

mod incident_context;
use incident_context::derive_incident_context;

/// 一天的微秒数；anomaly 历史窗口按整天回看。
const DAY_MICROS: i64 = 86_400_000_000;

/// incident grouping 的合并时间窗：新 incident 与同 (scope, group key) 上一个的
/// last_at 在此窗口内则并入同组（count++）、否则另起一组。
const GROUP_WINDOW_SECS: i64 = 6 * 3600;

pub struct RuleEvaluator {
    rules: Arc<dyn AlertRuleRepository>,
    incidents: Arc<dyn IncidentRepository>,
    eval_state: Arc<dyn AlertRuleEvalStateRepository>,
    sql_engine: Arc<dyn QueryEngine>,
    eval_timeout_secs: u32,
    /// incident grouping（可选）：仅当 [`with_grouping`](Self::with_grouping) 接上时启用。
    incident_groups: Option<Arc<dyn IncidentGroupRepository>>,
    semantic_groups: Option<Arc<dyn SemanticGroupRepository>>,
    notify_engine: Option<Arc<NotifyEngine>>,
}

impl RuleEvaluator {
    pub fn new(
        rules: Arc<dyn AlertRuleRepository>,
        incidents: Arc<dyn IncidentRepository>,
        eval_state: Arc<dyn AlertRuleEvalStateRepository>,
        sql_engine: Arc<dyn QueryEngine>,
        eval_timeout_secs: u32,
    ) -> Self {
        Self {
            rules,
            incidents,
            eval_state,
            sql_engine,
            eval_timeout_secs,
            incident_groups: None,
            semantic_groups: None,
            notify_engine: None,
        }
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

    #[tracing::instrument(
        name = "worker.alert_evaluator",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "alert_evaluator")
    )]
    pub async fn tick(&self, now: TimestampMicros) -> Result<()> {
        let rules = self.rules.list_enabled().await?;
        for rule in rules {
            if let Err(e) = self.eval_one(&rule, now).await {
                tracing::warn!(rule_id = %rule.id, error = %e, "rule eval failed");
            }
        }
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

        // 当前窗口 [now - period, now]：阈值 / anomaly 两种 kind 都先取一次当前值，
        // 顺带给 incident 提供跨信号上下文（trace/host/service 列）。
        let Some(query_result) = self
            .run_window(rule, &stream, TimestampMicros(now.0 - period_us), now)
            .await
        else {
            // 超时 / 查询错误：run_window 已 warn，跳过本轮（不动 eval_state）。
            return Ok(());
        };

        // 计算 firing（是否触发）、fired_severity（多档命中的最高档）、matched_any（任一档命中，
        // 用于 resolve 判定）、deviation_ratio（仅 anomaly）。多档与单档/anomaly 互斥。
        let (firing, fired_severity, matched_any, deviation_ratio) = if !rule.thresholds.is_empty()
            && rule.kind != AlertRuleKind::Anomaly
        {
            self.eval_thresholds(rule, &query_result, now).await?
        } else {
            match rule.kind {
                AlertRuleKind::Anomaly => {
                    match self
                        .eval_anomaly(rule, &stream, &query_result, now, period_us)
                        .await
                    {
                        Some(decision) => {
                            let m = decision.firing;
                            let state = self.eval_state.upsert_match(&rule.id, m, now).await?;
                            let firing = m && state.consecutive_matches >= rule.trigger.for_periods;
                            (firing, None, m, Some(decision.deviation_ratio()))
                        }
                        // 基线不可信：本轮 inconclusive，跳过且不动 eval_state（绝不误 resolve）。
                        None => return Ok(()),
                    }
                }
                // RealTime 当前仍按 scheduled 周期跑（realtime matcher 接入是 follow-up）。
                AlertRuleKind::Scheduled | AlertRuleKind::RealTime => {
                    let m = first_cell_matches(
                        &query_result,
                        &rule.trigger.operator,
                        rule.trigger.threshold,
                    );
                    let state = self.eval_state.upsert_match(&rule.id, m, now).await?;
                    let firing = m && state.consecutive_matches >= rule.trigger.for_periods;
                    (firing, None, m, None)
                }
            }
        };

        let fingerprint = compute_fingerprint(rule);
        let existing = self
            .incidents
            .find_by_fingerprint(&rule.org_id, &fingerprint)
            .await?;

        if firing {
            match existing.as_ref() {
                // 已 open：处理 severity 升降级（fingerprint 不含 severity，故同一逻辑告警单 incident）。
                Some(inc) if inc.status == IncidentStatus::Open => {
                    let new_sev = resolve_incident_severity(
                        fired_severity,
                        rule.severity,
                        &inc.labels,
                        deviation_ratio,
                    );
                    if new_sev != inc.severity {
                        let mut updated = inc.clone();
                        updated.severity = new_sev;
                        // 升级（更高档）：重置升级进度，重新从头 page；降级仅更新字段。
                        if new_sev > inc.severity {
                            updated.current_step = 0;
                            updated.current_loop = 0;
                            updated.current_step_started_at = now;
                        }
                        self.incidents.update(updated).await?;
                    }
                }
                // 无 open incident（不存在 / 已 resolved / closed）：创建新的。
                _ => {
                    // open incident — freeze rule labels/annotations + 采样跨信号 handle
                    let context = derive_incident_context(rule, &query_result);
                    let group_labels = context.labels.clone();
                    let severity = resolve_incident_severity(
                        fired_severity,
                        rule.severity,
                        &context.labels,
                        deviation_ratio,
                    );
                    let summary = generate_title(&rule.name, &context.affected_services);
                    let incident = Incident {
                        id: Id::new(),
                        org_id: rule.org_id.clone(),
                        rule_id: rule.id.clone(),
                        escalation_policy_id: rule.escalation_policy_id.clone(),
                        status: IncidentStatus::Open,
                        severity,
                        summary,
                        fingerprint: fingerprint.clone(),
                        current_step: 0,
                        current_loop: 0,
                        current_step_started_at: now,
                        assignees: Vec::new(),
                        labels: context.labels,
                        annotations: rule.annotations.clone(),
                        trace_ids: context.trace_ids,
                        host_ids: context.host_ids,
                        affected_services: context.affected_services,
                        triggering_query: Some(TriggeringQuery {
                            language: rule.query.language,
                            statement: rule.query.statement.clone(),
                            sample_values: context.sample_values,
                        }),
                        created_at: now,
                        acknowledged_at: None,
                        acknowledged_by: None,
                        resolved_at: None,
                        resolved_by: None,
                    };
                    let incident = self.incidents.create(incident).await?;
                    self.enqueue_notify_event(&incident, ALERT_TRIGGERED_EVENT)
                        .await;
                    self.assign_to_group(rule, &group_labels, &fingerprint, now)
                        .await;
                }
            }
        } else if !matched_any {
            // 所有档都不命中 → resolve open incident（命中但未到 for_periods 的 pending 不动）。
            if let Some(mut inc) = existing
                && inc.status == IncidentStatus::Open
            {
                inc.status = IncidentStatus::Resolved;
                inc.resolved_at = Some(now);
                let incident = self.incidents.update(inc).await?;
                self.enqueue_notify_event(&incident, ALERT_RESOLVED_EVENT)
                    .await;
                self.eval_state.reset(&rule.id).await?;
            }
        }
        Ok(())
    }

    async fn enqueue_notify_event(&self, incident: &Incident, event_type: &str) {
        let Some(engine) = &self.notify_engine else {
            return;
        };
        if event_type == ALERT_RESOLVED_EVENT
            && let Err(error) = engine
                .acknowledge_event(
                    &incident.org_id,
                    &triggered_event_id(&incident.id),
                    incident.resolved_at.unwrap_or(incident.created_at),
                )
                .await
        {
            tracing::warn!(
                incident_id = %incident.id,
                error = %error,
                "notify alert resolved acknowledgement update failed"
            );
        }
        if let Err(error) = engine
            .enqueue_event(alert_dispatch(incident, event_type))
            .await
        {
            tracing::warn!(
                incident_id = %incident.id,
                event_type,
                error = %error,
                "alert notify event enqueue failed"
            );
        }
    }

    /// 多档阈值评估：逐档比较首格值、每档独立去抖（severity_streaks），取最高命中档。
    /// 返回 `(firing, 最高命中档 severity, 任一档命中 matched_any, deviation_ratio=None)`。
    /// read-modify-write 依赖 alert_manager 单实例运行（spawn_alert_manager_loops 单进程）。
    async fn eval_thresholds(
        &self,
        rule: &AlertRule,
        query_result: &QueryResult,
        now: TimestampMicros,
    ) -> Result<(bool, Option<Severity>, bool, Option<f64>)> {
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
        self.eval_state.upsert_state(state).await?;
        Ok((fired.is_some(), fired, matched_any, None))
    }

    /// 跑一条规则查询在 [from, to] 窗口的结果；超时 / 错误返回 None 并 warn。
    async fn run_window(
        &self,
        rule: &AlertRule,
        stream: &StreamHint,
        from: TimestampMicros,
        to: TimestampMicros,
    ) -> Option<QueryResult> {
        let req = QueryRequest {
            org_id: rule.org_id.clone(),
            language: rule.query.language,
            statement: rule.query.statement.clone(),
            time_range: TimeRange::new(from, to),
            stream: Some(stream.clone()),
            limit: Some(1),
            federation_clusters: Vec::new(),
        };
        let timeout = Duration::from_secs(self.eval_timeout_secs as u64);
        match tokio::time::timeout(timeout, self.sql_engine.execute(req)).await {
            Err(_) => {
                tracing::warn!(rule_id = %rule.id, "rule eval timed out");
                None
            }
            Ok(Err(e)) => {
                tracing::warn!(rule_id = %rule.id, error = %e, "rule query error");
                None
            }
            Ok(Ok(q)) => Some(q),
        }
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
    async fn eval_anomaly(
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

    /// 把刚创建的 incident 收拢进 incident_groups（告警分组规则）：
    /// 取该 org 第一条 enabled 且 matchers 全命中的 [`SemanticGroup`]，用其 `group_by`
    /// 派生 grouping key，scope = group id（于是跨规则、group_by 取值相同的相关
    /// incident 收成一组，类似 Alertmanager group_by）；无命中则按 incident 自身
    /// fingerprint 分组、scope = rule id（原始的逐规则分组行为）。
    ///
    /// 仅 [`with_grouping`](Self::with_grouping) 接上后才执行；grouping 失败只 warn，
    /// 不影响告警 / incident 主流程。
    async fn assign_to_group(
        &self,
        rule: &AlertRule,
        incident_labels: &BTreeMap<String, String>,
        incident_fingerprint: &str,
        now: TimestampMicros,
    ) {
        let (Some(groups), Some(semantic)) = (&self.incident_groups, &self.semantic_groups) else {
            return;
        };
        let enabled = match semantic.list_enabled(&rule.org_id).await {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!(rule_id = %rule.id, error = %e, "load semantic groups failed; skip grouping");
                return;
            }
        };
        let (scope_id, group_fp) = match enabled.iter().find(|g| g.matches(incident_labels)) {
            Some(g) => (g.id.clone(), g.group_key(incident_labels)),
            None => (rule.id.clone(), incident_fingerprint.to_string()),
        };
        if let Err(e) = groups
            .upsert_for_incident(&rule.org_id, &scope_id, &group_fp, now, GROUP_WINDOW_SECS)
            .await
        {
            tracing::warn!(rule_id = %rule.id, error = %e, "incident grouping upsert failed");
        }
    }
}

/// 结果集首行首列解析成 f64（数值列）；缺值 / 非数值返回 None。
fn first_cell_f64(res: &QueryResult) -> Option<f64> {
    let val = res.rows.first()?.first()?;
    val.as_f64()
        .or_else(|| val.as_i64().map(|i| i as f64))
        .or_else(|| val.as_u64().map(|u| u as f64))
}

fn compare_value(v: f64, op: &ComparisonOp, threshold: f64) -> bool {
    match op {
        ComparisonOp::Gt => v > threshold,
        ComparisonOp::Gte => v >= threshold,
        ComparisonOp::Lt => v < threshold,
        ComparisonOp::Lte => v <= threshold,
        ComparisonOp::Eq => (v - threshold).abs() < f64::EPSILON,
        ComparisonOp::Neq => (v - threshold).abs() >= f64::EPSILON,
    }
}

fn first_cell_matches(res: &QueryResult, op: &ComparisonOp, threshold: f64) -> bool {
    match first_cell_f64(res) {
        Some(v) if !v.is_nan() => compare_value(v, op, threshold),
        _ => false,
    }
}

fn compute_fingerprint(rule: &AlertRule) -> String {
    // 简化：rule_id + sorted labels（blake3）
    let mut buf = format!("{}|", rule.id.0);
    for (k, v) in &rule.labels {
        buf.push_str(k);
        buf.push('=');
        buf.push_str(v);
        buf.push(';');
    }
    blake3::hash(buf.as_bytes()).to_hex().to_string()
}
