// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 的用量与预算。
//!
//! 每次模型调用由 [`UsageRecorder`] 记进当天的计数，并排队写进 `_agent_model_traces`。
//! 调模型之前先问 [`AgentUsage::gate`]：组织今天的 token 是否用完，用完时后台调用不做，
//! 有人在等的调用按组织设置拦截或只提示。预算状况在进程内缓存十几秒，记账时顺手更新，
//! 所以一轮回答可能比上限多用一点，不会多用很多。

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use serde::Serialize;

use crate::{
    agent::usage::{
        AgentUsageRepository, BudgetStatus, DailyUsage, InteractiveBudgetMode, ModelPrices,
        UsageBudget, UsageDay, UsageDelta, UsageOrigin,
    },
    config::AgentSettings,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

mod recorder;
mod traces;

pub use recorder::UsageRecorder;
pub use traces::{TraceEvent, TraceQueue, TraceReceiver, trace_queue};

/// 缓存的预算状况多久之后重新从库里算。
const STATUS_TTL: Duration = Duration::from_secs(15);

/// 用量页最多回看这么多天。
pub const MAX_HISTORY_DAYS: i64 = 90;

/// 一个记账器记到谁的名下、按什么价格算成本、在用量流里带上哪些上下文。
#[derive(Debug, Clone)]
pub struct UsageScope {
    pub org_id: Id,
    pub origin: UsageOrigin,
    pub user_id: Option<Id>,
    pub provider_id: Option<Id>,
    pub prices: Option<ModelPrices>,
    pub chat_id: Option<Id>,
    pub profile_id: Option<Id>,
    pub prompt_hash: Option<String>,
    /// 这一轮显式调用的 Skill。
    pub skill_id: Option<String>,
}

impl UsageScope {
    pub fn new(org_id: Id, origin: UsageOrigin) -> Self {
        Self {
            org_id,
            origin,
            user_id: None,
            provider_id: None,
            prices: None,
            chat_id: None,
            profile_id: None,
            prompt_hash: None,
            skill_id: None,
        }
    }
}

/// 调模型之前的判定。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BudgetGate {
    Proceed(BudgetStatus),
    Refuse(BudgetStatus),
}

impl BudgetGate {
    pub fn status(&self) -> &BudgetStatus {
        match self {
            Self::Proceed(status) | Self::Refuse(status) => status,
        }
    }

    pub fn refused(&self) -> bool {
        matches!(self, Self::Refuse(_))
    }
}

/// 组织的预算设置连同实例默认值，给设置页展示。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BudgetSettings {
    /// 组织自己设的每日上限；空表示跟随实例默认值，0 表示不限。
    pub org_limit: Option<i64>,
    /// 实例配置 `[agent] daily_token_budget`；0 表示不限。
    pub instance_default: i64,
    /// 现在生效的上限；0 表示不限。
    pub effective_limit: i64,
    pub interactive_mode: InteractiveBudgetMode,
    pub warning_percent: u8,
    pub updated_by: Option<Id>,
    pub updated_at: Option<TimestampMicros>,
}

#[derive(Debug, Clone, Copy)]
struct CachedStatus {
    status: BudgetStatus,
    fetched: Instant,
}

pub struct AgentUsage {
    repository: Arc<dyn AgentUsageRepository>,
    traces: TraceQueue,
    instance_default: i64,
    warning_percent: u8,
    statuses: Mutex<HashMap<Id, CachedStatus>>,
}

impl AgentUsage {
    pub fn new(
        repository: Arc<dyn AgentUsageRepository>,
        settings: &AgentSettings,
        traces: TraceQueue,
    ) -> Self {
        Self {
            repository,
            traces,
            instance_default: i64::try_from(settings.daily_token_budget).unwrap_or(i64::MAX),
            warning_percent: settings.budget_warning_percent.clamp(1, 100),
            statuses: Mutex::new(HashMap::new()),
        }
    }

    /// 一个新的记账器：一轮回答、一次巡检或一次 RCA 用一个，结束后读它的合计。
    pub fn recorder(self: &Arc<Self>, scope: UsageScope) -> Arc<UsageRecorder> {
        Arc::new(UsageRecorder::new(self.clone(), scope))
    }

