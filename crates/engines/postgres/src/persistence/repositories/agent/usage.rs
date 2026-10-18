// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `agent_usage_daily` 与 `agent_usage_budgets`：Mole Agent 的每日用量与组织预算。
//!
//! 计数按（组织、UTC 日期、来源、币种）一行，每次模型调用一条 upsert 累加；没有单价的调用记在
//! 币种为空串的那行。日期在 SQL 里是 `DATE`，Rust 里是从 1970-01-01 起的天数。

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use super::sqlx_err;
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

fn budget_row(row: sqlx::postgres::PgRow) -> Result<UsageBudget> {
    let mode: String = row.try_get("interactive_mode").map_err(sqlx_err)?;

    Ok(UsageBudget {
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        daily_token_limit: row.try_get("daily_token_limit").map_err(sqlx_err)?,
        interactive_mode: InteractiveBudgetMode::parse(&mode)?,
        updated_by: Id(row.try_get("updated_by").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
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
                 completion_tokens, cost, updated_at_micros)
             VALUES ($1, DATE '1970-01-01' + $2::INTEGER, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (org_id, day, origin, cost_currency) DO UPDATE SET
                calls = agent_usage_daily.calls + EXCLUDED.calls,
                unpriced_calls = agent_usage_daily.unpriced_calls + EXCLUDED.unpriced_calls,
                prompt_tokens = agent_usage_daily.prompt_tokens + EXCLUDED.prompt_tokens,
                completion_tokens =
                    agent_usage_daily.completion_tokens + EXCLUDED.completion_tokens,
                cost = agent_usage_daily.cost + EXCLUDED.cost,
                updated_at_micros = EXCLUDED.updated_at_micros",
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
        .bind(TimestampMicros::now().0)
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
        sqlx::query(
            "SELECT org_id, daily_token_limit, interactive_mode, updated_by, updated_at_micros
             FROM agent_usage_budgets WHERE org_id = $1",
        )
        .bind(&org_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .map(budget_row)
        .transpose()
    }

    async fn put_budget(&self, budget: UsageBudget) -> Result<UsageBudget> {
        let row = sqlx::query(
            "INSERT INTO agent_usage_budgets
                (org_id, daily_token_limit, interactive_mode, updated_by, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (org_id) DO UPDATE SET
                daily_token_limit = EXCLUDED.daily_token_limit,
                interactive_mode = EXCLUDED.interactive_mode,
                updated_by = EXCLUDED.updated_by,
                updated_at_micros = EXCLUDED.updated_at_micros
             RETURNING org_id, daily_token_limit, interactive_mode, updated_by,
                       updated_at_micros",
        )
        .bind(&budget.org_id.0)
        .bind(budget.daily_token_limit)
        .bind(budget.interactive_mode.as_str())
        .bind(&budget.updated_by.0)
        .bind(budget.updated_at.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;

        budget_row(row)
    }
}
