// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `search_jobs` 表 Pg 实装（spec search-jobs capability）。
//!
//! 状态机：pending → running → done | failed。
//! TTL 由 expires_at_micros 控制；后台 cleanup task 扫超时行 + 删 result Parquet。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgPool, Row, types::Json};

use super::super::sqlx_err;
use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchJobState {
    Pending,
    Running,
    Done,
    Failed,
}

impl SearchJobState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "running" => Self::Running,
            "done" => Self::Done,
            "failed" => Self::Failed,
            _ => Self::Pending,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchJob {
    pub id: Id,
    pub org_id: Id,
    pub user_id: Id,
    pub request_json: Value,
    /// Bounded diagnostic correlation only; consumers must never derive auth from it.
    #[serde(default)]
    pub trace_link: Option<crate::shared::trace_context::SerializedTraceLink>,
    pub state: SearchJobState,
    pub result_object_key: Option<String>,
    pub result_rows: Option<i64>,
    pub error: Option<String>,
    pub submitted_at: TimestampMicros,
    pub started_at: Option<TimestampMicros>,
    pub finished_at: Option<TimestampMicros>,
    pub expires_at: TimestampMicros,
}

#[async_trait]
pub trait SearchJobRepository: Send + Sync {
    async fn create(&self, j: SearchJob) -> Result<SearchJob>;
    async fn get(&self, org_id: &Id, id: &Id) -> Result<SearchJob>;
    async fn list(&self, org_id: &Id, limit: i64) -> Result<Vec<SearchJob>>;
    /// 抢占下一条 pending job：UPDATE ... RETURNING（atomic claim）。
    async fn claim_next_pending(&self) -> Result<Option<SearchJob>>;
    async fn mark_done(
        &self,
        id: &Id,
        result_object_key: &str,
        result_rows: i64,
        finished_at: TimestampMicros,
    ) -> Result<()>;
    async fn mark_failed(&self, id: &Id, error: &str, finished_at: TimestampMicros) -> Result<()>;
    /// 列出 expires_at < cutoff 的过期 job（cleanup 用）。
    async fn list_expired(&self, cutoff: TimestampMicros, limit: i64) -> Result<Vec<SearchJob>>;
    async fn delete(&self, id: &Id) -> Result<()>;
}

pub struct PgSearchJobRepository {
    pool: PgPool,
}

impl PgSearchJobRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, user_id, request_json, state, result_object_key, result_rows,
                    error, submitted_at_micros, started_at_micros, finished_at_micros,
                    expires_at_micros, trace_link";

fn row_to(r: sqlx::postgres::PgRow) -> SearchJob {
    let req: Json<Value> = r.try_get("request_json").unwrap_or(Json(Value::Null));
    SearchJob {
        id: Id(r.try_get::<String, _>("id").unwrap_or_default()),
        org_id: Id(r.try_get::<String, _>("org_id").unwrap_or_default()),
        user_id: Id(r.try_get::<String, _>("user_id").unwrap_or_default()),
        request_json: req.0,
        trace_link: r
            .try_get::<Option<Json<crate::shared::trace_context::SerializedTraceLink>>, _>(
                "trace_link",
            )
            .unwrap_or_default()
            .map(|link| link.0),
        state: SearchJobState::parse(&r.try_get::<String, _>("state").unwrap_or_default()),
        result_object_key: r
            .try_get::<Option<String>, _>("result_object_key")
            .unwrap_or_default(),
        result_rows: r
            .try_get::<Option<i64>, _>("result_rows")
            .unwrap_or_default(),
        error: r.try_get::<Option<String>, _>("error").unwrap_or_default(),
        submitted_at: TimestampMicros(
            r.try_get::<i64, _>("submitted_at_micros")
                .unwrap_or_default(),
        ),
        started_at: r
            .try_get::<Option<i64>, _>("started_at_micros")
            .unwrap_or_default()
            .map(TimestampMicros),
        finished_at: r
            .try_get::<Option<i64>, _>("finished_at_micros")
            .unwrap_or_default()
            .map(TimestampMicros),
        expires_at: TimestampMicros(r.try_get::<i64, _>("expires_at_micros").unwrap_or_default()),
    }
}

#[async_trait]
impl SearchJobRepository for PgSearchJobRepository {
    async fn create(&self, j: SearchJob) -> Result<SearchJob> {
        sqlx::query(
            "INSERT INTO search_jobs
                (id, org_id, user_id, request_json, trace_link, state,
                 submitted_at_micros, expires_at_micros)
             VALUES ($1, $2, $3, $4, $5, 'pending', $6, $7)",
        )
        .bind(&j.id.0)
        .bind(&j.org_id.0)
        .bind(&j.user_id.0)
        .bind(Json(&j.request_json))
        .bind(j.trace_link.as_ref().map(Json))
        .bind(j.submitted_at.0)
        .bind(j.expires_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(j)
    }

    async fn get(&self, org_id: &Id, id: &Id) -> Result<SearchJob> {
        let sql = format!("SELECT {COLS} FROM search_jobs WHERE org_id = $1 AND id = $2");
        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(row_to(row))
    }

    async fn list(&self, org_id: &Id, limit: i64) -> Result<Vec<SearchJob>> {
        let sql = format!(
            "SELECT {COLS} FROM search_jobs WHERE org_id = $1
             ORDER BY submitted_at_micros DESC LIMIT $2"
        );
        let rows = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(rows.into_iter().map(row_to).collect())
    }

    async fn claim_next_pending(&self) -> Result<Option<SearchJob>> {
        // Postgres FOR UPDATE SKIP LOCKED 让多 worker 安全抢占
        let sql = format!(
            "UPDATE search_jobs SET state = 'running', started_at_micros = $1
             WHERE id = (
                 SELECT id FROM search_jobs
                 WHERE state = 'pending'
                 ORDER BY submitted_at_micros ASC
                 LIMIT 1
                 FOR UPDATE SKIP LOCKED
             )
             RETURNING {COLS}"
        );
        let row = sqlx::query(&sql)
            .bind(TimestampMicros::now().0)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(row.map(row_to))
    }

    async fn mark_done(
        &self,
        id: &Id,
        result_object_key: &str,
        result_rows: i64,
        finished_at: TimestampMicros,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE search_jobs
             SET state = 'done', result_object_key = $2, result_rows = $3, finished_at_micros = $4
             WHERE id = $1",
        )
        .bind(&id.0)
        .bind(result_object_key)
        .bind(result_rows)
        .bind(finished_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }

    async fn mark_failed(&self, id: &Id, error: &str, finished_at: TimestampMicros) -> Result<()> {
        sqlx::query(
            "UPDATE search_jobs
             SET state = 'failed', error = $2, finished_at_micros = $3
             WHERE id = $1",
        )
        .bind(&id.0)
        .bind(error)
        .bind(finished_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }

    async fn list_expired(&self, cutoff: TimestampMicros, limit: i64) -> Result<Vec<SearchJob>> {
        let sql = format!("SELECT {COLS} FROM search_jobs WHERE expires_at_micros < $1 LIMIT $2");
        let rows = sqlx::query(&sql)
            .bind(cutoff.0)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(rows.into_iter().map(row_to).collect())
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM search_jobs WHERE id = $1")
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_roundtrip() {
        for s in [
            SearchJobState::Pending,
            SearchJobState::Running,
            SearchJobState::Done,
            SearchJobState::Failed,
        ] {
            assert_eq!(SearchJobState::parse(s.as_str()), s);
        }
        // 未知 fallback 到 Pending
        assert_eq!(SearchJobState::parse("garbage"), SearchJobState::Pending);
    }
}
