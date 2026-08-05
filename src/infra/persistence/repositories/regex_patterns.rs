// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `regex_patterns` 表 Pg 实装（backend-settings-endpoints regex-patterns）。
//!
//! Org-scoped VRL regex pattern shortcuts。pattern 体 round-trip 字节级一致。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

use super::sqlx_err;
use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegexPattern {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub pattern: String,
    pub description: String,
    /// 命中片段替换成的串；支持 `$1` 捕获组回引（masking）。
    #[serde(default = "default_replacement")]
    pub replacement: String,
    /// 写入前对所有字符串值做不可逆脱敏；off 时仅查询端 `mask(col)` 应用此规则。
    #[serde(default)]
    pub apply_on_ingest: bool,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

fn default_replacement() -> String {
    "[REDACTED]".to_string()
}

#[async_trait]
pub trait RegexPatternRepository: Send + Sync {
    async fn list(&self, org_id: &Id) -> Result<Vec<RegexPattern>>;
    async fn create(&self, p: RegexPattern) -> Result<RegexPattern>;
    async fn update(&self, p: RegexPattern) -> Result<RegexPattern>;
    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()>;
}

pub struct PgRegexPatternRepository {
    pool: PgPool,
}

impl PgRegexPatternRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, name, pattern, description, replacement, apply_on_ingest, created_at_micros, updated_at_micros";

fn row_to(r: sqlx::postgres::PgRow) -> RegexPattern {
    RegexPattern {
        id: Id(r.try_get::<String, _>("id").unwrap_or_default()),
        org_id: Id(r.try_get::<String, _>("org_id").unwrap_or_default()),
        name: r.try_get::<String, _>("name").unwrap_or_default(),
        pattern: r.try_get::<String, _>("pattern").unwrap_or_default(),
        description: r.try_get::<String, _>("description").unwrap_or_default(),
        replacement: r
            .try_get::<String, _>("replacement")
            .unwrap_or_else(|_| default_replacement()),
        apply_on_ingest: r.try_get::<bool, _>("apply_on_ingest").unwrap_or(false),
        created_at: TimestampMicros(r.try_get::<i64, _>("created_at_micros").unwrap_or_default()),
        updated_at: TimestampMicros(r.try_get::<i64, _>("updated_at_micros").unwrap_or_default()),
    }
}

#[async_trait]
impl RegexPatternRepository for PgRegexPatternRepository {
    async fn list(&self, org_id: &Id) -> Result<Vec<RegexPattern>> {
        let sql = format!("SELECT {COLS} FROM regex_patterns WHERE org_id = $1 ORDER BY name ASC");
        let rows = sqlx::query(&sql)
            .bind(&org_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(rows.into_iter().map(row_to).collect())
    }

    async fn create(&self, p: RegexPattern) -> Result<RegexPattern> {
        sqlx::query(
            "INSERT INTO regex_patterns
                (id, org_id, name, pattern, description, replacement, apply_on_ingest,
                 created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(&p.id.0)
        .bind(&p.org_id.0)
        .bind(&p.name)
        .bind(&p.pattern)
        .bind(&p.description)
        .bind(&p.replacement)
        .bind(p.apply_on_ingest)
        .bind(p.created_at.0)
        .bind(p.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(p)
    }

    async fn update(&self, p: RegexPattern) -> Result<RegexPattern> {
        let res = sqlx::query(
            "UPDATE regex_patterns
                SET name = $3, pattern = $4, description = $5,
                    replacement = $6, apply_on_ingest = $7, updated_at_micros = $8
             WHERE org_id = $1 AND id = $2",
        )
        .bind(&p.org_id.0)
        .bind(&p.id.0)
        .bind(&p.name)
        .bind(&p.pattern)
        .bind(&p.description)
        .bind(&p.replacement)
        .bind(p.apply_on_ingest)
        .bind(p.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        if res.rows_affected() == 0 {
            return Err(crate::shared::Error::not_found("regex pattern"));
        }
        Ok(p)
    }

    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM regex_patterns WHERE org_id = $1 AND id = $2")
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(())
    }
}
