// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `inhibit_rules` 表 Pg 实装（告警抑制）。

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::{
    domain::alerting::inhibit::{InhibitRule, InhibitRuleRepository, InhibitSource, InhibitTarget},
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub struct PgInhibitRuleRepository {
    pool: PgPool,
}

impl PgInhibitRuleRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, name, enabled, source, target, equal_labels, more_severe_only,
     wait_secs, linger_secs, comment, created_by, created_at, updated_at";

fn row_to(row: sqlx::postgres::PgRow) -> Result<InhibitRule> {
    let source: Json<InhibitSource> = row.try_get("source").map_err(sqlx_err)?;
    let target: Json<InhibitTarget> = row.try_get("target").map_err(sqlx_err)?;
    let equal: Json<Vec<String>> = row.try_get("equal_labels").map_err(sqlx_err)?;
    let wait_secs: i32 = row.try_get("wait_secs").map_err(sqlx_err)?;
    let linger_secs: i32 = row.try_get("linger_secs").map_err(sqlx_err)?;

    Ok(InhibitRule {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        org_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        name: row.try_get("name").map_err(sqlx_err)?,
        enabled: row.try_get("enabled").map_err(sqlx_err)?,
        source: source.0,
        target: target.0,
        equal: equal.0,
        more_severe_only: row.try_get("more_severe_only").map_err(sqlx_err)?,
        wait_secs: u32::try_from(wait_secs).unwrap_or_default(),
        linger_secs: u32::try_from(linger_secs).unwrap_or_default(),
        comment: row.try_get("comment").map_err(sqlx_err)?,
        created_by: row
            .try_get::<Option<String>, _>("created_by")
            .map_err(sqlx_err)?
            .map(Id::from_string),
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
        updated_at: row
            .try_get::<TimestampMicros, _>("updated_at")
            .map_err(sqlx_err)?,
    })
}

#[async_trait]
impl InhibitRuleRepository for PgInhibitRuleRepository {
    async fn create(&self, rule: InhibitRule) -> Result<InhibitRule> {
        sqlx::query(
            "INSERT INTO inhibit_rules
                (id, org_id, name, enabled, source, target, equal_labels, more_severe_only,
                 wait_secs, linger_secs, comment, created_by, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
        )
        .bind(&rule.id.0)
        .bind(&rule.org_id.0)
        .bind(&rule.name)
        .bind(rule.enabled)
        .bind(Json(&rule.source))
        .bind(Json(&rule.target))
        .bind(Json(&rule.equal))
        .bind(rule.more_severe_only)
        .bind(i32::try_from(rule.wait_secs).unwrap_or(i32::MAX))
        .bind(i32::try_from(rule.linger_secs).unwrap_or(i32::MAX))
        .bind(&rule.comment)
        .bind(rule.created_by.as_ref().map(|id| id.0.clone()))
        .bind(rule.created_at)
        .bind(rule.updated_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(rule)
    }

    async fn update(&self, rule: InhibitRule) -> Result<InhibitRule> {
        sqlx::query(
            "UPDATE inhibit_rules SET
               name = $2, enabled = $3, source = $4, target = $5, equal_labels = $6,
               more_severe_only = $7, wait_secs = $8, linger_secs = $9, comment = $10,
               updated_at = $11
             WHERE id = $1",
        )
        .bind(&rule.id.0)
        .bind(&rule.name)
        .bind(rule.enabled)
        .bind(Json(&rule.source))
        .bind(Json(&rule.target))
        .bind(Json(&rule.equal))
        .bind(rule.more_severe_only)
        .bind(i32::try_from(rule.wait_secs).unwrap_or(i32::MAX))
        .bind(i32::try_from(rule.linger_secs).unwrap_or(i32::MAX))
        .bind(&rule.comment)
        .bind(rule.updated_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(rule)
    }

    async fn get(&self, id: &Id) -> Result<InhibitRule> {
        let row = sqlx::query(&format!("SELECT {COLS} FROM inhibit_rules WHERE id = $1"))
            .bind(&id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;

        row_to(row)
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<InhibitRule>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM inhibit_rules WHERE org_id = $1 ORDER BY created_at, id"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(row_to).collect()
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM inhibit_rules WHERE id = $1")
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(())
    }

    async fn list_enabled(&self, org_id: &Id) -> Result<Vec<InhibitRule>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM inhibit_rules
             WHERE org_id = $1 AND enabled = TRUE
             ORDER BY created_at, id"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(row_to).collect()
    }
}
