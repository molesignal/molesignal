// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `agent_usage_daily`：Mole Agent 的每日用量；组织预算在 `org_settings` 的 `agent.usage_budget` 一节。
//!
//! 计数按（组织、UTC 日期、来源、币种）一行，每次模型调用一条 upsert 累加；没有单价的调用记在
//! 币种为空串的那行。日期在 SQL 里是 `DATE`，Rust 里是从 1970-01-01 起的天数。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

use super::{
    super::org_settings::{self, Section, sections::AGENT_USAGE_BUDGET},
    sqlx_err,
};
use crate::{
    agent::usage::{
        AgentUsageRepository, Currency, DailyUsage, InteractiveBudgetMode, UsageBudget, UsageDay,
        UsageDelta, UsageOrigin,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct PgAgentUsageRepository {
    pool: PgPool,
}

impl PgAgentUsageRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// 天数转成 SQL 里能直接和 `DATE` 相加的整数。
fn day_number(day: UsageDay) -> Result<i32> {
    i32::try_from(day.0).map_err(|_| Error::invalid(format!("usage day {} is out of range", day.0)))
}

fn usage_row(row: sqlx::postgres::PgRow) -> Result<DailyUsage> {
    let origin: String = row.try_get("origin").map_err(sqlx_err)?;
    let currency: String = row.try_get("cost_currency").map_err(sqlx_err)?;

    Ok(DailyUsage {
        day: UsageDay(row.try_get("day_number").map_err(sqlx_err)?),
        origin: UsageOrigin::parse(&origin)?,
        calls: row.try_get("calls").map_err(sqlx_err)?,
        unpriced_calls: row.try_get("unpriced_calls").map_err(sqlx_err)?,
        prompt_tokens: row.try_get("prompt_tokens").map_err(sqlx_err)?,
        completion_tokens: row.try_get("completion_tokens").map_err(sqlx_err)?,
        cost: row.try_get("cost").map_err(sqlx_err)?,
        currency: (!currency.is_empty())
            .then(|| Currency::parse(&currency))
            .transpose()?,
    })
}

/// `agent.usage_budget` 一节的值。没有这一节时跟随实例配置 `[agent] daily_token_budget`。
#[derive(Serialize, Deserialize)]
struct StoredBudget {
    /// 每日 token 上限；空跟随实例默认值，0 表示不限。
    daily_token_limit: Option<i64>,
    /// 预算用完时对话与手动 RCA 的处理：block 拦截 | warn 只提示。
    interactive_mode: String,
}

fn budget_of(org_id: &Id, section: Section<StoredBudget>) -> Result<UsageBudget> {
    Ok(UsageBudget {
        org_id: org_id.clone(),
        daily_token_limit: section.value.daily_token_limit,
        interactive_mode: InteractiveBudgetMode::parse(&section.value.interactive_mode)?,
        updated_by: Id(section.updated_by.unwrap_or_default()),
        updated_at: section.updated_at,
    })
}

#[async_trait]
impl AgentUsageRepository for PgAgentUsageRepository {
    async fn add(
        &self,
        org_id: &Id,
        day: UsageDay,
        origin: UsageOrigin,
        delta: &UsageDelta,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO agent_usage_daily
                (org_id, day, origin, cost_currency, calls, unpriced_calls, prompt_tokens,
                 completion_tokens, cost, updated_at)
             VALUES ($1, DATE '1970-01-01' + $2::INTEGER, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (org_id, day, origin, cost_currency) DO UPDATE SET
                calls = agent_usage_daily.calls + EXCLUDED.calls,
                unpriced_calls = agent_usage_daily.unpriced_calls + EXCLUDED.unpriced_calls,
                prompt_tokens = agent_usage_daily.prompt_tokens + EXCLUDED.prompt_tokens,
                completion_tokens =
                    agent_usage_daily.completion_tokens + EXCLUDED.completion_tokens,
                cost = agent_usage_daily.cost + EXCLUDED.cost,
                updated_at = EXCLUDED.updated_at",
        )
        .bind(&org_id.0)
        .bind(day_number(day)?)
        .bind(origin.as_str())
        .bind(
            delta
                .currency
                .map(|currency| currency.as_str().to_string())
                .unwrap_or_default(),
        )
        .bind(delta.calls)
        .bind(delta.unpriced_calls)
        .bind(delta.prompt_tokens)
        .bind(delta.completion_tokens)
        .bind(delta.cost)
        .bind(TimestampMicros::now())
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn list(&self, org_id: &Id, from: UsageDay, to: UsageDay) -> Result<Vec<DailyUsage>> {
        let rows = sqlx::query(
            "SELECT (day - DATE '1970-01-01')::BIGINT AS day_number, origin, cost_currency,
                    calls, unpriced_calls, prompt_tokens, completion_tokens, cost
             FROM agent_usage_daily
             WHERE org_id = $1
               AND day BETWEEN DATE '1970-01-01' + $2::INTEGER
                           AND DATE '1970-01-01' + $3::INTEGER
             ORDER BY day, origin, cost_currency",
        )
        .bind(&org_id.0)
        .bind(day_number(from)?)
        .bind(day_number(to)?)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(usage_row).collect()
    }

    async fn tokens_on(&self, org_id: &Id, day: UsageDay) -> Result<i64> {
        let row = sqlx::query(
            "SELECT COALESCE(SUM(prompt_tokens + completion_tokens), 0)::BIGINT AS tokens
             FROM agent_usage_daily
             WHERE org_id = $1 AND day = DATE '1970-01-01' + $2::INTEGER",
        )
        .bind(&org_id.0)
        .bind(day_number(day)?)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;

        row.try_get("tokens").map_err(sqlx_err)
    }

    async fn get_budget(&self, org_id: &Id) -> Result<Option<UsageBudget>> {
        org_settings::read::<StoredBudget>(&self.pool, &org_id.0, AGENT_USAGE_BUDGET)
            .await?
            .map(|section| budget_of(org_id, section))
            .transpose()
    }

    async fn put_budget(&self, budget: UsageBudget) -> Result<UsageBudget> {
        let stored = StoredBudget {
            daily_token_limit: budget.daily_token_limit,
            interactive_mode: budget.interactive_mode.as_str().to_owned(),
        };

        org_settings::write(
            &self.pool,
            &budget.org_id.0,
            AGENT_USAGE_BUDGET,
            &stored,
            Some(&budget.updated_by.0),
            budget.updated_at,
        )
        .await?;

        Ok(budget)
    }
}
