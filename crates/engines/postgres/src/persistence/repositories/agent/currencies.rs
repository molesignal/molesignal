// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 组织给模型服务单价备的币种，存在 `org_settings` 的 `agent.currencies` 一节：一个数组，列表按写入时间排。
//!
//! 组织第一次读时（或删光之后再读时）写入常用的几种，只在这一节还空着时写，同时读的两个请求不会重复写；
//! 写入时间按常用顺序错开一微秒。增改删在事务里锁住这一节再改。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, PgPool, types::Json};

use super::super::{
    org_settings::{self, sections::AGENT_CURRENCIES},
    sqlx_err,
};
use crate::{
    agent::{
        currencies::{CurrencyRepository, DEFAULT_CURRENCIES, PriceCurrency},
        usage::Currency,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// `agent.currencies` 里的一种币种。
#[derive(Serialize, Deserialize)]
struct StoredCurrency {
    code: String,
    name: Option<String>,
    symbol: Option<String>,
    created_at: TimestampMicros,
    updated_at: TimestampMicros,
}

fn currencies_of(mut stored: Vec<StoredCurrency>) -> Result<Vec<PriceCurrency>> {
    stored
        .sort_by(|left, right| (left.created_at, &left.code).cmp(&(right.created_at, &right.code)));

    stored
        .into_iter()
        .map(|item| {
            Ok(PriceCurrency {
                code: Currency::parse(&item.code)?,
                name: item.name,
                symbol: item.symbol,
                updated_at: item.updated_at,
            })
        })
        .collect()
}

pub struct PgCurrencyRepository {
    pool: PgPool,
}

impl PgCurrencyRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn stored(&self, org_id: &Id) -> Result<Vec<StoredCurrency>> {
        Ok(org_settings::read(&self.pool, &org_id.0, AGENT_CURRENCIES)
            .await?
            .map(|section| section.value)
            .unwrap_or_default())
    }
}

/// 锁住这一节读出来。还没有这一节时先建一个空的再锁，并发的第一次写入不会互相覆盖。
async fn locked(connection: &mut PgConnection, org_id: &Id) -> Result<Vec<StoredCurrency>> {
    sqlx::query(
        "INSERT INTO org_settings (org_id, section, value, updated_at)
         VALUES ($1, $2, '[]'::JSONB, $3)
         ON CONFLICT (org_id, section) DO NOTHING",
    )
    .bind(&org_id.0)
    .bind(AGENT_CURRENCIES)
    .bind(TimestampMicros::now())
    .execute(&mut *connection)
    .await
    .map_err(sqlx_err)?;

    Ok(
        org_settings::read_for_update(connection, &org_id.0, AGENT_CURRENCIES)
            .await?
            .map(|section| section.value)
            .unwrap_or_default(),
    )
}

#[async_trait]
impl CurrencyRepository for PgCurrencyRepository {
    async fn list(&self, org_id: &Id) -> Result<Vec<PriceCurrency>> {
        let stored = self.stored(org_id).await?;

        if !stored.is_empty() {
            return currencies_of(stored);
        }

        let now = TimestampMicros::now();

        let defaults: Vec<StoredCurrency> = DEFAULT_CURRENCIES
            .iter()
            .zip(0_i64..)
            .map(|(code, offset)| StoredCurrency {
                code: (*code).to_owned(),
                name: None,
                symbol: None,
                created_at: TimestampMicros(now.0 + offset),
                updated_at: now,
            })
            .collect();

        sqlx::query(
            "INSERT INTO org_settings (org_id, section, value, updated_at)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (org_id, section) DO UPDATE SET
                value = EXCLUDED.value,
                updated_at = EXCLUDED.updated_at
             WHERE org_settings.value = '[]'::JSONB",
        )
        .bind(&org_id.0)
        .bind(AGENT_CURRENCIES)
        .bind(Json(&defaults))
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        currencies_of(self.stored(org_id).await?)
    }

    async fn put(&self, org_id: &Id, currency: &PriceCurrency) -> Result<PriceCurrency> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        let mut stored = locked(&mut tx, org_id).await?;
        let code = currency.code.as_str();

        match stored.iter_mut().find(|item| item.code == code) {
            Some(existing) => {
                existing.name = currency.name.clone();
                existing.symbol = currency.symbol.clone();
                existing.updated_at = currency.updated_at;
            }
            None => stored.push(StoredCurrency {
                code: code.to_owned(),
                name: currency.name.clone(),
                symbol: currency.symbol.clone(),
                created_at: currency.updated_at,
                updated_at: currency.updated_at,
            }),
        }

        org_settings::write_in(
            &mut tx,
            &org_id.0,
            AGENT_CURRENCIES,
            &stored,
            None,
            currency.updated_at,
        )
        .await?;

        tx.commit().await.map_err(sqlx_err)?;
        Ok(currency.clone())
    }

    async fn delete(&self, org_id: &Id, code: Currency) -> Result<bool> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        let mut stored = locked(&mut tx, org_id).await?;
        let before = stored.len();

        stored.retain(|item| item.code != code.as_str());

        if stored.len() == before {
            return Ok(false);
        }

        org_settings::write_in(
            &mut tx,
            &org_id.0,
            AGENT_CURRENCIES,
            &stored,
            None,
            TimestampMicros::now(),
        )
        .await?;

        tx.commit().await.map_err(sqlx_err)?;
        Ok(true)
    }
}
