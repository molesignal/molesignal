// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{
    escalation::EscalationPolicy,
    forecast::ForecastSeries,
    incident::{Incident, IncidentRca, IncidentStatus},
    rule::{AlertRule, RuleState},
    schedule::Schedule,
};
use crate::shared::{Result, ids::Id, time::TimestampMicros};

/// 评估状态行：alert_rule_eval_state 表的领域模型。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertRuleEvalState {
    pub rule_id: Id,
    /// 按列拆分的规则里是哪一组（各列取值的摘要）；整条规则一个状态时为空。
    #[serde(default)]
    pub group_key: String,
    /// 这一组各列的取值：结果里没有这一组时，靠它认出是哪一组。
    #[serde(default)]
    pub group_labels: BTreeMap<String, String>,
    pub consecutive_matches: u32,
    /// 连续多少轮在恢复线健康的一侧。
    #[serde(default)]
    pub consecutive_clears: u32,
    /// 连续多少轮没有值。
    #[serde(default)]
    pub consecutive_no_data: u32,
    /// 这一段没有值从哪一轮开始；有值时为空。
    #[serde(default)]
    pub no_data_since: Option<TimestampMicros>,
    pub last_eval_at: TimestampMicros,
    pub last_matched: bool,
    /// 每档（severity 字符串 → 连续命中次数）去抖计数；多档评估用，空 = 单档历史行为。
    #[serde(default)]
    pub severity_streaks: BTreeMap<String, u32>,
    /// 预测规则记着的序列；别的规则为空。
    #[serde(default)]
    pub series: ForecastSeries,
}

impl AlertRuleEvalState {
    /// 还没评估过的一组（或整条规则）。
    pub fn fresh(
        rule_id: &Id,
        group_key: &str,
        group_labels: BTreeMap<String, String>,
        now: TimestampMicros,
    ) -> Self {
        Self {
            rule_id: rule_id.clone(),
            group_key: group_key.to_string(),
            group_labels,
            consecutive_matches: 0,
            consecutive_clears: 0,
            consecutive_no_data: 0,
            no_data_since: None,
            last_eval_at: now,
            last_matched: false,
            severity_streaks: BTreeMap::new(),
            series: ForecastSeries::default(),
        }
    }
}

#[async_trait]
pub trait AlertRuleEvalStateRepository: Send + Sync {
    /// 认领某规则的本轮评估，保证同一个评估周期的命中只被计一次。
    ///
    /// 距上一次认领不足 `min_gap_micros` 时返回 `false`：这个周期已被另一个节点评估过（成员变化
    /// 的短暂窗口里两个节点同时拥有这条规则），调用方跳过本轮。否则把 `last_eval_at` 推进到
    /// `eval_at` 并返回 `true`。`last_eval_at` 只会前进，晚到的旧认领不会把它拨回去。
    async fn claim_cycle(
        &self,
        rule_id: &Id,
        eval_at: TimestampMicros,
        min_gap_micros: i64,
    ) -> Result<bool>;
    /// 一轮评估完整行写回（计数在评估器里算好）。`last_eval_at` 只会前进。
    async fn upsert_state(&self, state: AlertRuleEvalState) -> Result<AlertRuleEvalState>;
    /// `group_key` 是按列拆分的规则里的一组，整条规则时为空。
    async fn get(&self, rule_id: &Id, group_key: &str) -> Result<Option<AlertRuleEvalState>>;
    /// 按列拆分的规则记着的各组，不含规则本身那一行。
    async fn list_groups(&self, rule_id: &Id) -> Result<Vec<AlertRuleEvalState>>;
    /// 显式清零（incident resolve 或 rule threshold update 时调用）。
    async fn reset(&self, rule_id: &Id, group_key: &str) -> Result<()>;
    /// 忘掉这些组：删掉它们的状态，下次出现时从头算。规则本身那一行不会被删。
    async fn forget_groups(&self, rule_id: &Id, group_keys: &[String]) -> Result<()>;
}

#[async_trait]
pub trait AlertRuleRepository: Send + Sync {
    async fn create(&self, rule: AlertRule) -> Result<AlertRule>;
    /// 写回规则定义。`last_eval_at` 与 `last_state` 由评估器经
    /// [`record_evaluation`](Self::record_evaluation) 维护，这里不写，避免用户编辑覆盖掉评估结果。
    async fn update(&self, rule: AlertRule) -> Result<AlertRule>;

    /// 一次写入一批（导入）。能全部写进或都不写的实现覆盖它；这里逐条写。
    async fn save_all(&self, creates: Vec<AlertRule>, updates: Vec<AlertRule>) -> Result<()> {
        for rule in creates {
            self.create(rule).await?;
        }

        for rule in updates {
            self.update(rule).await?;
        }

        Ok(())
    }

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
    /// 仅当库中事件的状态属于 `expected`、且 `revision` 仍是读出时的值才写入，返回写入后的事件
    /// （`revision` 加一）。读出之后被别人改过（用户确认、转派，调度器升级，评估器解决）时不写入，
    /// 返回 `None`：调用方重读再改，或者放到下一轮。改事件都应走它，否则拿着旧副本的一方会把
    /// 对方的结果覆盖回去。
    async fn update_if_status(
        &self,
        incident: Incident,
        expected: &[IncidentStatus],
    ) -> Result<Option<Incident>>;
    async fn get(&self, id: &Id) -> Result<Incident>;
    async fn list_active(&self, org_id: &Id) -> Result<Vec<Incident>>;
    /// 并入 `target_id` 的事件，不论状态。
    async fn list_merged_into(&self, org_id: &Id, target_id: &Id) -> Result<Vec<Incident>>;
    /// 一条规则的活跃事件（open / acknowledged）。
    async fn list_active_for_rule(&self, org_id: &Id, rule_id: &Id) -> Result<Vec<Incident>>;
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
