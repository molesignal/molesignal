// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `log_patterns` 表 Pg 实装（spec log-patterns）。
//!
//! 当前实装：CRUD + regex 编译验证。DataFusion UDF `extract_pattern(message)`
//! 注册留 follow-up（需 DataFusion `SessionContext::register_udf` + 跨 stream
//! schema 关联）。`vectorscan` 加速也是后续 feature。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogPattern {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub regex: String,
    pub capture_groups: Vec<String>,
    pub category: String,
    pub priority: i32,
    pub stream_filter: Option<String>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[async_trait]
pub trait LogPatternRepository: Send + Sync {
    async fn create(&self, p: LogPattern) -> Result<LogPattern>;
    async fn update(&self, p: LogPattern) -> Result<LogPattern>;
    async fn get(&self, org_id: &Id, id: &Id) -> Result<LogPattern>;
    async fn list(&self, org_id: &Id) -> Result<Vec<LogPattern>>;
    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()>;
}

pub struct PgLogPatternRepository {
    pool: PgPool,
}

impl PgLogPatternRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, name, regex, capture_groups, category, priority,
                    stream_filter, created_at_micros, updated_at_micros";

fn row_to(r: sqlx::postgres::PgRow) -> LogPattern {
    let groups: Json<Vec<String>> = r.try_get("capture_groups").unwrap_or(Json(Vec::new()));
    LogPattern {
        id: Id(r.try_get::<String, _>("id").unwrap_or_default()),
        org_id: Id(r.try_get::<String, _>("org_id").unwrap_or_default()),
        name: r.try_get::<String, _>("name").unwrap_or_default(),
        regex: r.try_get::<String, _>("regex").unwrap_or_default(),
        capture_groups: groups.0,
        category: r.try_get::<String, _>("category").unwrap_or_default(),
        priority: r.try_get::<i32, _>("priority").unwrap_or_default(),
        stream_filter: r
            .try_get::<Option<String>, _>("stream_filter")
            .unwrap_or_default(),
        created_at: TimestampMicros(r.try_get::<i64, _>("created_at_micros").unwrap_or_default()),
        updated_at: TimestampMicros(r.try_get::<i64, _>("updated_at_micros").unwrap_or_default()),
    }
}

#[async_trait]
impl LogPatternRepository for PgLogPatternRepository {
    async fn create(&self, p: LogPattern) -> Result<LogPattern> {
        sqlx::query(
            "INSERT INTO log_patterns
                (id, org_id, name, regex, capture_groups, category, priority,
                 stream_filter, created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(&p.id.0)
        .bind(&p.org_id.0)
        .bind(&p.name)
        .bind(&p.regex)
        .bind(Json(&p.capture_groups))
        .bind(&p.category)
        .bind(p.priority)
        .bind(&p.stream_filter)
        .bind(p.created_at.0)
        .bind(p.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(p)
    }

    async fn update(&self, p: LogPattern) -> Result<LogPattern> {
        sqlx::query(
            "UPDATE log_patterns SET
                name = $3, regex = $4, capture_groups = $5, category = $6,
                priority = $7, stream_filter = $8, updated_at_micros = $9
             WHERE id = $1 AND org_id = $2",
        )
        .bind(&p.id.0)
        .bind(&p.org_id.0)
        .bind(&p.name)
        .bind(&p.regex)
        .bind(Json(&p.capture_groups))
        .bind(&p.category)
        .bind(p.priority)
        .bind(&p.stream_filter)
        .bind(p.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(p)
    }

    async fn get(&self, org_id: &Id, id: &Id) -> Result<LogPattern> {
        let sql = format!("SELECT {COLS} FROM log_patterns WHERE org_id = $1 AND id = $2");
        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(row_to(row))
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<LogPattern>> {
        let sql = format!(
            "SELECT {COLS} FROM log_patterns WHERE org_id = $1 ORDER BY priority DESC, name"
        );
        let rows = sqlx::query(&sql)
            .bind(&org_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(rows.into_iter().map(row_to).collect())
    }

    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM log_patterns WHERE org_id = $1 AND id = $2")
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(())
    }
}

/// 编译验证（POST 时同步调用）：`regex::Regex::new` 抛错 → handler 400。
pub fn compile_check(regex: &str) -> Result<()> {
    use crate::shared::Error;
    regex::Regex::new(regex)
        .map(|_| ())
        .map_err(|e| Error::invalid(format!("regex parse error: {e}")))
}

/// 按 `priority` DESC 评估：返回首个匹配 pattern 的 category（无匹配 → None）。
/// 此函数用于运行时 SQL UDF / pipeline hook（暂未对接）。
pub fn first_match<'a>(patterns: &'a [LogPattern], message: &str) -> Option<&'a str> {
    for p in patterns {
        if let Ok(re) = regex::Regex::new(&p.regex)
            && re.is_match(message)
        {
            return Some(&p.category);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(re: &str, cat: &str, prio: i32) -> LogPattern {
        LogPattern {
            id: Id::new(),
            org_id: Id("o".into()),
            name: cat.into(),
            regex: re.into(),
            capture_groups: vec![],
            category: cat.into(),
            priority: prio,
            stream_filter: None,
            created_at: TimestampMicros::now(),
            updated_at: TimestampMicros::now(),
        }
    }

    #[test]
    fn bad_regex_returns_error() {
        assert!(compile_check("[invalid(").is_err());
    }

    #[test]
    fn good_regex_passes() {
        assert!(compile_check(r"error|warn|info").is_ok());
    }

    #[test]
    fn first_match_respects_priority_order() {
        // 已按 priority DESC 入参（调用方在 list 时排）
        let pats = vec![p(r"panic", "panic", 100), p(r".*", "other", 1)];
        assert_eq!(first_match(&pats, "panic at the disco"), Some("panic"));
        assert_eq!(first_match(&pats, "just some text"), Some("other"));
        assert_eq!(first_match(&[], "anything"), None);
    }
}
