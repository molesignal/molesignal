// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{
    escalation::EscalationPolicy,
    incident::{Incident, IncidentRca, IncidentStatus},
    rule::{AlertRule, RuleState},
    schedule::Schedule,
};
use crate::shared::{Result, ids::Id, time::TimestampMicros};

/// 评估状态行：alert_rule_eval_state 表的领域模型。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRuleEvalState {
    pub rule_id: Id,
    pub consecutive_matches: u32,
    pub last_eval_at: TimestampMicros,
    pub last_matched: bool,
    /// 每档（severity 字符串 → 连续命中次数）去抖计数；多档评估用，空 = 单档历史行为。
    #[serde(default)]
    pub severity_streaks: BTreeMap<String, u32>,
}

#[async_trait]
pub trait AlertRuleEvalStateRepository: Send + Sync {
    /// 一次评估完后 upsert：matched=true 时累加 consecutive_matches，否则清零。
    async fn upsert_match(
        &self,
        rule_id: &Id,
        matched: bool,
        eval_at: TimestampMicros,
    ) -> Result<AlertRuleEvalState>;
    /// 多档评估：整行写回（含 severity_streaks 的逐档计数）。单档路径仍用 upsert_match。
    async fn upsert_state(&self, state: AlertRuleEvalState) -> Result<AlertRuleEvalState>;
    async fn get(&self, rule_id: &Id) -> Result<Option<AlertRuleEvalState>>;
    /// 显式清零（incident resolve 或 rule threshold update 时调用）。
    async fn reset(&self, rule_id: &Id) -> Result<()>;
}

#[async_trait]
pub trait AlertRuleRepository: Send + Sync {
    async fn create(&self, rule: AlertRule) -> Result<AlertRule>;
    /// 写回规则定义。`last_eval_at` 与 `last_state` 由评估器经
    /// [`record_evaluation`](Self::record_evaluation) 维护，这里不写，避免用户编辑覆盖掉评估结果。
    async fn update(&self, rule: AlertRule) -> Result<AlertRule>;
    async fn get(&self, id: &Id) -> Result<AlertRule>;
    async fn list(&self, org_id: &Id) -> Result<Vec<AlertRule>>;
    async fn delete(&self, id: &Id) -> Result<()>;
    /// 给 alert 调度用：拉取所有启用的规则
    async fn list_enabled(&self) -> Result<Vec<AlertRule>>;
    /// 记录一次完成的评估，只写 `last_eval_at` 与 `last_state`。规则已被删除时静默忽略。
    async fn record_evaluation(
        &self,
        id: &Id,
        evaluated_at: TimestampMicros,
        state: RuleState,
    ) -> Result<()>;
}

#[async_trait]
pub trait IncidentRepository: Send + Sync {
    async fn create(&self, incident: Incident) -> Result<Incident>;
    async fn update(&self, incident: Incident) -> Result<Incident>;
    /// 仅当库中事件的当前状态属于 `expected` 时写入，返回写入后的事件；状态已被并发操作
    /// （用户确认 / 解决、评估器自动解决）改变时不写入并返回 `None`。状态迁移都应走它，
    /// 否则读到旧状态的一方会把对方的结果覆盖回去。
    async fn update_if_status(
        &self,
        incident: Incident,
        expected: &[IncidentStatus],
    ) -> Result<Option<Incident>>;
    async fn get(&self, id: &Id) -> Result<Incident>;
    async fn list_active(&self, org_id: &Id) -> Result<Vec<Incident>>;
    async fn find_by_fingerprint(&self, org_id: &Id, fingerprint: &str)
    -> Result<Option<Incident>>;
    async fn list_by_status(&self, org_id: &Id, status: IncidentStatus) -> Result<Vec<Incident>>;
    /// All incidents (any status) created at/after `since`, for insights
    /// aggregation (spec alert insights).
    async fn list_since(&self, org_id: &Id, since: TimestampMicros) -> Result<Vec<Incident>>;
}

/// AI 根因分析（RCA）持久化：后台 sweeper 写回、incident detail 读。每个 incident 一条。
#[async_trait]
pub trait IncidentRcaRepository: Send + Sync {
    /// 读某 incident 的 RCA；不存在返 None。
    async fn get(&self, incident_id: &Id) -> Result<Option<IncidentRca>>;
    /// 写入 / 覆盖某 incident 的 RCA。
    async fn upsert(&self, rca: IncidentRca) -> Result<IncidentRca>;
}

#[async_trait]
pub trait ScheduleRepository: Send + Sync {
    async fn create(&self, schedule: Schedule) -> Result<Schedule>;
    async fn update(&self, schedule: Schedule) -> Result<Schedule>;
    async fn get(&self, id: &Id) -> Result<Schedule>;
    async fn list(&self, org_id: &Id) -> Result<Vec<Schedule>>;
    async fn delete(&self, id: &Id) -> Result<()>;
}

#[async_trait]
pub trait EscalationPolicyRepository: Send + Sync {
    async fn create(&self, policy: EscalationPolicy) -> Result<EscalationPolicy>;
    async fn update(&self, policy: EscalationPolicy) -> Result<EscalationPolicy>;
    async fn get(&self, id: &Id) -> Result<EscalationPolicy>;
    async fn list(&self, org_id: &Id) -> Result<Vec<EscalationPolicy>>;
    async fn delete(&self, id: &Id) -> Result<()>;
}