    /// 组织今天的预算状况。缓存过期或跨了 UTC 零点时重新计算；读不到用量时不拦任何调用。
    pub async fn status(&self, org_id: &Id) -> BudgetStatus {
        let now = TimestampMicros::now();

        let cached = self.statuses.lock().get(org_id).copied().filter(|cached| {
            cached.fetched.elapsed() < STATUS_TTL && cached.status.resets_at.0 > now.0
        });

        if let Some(cached) = cached {
            return cached.status;
        }

        match self.compute(org_id).await {
            Ok(status) => {
                self.statuses.lock().insert(
                    org_id.clone(),
                    CachedStatus {
                        status,
                        fetched: Instant::now(),
                    },
                );

                status
            }
            Err(error) => {
                tracing::warn!(org = %org_id.0, %error, "agent budget status unavailable");

                BudgetStatus::evaluate(
                    0,
                    0,
                    self.warning_percent,
                    InteractiveBudgetMode::Block,
                    UsageDay::today(),
                )
            }
        }
    }

    /// 这次调用能不能做。`interactive` 表示有人在等这次回答。
    pub async fn gate(&self, org_id: &Id, interactive: bool) -> BudgetGate {
        let status = self.status(org_id).await;

        if status.allows(interactive) {
            BudgetGate::Proceed(status)
        } else {
            BudgetGate::Refuse(status)
        }
    }

    async fn compute(&self, org_id: &Id) -> Result<BudgetStatus> {
        let day = UsageDay::today();
        let budget = self.repository.get_budget(org_id).await?;
        let used = self.repository.tokens_on(org_id, day).await?;

        let limit = budget
            .as_ref()
            .and_then(|budget| budget.daily_token_limit)
            .unwrap_or(self.instance_default);

        let mode = budget
            .map(|budget| budget.interactive_mode)
            .unwrap_or_default();

        Ok(BudgetStatus::evaluate(
            used,
            limit,
            self.warning_percent,
            mode,
            day,
        ))
    }

    /// 记一笔用量：加进当天的计数，并让缓存里的预算状况跟上。记不上只记日志。
    pub(crate) async fn add(&self, scope: &UsageScope, delta: &UsageDelta) {
        let recorded = self
            .repository
            .add(&scope.org_id, UsageDay::today(), scope.origin, delta)
            .await;

        if let Err(error) = recorded {
            tracing::warn!(org = %scope.org_id.0, %error, "agent usage was not recorded");
        }

        if let Some(cached) = self.statuses.lock().get_mut(&scope.org_id) {
            cached.status = cached.status.with_spent(delta.tokens());
        }
    }

    pub(crate) fn enqueue_trace(&self, event: TraceEvent) {
        self.traces.push(event);
    }

    /// 最近 `days` 天（含今天）的用量。
    pub async fn history(&self, org_id: &Id, days: i64) -> Result<Vec<DailyUsage>> {
        let today = UsageDay::today();
        let days = days.clamp(1, MAX_HISTORY_DAYS);

        self.repository
            .list(org_id, today.minus(days - 1), today)
            .await
    }

    pub async fn budget_settings(&self, org_id: &Id) -> Result<BudgetSettings> {
        let budget = self.repository.get_budget(org_id).await?;
        Ok(self.settings_view(budget))
    }

    /// 保存组织的预算设置。`daily_token_limit` 为空表示跟随实例默认值。
    pub async fn put_budget(
        &self,
        org_id: &Id,
        daily_token_limit: Option<i64>,
        interactive_mode: InteractiveBudgetMode,
        updated_by: &Id,
    ) -> Result<BudgetSettings> {
        if daily_token_limit.is_some_and(|limit| limit < 0) {
            return Err(Error::invalid("daily_token_limit must be zero or more"));
        }

        let saved = self
            .repository
            .put_budget(UsageBudget {
                org_id: org_id.clone(),
                daily_token_limit,
                interactive_mode,
                updated_by: updated_by.clone(),
                updated_at: TimestampMicros::now(),
            })
            .await?;

        self.statuses.lock().remove(org_id);

        Ok(self.settings_view(Some(saved)))
    }

    fn settings_view(&self, budget: Option<UsageBudget>) -> BudgetSettings {
        let org_limit = budget.as_ref().and_then(|budget| budget.daily_token_limit);

        BudgetSettings {
            org_limit,
            instance_default: self.instance_default,
            effective_limit: org_limit.unwrap_or(self.instance_default),
            interactive_mode: budget
                .as_ref()
                .map(|budget| budget.interactive_mode)
                .unwrap_or_default(),
            warning_percent: self.warning_percent,
            updated_by: budget.as_ref().map(|budget| budget.updated_by.clone()),
            updated_at: budget.map(|budget| budget.updated_at),
        }
    }
}

#[cfg(test)]
mod tests;
