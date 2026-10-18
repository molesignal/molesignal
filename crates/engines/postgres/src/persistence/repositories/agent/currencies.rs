// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `agent_currencies`：组织给模型服务单价备的币种。
//!
//! 组织第一次读时写入常用的几种，用 `ON CONFLICT DO NOTHING`，同时读的两个请求不会重复写；
//! 写入时间按常用顺序错开一微秒，列表按写入时间排。

use async_trait::async_trait;
use sqlx::{PgPool, Row, postgres::PgRow};

use super::super::sqlx_err;
use crate::{
    agent::{
        currencies::{CurrencyRepository, DEFAULT_CURRENCIES, PriceCurrency},
        usage::Currency,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub struct PgCurrencyRepository {
    pool: PgPool,
}

impl PgCurrencyRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn rows(&self, org_id: &Id) -> Result<Vec<PriceCurrency>> {
        let rows = sqlx::query(
            "SELECT code, name, symbol, updated_at_micros
             FROM agent_currencies
             WHERE org_id = $1
             ORDER BY created_at_micros, code",
        )
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(currency_row).collect()
    }
}

fn currency_row(row: PgRow) -> Result<PriceCurrency> {
    let code: String = row.try_get("code").map_err(sqlx_err)?;

    Ok(PriceCurrency {
        code: Currency::parse(&code)?,
        name: row.try_get("name").map_err(sqlx_err)?,
        symbol: row.try_get("symbol").map_err(sqlx_err)?,
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

#[async_trait]
impl CurrencyRepository for PgCurrencyRepository {
    async fn list(&self, org_id: &Id) -> Result<Vec<PriceCurrency>> {
        let rows = self.rows(org_id).await?;

        if !rows.is_empty() {
            return Ok(rows);
        }

        sqlx::query(
            "INSERT INTO agent_currencies (org_id, code, created_at_micros, updated_at_micros)
             SELECT $1, code, $3 + ordinal, $3
             FROM unnest($2::TEXT[]) WITH ORDINALITY AS defaults(code, ordinal)
             ON CONFLICT (org_id, code) DO NOTHING",
        )
        .bind(&org_id.0)
        .bind(DEFAULT_CURRENCIES.map(str::to_string).to_vec())
        .bind(TimestampMicros::now().0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        self.rows(org_id).await
    }

    async fn put(&self, org_id: &Id, currency: &PriceCurrency) -> Result<PriceCurrency> {
        let row = sqlx::query(
            "INSERT INTO agent_currencies
                (org_id, code, name, symbol, created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $5)
             ON CONFLICT (org_id, code) DO UPDATE SET
                name = EXCLUDED.name,
                symbol = EXCLUDED.symbol,
                updated_at_micros = EXCLUDED.updated_at_micros
             RETURNING code, name, symbol, updated_at_micros",
        )
        .bind(&org_id.0)
        .bind(currency.code.as_str())
        .bind(&currency.name)
        .bind(&currency.symbol)
        .bind(currency.updated_at.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;

        currency_row(row)
    }

    async fn delete(&self, org_id: &Id, code: Currency) -> Result<bool> {
        let deleted = sqlx::query("DELETE FROM agent_currencies WHERE org_id = $1 AND code = $2")
            .bind(&org_id.0)
            .bind(code.as_str())
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(deleted.rows_affected() > 0)
    }
}
