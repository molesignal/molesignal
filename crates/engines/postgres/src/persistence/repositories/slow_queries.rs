// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `slow_queries` 表 Pg 实装：慢查询采集（按 (org, fingerprint) 去重累计）。

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use super::sqlx_err;
use crate::{
    domain::query::{QueryLanguage, SlowQuery, SlowQueryRepository},
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub struct PgSlowQueryRepository {
    pool: PgPool,
}

impl PgSlowQueryRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, fingerprint, language, statement, scanned_rows, returned_rows, \
    took_ms, time_range_secs, hit_count, first_seen_micros, last_seen_micros";

fn lang_str(l: QueryLanguage) -> &'static str {
    match l {
        QueryLanguage::Sql => "sql",
        QueryLanguage::Promql => "promql",
    }
}
fn lang_parse(s: &str) -> QueryLanguage {
    match s {
        "promql" => QueryLanguage::Promql,
        _ => QueryLanguage::Sql,
    }
}

fn row_to(r: sqlx::postgres::PgRow) -> SlowQuery {
    SlowQuery {
        id: Id(r.try_get::<String, _>("id").unwrap_or_default()),
        org_id: Id(r.try_get::<String, _>("org_id").unwrap_or_default()),
        fingerprint: r.try_get("fingerprint").unwrap_or_default(),
        language: lang_parse(&r.try_get::<String, _>("language").unwrap_or_default()),
        statement: r.try_get("statement").unwrap_or_default(),
        scanned_rows: r.try_get::<i64, _>("scanned_rows").unwrap_or_default(),
        returned_rows: r.try_get::<i64, _>("returned_rows").unwrap_or_default(),
        took_ms: r.try_get::<i64, _>("took_ms").unwrap_or_default(),
        time_range_secs: r
            .try_get::<Option<i64>, _>("time_range_secs")
            .unwrap_or_default(),
        hit_count: r.try_get::<i64, _>("hit_count").unwrap_or_default(),
        first_seen: TimestampMicros(r.try_get::<i64, _>("first_seen_micros").unwrap_or_default()),
        last_seen: TimestampMicros(r.try_get::<i64, _>("last_seen_micros").unwrap_or_default()),
    }
}

#[async_trait]
impl SlowQueryRepository for PgSlowQueryRepository {
    async fn record(&self, q: SlowQuery) -> Result<()> {
        // 同 (org, fingerprint) 已存在 → 累加 hit_count、刷新最新统计与 last_seen；
        // first_seen 保留最早值。
        sqlx::query(
            "INSERT INTO slow_queries
                (id, org_id, fingerprint, language, statement, scanned_rows, returned_rows,
                 took_ms, time_range_secs, hit_count, first_seen_micros, last_seen_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 1, $10, $10)
             ON CONFLICT (org_id, fingerprint) DO UPDATE SET
                language = EXCLUDED.language,
                statement = EXCLUDED.statement,
                scanned_rows = EXCLUDED.scanned_rows,
                returned_rows = EXCLUDED.returned_rows,
                took_ms = EXCLUDED.took_ms,
                time_range_secs = EXCLUDED.time_range_secs,
                hit_count = slow_queries.hit_count + 1,
                last_seen_micros = EXCLUDED.last_seen_micros",
        )
        .bind(&q.id.0)
        .bind(&q.org_id.0)
        .bind(&q.fingerprint)
        .bind(lang_str(q.language))
        .bind(&q.statement)
        .bind(q.scanned_rows)
        .bind(q.returned_rows)
        .bind(q.took_ms)
        .bind(q.time_range_secs)
        .bind(q.last_seen.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }

    async fn list_recent(&self, org_id: &Id, limit: i64) -> Result<Vec<SlowQuery>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM slow_queries
             WHERE org_id = $1 ORDER BY last_seen_micros DESC LIMIT $2"
        ))
        .bind(&org_id.0)
        .bind(limit.clamp(1, 500))
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(rows.into_iter().map(row_to).collect())
    }
}
