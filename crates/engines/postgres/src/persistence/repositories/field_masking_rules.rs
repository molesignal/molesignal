// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 组织级字段遮掩规则的 PostgreSQL repository。

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::{
    sqlx_err,
    streams::{stream_type_from_str, stream_type_to_str},
};
use crate::{
    domain::masking::{FieldMaskingAlgorithm, FieldMaskingRule, FieldMaskingRuleRepository},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct PgFieldMaskingRuleRepository {
    pool: PgPool,
}

impl PgFieldMaskingRuleRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, name, priority, enabled, field_pattern, stream_pattern, stream_type, algorithm, created_at_micros, updated_at_micros";

fn row_to_rule(row: sqlx::postgres::PgRow) -> Result<FieldMaskingRule> {
    let stream_type = row
        .try_get::<Option<String>, _>("stream_type")
        .map_err(sqlx_err)?
        .map(|value| stream_type_from_str(&value))
        .transpose()?;
    let algorithm: Json<FieldMaskingAlgorithm> = row.try_get("algorithm").map_err(sqlx_err)?;
    Ok(FieldMaskingRule {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        org_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        name: row.try_get("name").map_err(sqlx_err)?,
        priority: row.try_get("priority").map_err(sqlx_err)?,
        enabled: row.try_get("enabled").map_err(sqlx_err)?,
        field_pattern: row.try_get("field_pattern").map_err(sqlx_err)?,
        stream_pattern: row.try_get("stream_pattern").map_err(sqlx_err)?,
        stream_type,
        algorithm: algorithm.0,
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

#[async_trait]
impl FieldMaskingRuleRepository for PgFieldMaskingRuleRepository {
    async fn list(&self, org_id: &Id) -> Result<Vec<FieldMaskingRule>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM field_masking_rules WHERE org_id = $1 ORDER BY priority ASC, created_at_micros ASC"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(row_to_rule).collect()
    }

    async fn create(&self, mut rule: FieldMaskingRule) -> Result<FieldMaskingRule> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
            .bind(&rule.org_id.0)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        let maximum_priority = sqlx::query_scalar::<Option<i32>>(
            "SELECT MAX(priority) FROM field_masking_rules WHERE org_id = $1",
        )
        .bind(&rule.org_id.0)
        .fetch_one(&mut *tx)
        .await
        .map_err(sqlx_err)?
        .unwrap_or(-1);
        rule.priority = maximum_priority
            .checked_add(1)
            .ok_or_else(|| Error::invalid("field masking rule priority overflow"))?;
        let result = sqlx::query(
            "INSERT INTO field_masking_rules
             (id, org_id, name, priority, enabled, field_pattern, stream_pattern, stream_type,
              algorithm, created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .bind(&rule.id.0)
        .bind(&rule.org_id.0)
        .bind(&rule.name)
        .bind(rule.priority)
        .bind(rule.enabled)
        .bind(&rule.field_pattern)
        .bind(&rule.stream_pattern)
        .bind(rule.stream_type.map(stream_type_to_str))
        .bind(Json(&rule.algorithm))
        .bind(rule.created_at.0)
        .bind(rule.updated_at.0)
        .execute(&mut *tx)
        .await;
        if let Err(error) = result {
            if let sqlx::Error::Database(database_error) = &error
                && database_error.code().as_deref() == Some("23505")
            {
                return Err(Error::conflict("field masking rule name already exists"));
            }
            return Err(sqlx_err(error));
        }
        tx.commit().await.map_err(sqlx_err)?;
        Ok(rule)
    }

    async fn update(&self, rule: FieldMaskingRule) -> Result<FieldMaskingRule> {
        let result = sqlx::query(
            "UPDATE field_masking_rules
             SET name = $3, priority = $4, enabled = $5, field_pattern = $6,
                 stream_pattern = $7, stream_type = $8, algorithm = $9, updated_at_micros = $10
             WHERE org_id = $1 AND id = $2",
        )
        .bind(&rule.org_id.0)
        .bind(&rule.id.0)
        .bind(&rule.name)
        .bind(rule.priority)
        .bind(rule.enabled)
        .bind(&rule.field_pattern)
        .bind(&rule.stream_pattern)
        .bind(rule.stream_type.map(stream_type_to_str))
        .bind(Json(&rule.algorithm))
        .bind(rule.updated_at.0)
        .execute(&self.pool)
        .await;
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                if let sqlx::Error::Database(database_error) = &error
                    && database_error.code().as_deref() == Some("23505")
                {
                    return Err(Error::conflict("field masking rule name already exists"));
                }
                return Err(sqlx_err(error));
            }
        };
        if result.rows_affected() == 0 {
            return Err(Error::not_found("field masking rule"));
        }
        Ok(rule)
    }

    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()> {
        let result = sqlx::query("DELETE FROM field_masking_rules WHERE org_id = $1 AND id = $2")
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        if result.rows_affected() == 0 {
            return Err(Error::not_found("field masking rule"));
        }
        Ok(())
    }

    async fn reorder(&self, org_id: &Id, ids: &[Id], now: TimestampMicros) -> Result<()> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
            .bind(&org_id.0)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        let existing = sqlx::query_scalar::<String>(
            "SELECT id FROM field_masking_rules WHERE org_id = $1 ORDER BY priority FOR UPDATE",
        )
        .bind(&org_id.0)
        .fetch_all(&mut *tx)
        .await
        .map_err(sqlx_err)?;
        let requested = ids
            .iter()
            .map(|id| id.0.as_str())
            .collect::<std::collections::HashSet<_>>();
        if existing.len() != ids.len()
            || requested.len() != ids.len()
            || existing.iter().any(|id| !requested.contains(id.as_str()))
        {
            return Err(Error::invalid(
                "reorder must include every field masking rule",
            ));
        }
        let maximum_priority = sqlx::query_scalar::<Option<i32>>(
            "SELECT MAX(priority) FROM field_masking_rules WHERE org_id = $1",
        )
        .bind(&org_id.0)
        .fetch_one(&mut *tx)
        .await
        .map_err(sqlx_err)?
        .unwrap_or(-1);
        let offset = maximum_priority
            .checked_add(1)
            .ok_or_else(|| Error::invalid("field masking rule priority overflow"))?;
        maximum_priority
            .checked_add(offset)
            .ok_or_else(|| Error::invalid("field masking rule priority overflow"))?;
        sqlx::query("UPDATE field_masking_rules SET priority = priority + $2 WHERE org_id = $1")
            .bind(&org_id.0)
            .bind(offset)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        for (priority, id) in ids.iter().enumerate() {
            let result = sqlx::query(
                "UPDATE field_masking_rules SET priority = $3, updated_at_micros = $4
                 WHERE org_id = $1 AND id = $2",
            )
            .bind(&org_id.0)
            .bind(&id.0)
            .bind(priority as i32)
            .bind(now.0)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
            if result.rows_affected() != 1 {
                return Err(Error::invalid(
                    "reorder contains an unknown field masking rule",
                ));
            }
        }
        tx.commit().await.map_err(sqlx_err)
    }
}
