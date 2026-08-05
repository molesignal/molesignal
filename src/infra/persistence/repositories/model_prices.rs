// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `model_prices` 表 Pg 实装（spec Model pricing）。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

use super::sqlx_err;
use crate::shared::{Result, time::TimestampMicros};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPrice {
    pub provider: String,
    pub model: String,
    pub prompt_usd_per_1k: f64,
    pub completion_usd_per_1k: f64,
    pub updated_at: TimestampMicros,
}

#[async_trait]
pub trait ModelPriceRepository: Send + Sync {
    async fn upsert(&self, p: ModelPrice) -> Result<ModelPrice>;
    async fn get(&self, provider: &str, model: &str) -> Result<Option<ModelPrice>>;
    async fn list(&self) -> Result<Vec<ModelPrice>>;
    async fn delete(&self, provider: &str, model: &str) -> Result<()>;
}

pub struct PgModelPriceRepository {
    pool: PgPool,
}

impl PgModelPriceRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn row_to(r: sqlx::postgres::PgRow) -> ModelPrice {
    ModelPrice {
        provider: r.try_get::<String, _>("provider").unwrap_or_default(),
        model: r.try_get::<String, _>("model").unwrap_or_default(),
        prompt_usd_per_1k: r.try_get::<f64, _>("prompt_usd_per_1k").unwrap_or_default(),
        completion_usd_per_1k: r
            .try_get::<f64, _>("completion_usd_per_1k")
            .unwrap_or_default(),
        updated_at: TimestampMicros(r.try_get::<i64, _>("updated_at_micros").unwrap_or_default()),
    }
}

#[async_trait]
impl ModelPriceRepository for PgModelPriceRepository {
    async fn upsert(&self, p: ModelPrice) -> Result<ModelPrice> {
        sqlx::query(
            "INSERT INTO model_prices
                (provider, model, prompt_usd_per_1k, completion_usd_per_1k, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (provider, model) DO UPDATE
             SET prompt_usd_per_1k = EXCLUDED.prompt_usd_per_1k,
                 completion_usd_per_1k = EXCLUDED.completion_usd_per_1k,
                 updated_at_micros = EXCLUDED.updated_at_micros",
        )
        .bind(&p.provider)
        .bind(&p.model)
        .bind(p.prompt_usd_per_1k)
        .bind(p.completion_usd_per_1k)
        .bind(p.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(p)
    }

    async fn get(&self, provider: &str, model: &str) -> Result<Option<ModelPrice>> {
        let row = sqlx::query(
            "SELECT provider, model, prompt_usd_per_1k, completion_usd_per_1k, updated_at_micros
             FROM model_prices WHERE provider = $1 AND model = $2",
        )
        .bind(provider)
        .bind(model)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(row.map(row_to))
    }

    async fn list(&self) -> Result<Vec<ModelPrice>> {
        let rows = sqlx::query(
            "SELECT provider, model, prompt_usd_per_1k, completion_usd_per_1k, updated_at_micros
             FROM model_prices ORDER BY provider, model",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(rows.into_iter().map(row_to).collect())
    }

    async fn delete(&self, provider: &str, model: &str) -> Result<()> {
        sqlx::query("DELETE FROM model_prices WHERE provider = $1 AND model = $2")
            .bind(provider)
            .bind(model)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(())
    }
}

/// 给定 prompt / completion token 数计算 USD（与  crate `compute_cost` 一致）。
pub fn compute_cost_usd(p: &ModelPrice, prompt_tokens: i64, completion_tokens: i64) -> f64 {
    let prompt = (prompt_tokens as f64) / 1000.0 * p.prompt_usd_per_1k;
    let completion = (completion_tokens as f64) / 1000.0 * p.completion_usd_per_1k;
    prompt + completion
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_matches_spec_example() {
        // gpt-4o: $0.005/1k prompt + $0.015/1k completion
        // 1500 prompt + 500 completion → 0.005*1.5 + 0.015*0.5 = 0.0075 + 0.0075 = 0.015
        let p = ModelPrice {
            provider: "openai".into(),
            model: "gpt-4o".into(),
            prompt_usd_per_1k: 0.005,
            completion_usd_per_1k: 0.015,
            updated_at: TimestampMicros(0),
        };
        let cost = compute_cost_usd(&p, 1500, 500);
        assert!((cost - 0.015).abs() < 1e-9, "got {cost}");
    }

    /// Spec scenario "Known model yields non-zero cost"（task 4.3）：
    /// 100 prompt + 200 completion → 0.005 * 0.1 + 0.015 * 0.2 = 0.0005 + 0.003 = 0.0035。
    #[test]
    fn cost_matches_change_spec_scenario_for_gpt_4o() {
        let p = ModelPrice {
            provider: "openai".into(),
            model: "gpt-4o".into(),
            prompt_usd_per_1k: 0.005,
            completion_usd_per_1k: 0.015,
            updated_at: TimestampMicros(0),
        };
        let cost = compute_cost_usd(&p, 100, 200);
        assert!((cost - 0.0035).abs() < 1e-9, "got {cost}");
    }
}
