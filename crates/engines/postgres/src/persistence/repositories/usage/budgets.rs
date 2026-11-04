// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::{PgPool, Row, postgres::PgRow};

use super::super::sqlx_err;
use crate::{
    domain::intake::usage::{
        budget::{BudgetPeriod, IntakeBudget, IntakeBudgetRepository},
        source::UsageDimension,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const COLUMNS: &str = "id, org_id, dimension, source_key, period, limit_size, warn_percent,
    notify_user_ids, created_by, updated_by, created_at, updated_at";

pub struct PgIntakeBudgetRepository {
    pool: PgPool,
}

impl PgIntakeBudgetRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn budget_from(row: PgRow) -> Result<IntakeBudget> {
    let dimension: String = row.try_get("dimension").map_err(sqlx_err)?;
    let period: String = row.try_get("period").map_err(sqlx_err)?;
    let limit_size: i64 = row.try_get("limit_size").map_err(sqlx_err)?;
    let warn_percent: i16 = row.try_get("warn_percent").map_err(sqlx_err)?;
    let notify_user_ids: Vec<String> = row.try_get("notify_user_ids").map_err(sqlx_err)?;

    Ok(IntakeBudget {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        org_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        dimension: UsageDimension::parse(&dimension)?,
        source_key: row.try_get("source_key").map_err(sqlx_err)?,
        period: BudgetPeriod::parse(&period)?,
        limit_size: limit_size.max(1) as u64,
        warn_percent: warn_percent.clamp(0, 99) as u8,
        notify_user_ids: notify_user_ids.into_iter().map(Id::from_string).collect(),
        created_by: Id::from_string(row.try_get::<String, _>("created_by").map_err(sqlx_err)?),
        updated_by: Id::from_string(row.try_get::<String, _>("updated_by").map_err(sqlx_err)?),
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
        updated_at: row
            .try_get::<TimestampMicros, _>("updated_at")
            .map_err(sqlx_err)?,
    })
}

#[async_trait]
impl IntakeBudgetRepository for PgIntakeBudgetRepository {
    async fn list(&self, org_id: &Id) -> Result<Vec<IntakeBudget>> {
        let statement = format!(
            "SELECT {COLUMNS} FROM intake_budgets WHERE org_id = $1
             ORDER BY dimension, source_key"
        );

        let rows = sqlx::query(&statement)
            .bind(org_id.as_str())
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;

        rows.into_iter().map(budget_from).collect()
    }

    async fn upsert(&self, budget: IntakeBudget) -> Result<IntakeBudget> {
        let notify_user_ids = budget
            .notify_user_ids
            .iter()
            .map(Id::as_str)
            .collect::<Vec<_>>();

        let statement = format!(
            "INSERT INTO intake_budgets ({COLUMNS})
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
             ON CONFLICT (org_id, dimension, source_key) DO UPDATE
             SET period = EXCLUDED.period,
                 limit_size = EXCLUDED.limit_size,
                 warn_percent = EXCLUDED.warn_percent,
                 notify_user_ids = EXCLUDED.notify_user_ids,
                 updated_by = EXCLUDED.updated_by,
                 updated_at = EXCLUDED.updated_at
             RETURNING {COLUMNS}"
        );

        let row = sqlx::query(&statement)
            .bind(budget.id.as_str())
            .bind(budget.org_id.as_str())
            .bind(budget.dimension.as_str())
            .bind(&budget.source_key)
            .bind(budget.period.as_str())
            .bind(i64::try_from(budget.limit_size).unwrap_or(i64::MAX))
            .bind(i16::from(budget.warn_percent))
            .bind(&notify_user_ids)
            .bind(budget.created_by.as_str())
            .bind(budget.updated_by.as_str())
            .bind(budget.created_at)
            .bind(budget.updated_at)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;

        budget_from(row)
    }

    async fn delete(&self, org_id: &Id, id: &Id) -> Result<IntakeBudget> {
        let statement =
            format!("DELETE FROM intake_budgets WHERE org_id = $1 AND id = $2 RETURNING {COLUMNS}");

        let row = sqlx::query(&statement)
            .bind(org_id.as_str())
            .bind(id.as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?;

        match row {
            Some(row) => budget_from(row),
            None => Err(Error::not_found(format!("intake budget `{id}`"))),
        }
    }
}
