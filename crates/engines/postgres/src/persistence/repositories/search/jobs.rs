// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `search_jobs` 表 Pg 实装（spec search-jobs capability）。
//!
//! 状态机：pending → running → done | failed | cancelled；failed/cancelled → pending（retry）。
//! TTL 由 expires_at_micros 控制；后台 cleanup task 扫超时行 + 删 NDJSON result。

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgPool, Row, types::Json};
use tokio::{sync::Notify, task::JoinHandle};

use super::super::sqlx_err;
use crate::shared::{Result, ids::Id, time::TimestampMicros};

const PENDING_CHANNEL: &str = "molesignal_search_jobs_pending";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchJobState {
    Pending,
    Running,
    Done,
    Failed,
    Cancelled,
}

impl SearchJobState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "running" => Self::Running,
            "done" => Self::Done,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => Self::Pending,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchJob {
    pub id: Id,
    pub org_id: Id,
    pub user_id: Id,
    /// Query admission work-group captured from the authenticated IAM context at submission time.
    /// It is scheduling metadata only and MUST NOT be used to reconstruct authorization.
    pub role_key: String,
    pub request_json: Value,
    /// Bounded diagnostic correlation only; consumers must never derive auth from it.
    #[serde(default)]
    pub trace_link: Option<crate::shared::trace_context::SerializedTraceLink>,
    pub state: SearchJobState,
    /// Monotonic execution generation. Retry increments this so a stale worker cannot commit.
    pub attempt: i32,
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
    /// Wait for a local/cross-node enqueue notification, retaining timeout polling as a safety net.
    async fn wait_for_pending(&self, timeout: Duration) -> bool;
    /// Complete the matching running attempt. Returns false when cancel/retry already won.
    async fn mark_done(
        &self,
        org_id: &Id,
        id: &Id,
        attempt: i32,
        result_object_key: &str,
        result_rows: i64,
        finished_at: TimestampMicros,
    ) -> Result<bool>;
    /// Fail the matching running attempt. Returns false when cancel/retry already won.
    async fn mark_failed(
        &self,
        org_id: &Id,
        id: &Id,
        attempt: i32,
        error: &str,
        finished_at: TimestampMicros,
    ) -> Result<bool>;
    async fn cancel(&self, org_id: &Id, id: &Id, finished_at: TimestampMicros)
    -> Result<SearchJob>;
    async fn retry(
        &self,
        org_id: &Id,
        id: &Id,
        submitted_at: TimestampMicros,
        expires_at: TimestampMicros,
    ) -> Result<SearchJob>;
    /// 列出 expires_at < cutoff 的过期 job（cleanup 用）。
    async fn list_expired(&self, cutoff: TimestampMicros, limit: i64) -> Result<Vec<SearchJob>>;
    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()>;
}

pub struct PgSearchJobRepository {
    pool: PgPool,
    wakeup: Arc<Notify>,
}

impl PgSearchJobRepository {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            wakeup: Arc::new(Notify::new()),
        }
    }

    /// Listen on a dedicated PostgreSQL connection so an idle Search Job scheduler does not pin a
    /// connection from the shared metadata pool. `PgListener` re-subscribes after reconnects; the
    /// outer loop covers initial connection and terminal listener failures with bounded backoff.
    pub fn spawn_notification_listener(self: Arc<Self>, dsn: String) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut failures = 0_u32;
            loop {
                match sqlx::postgres::PgListener::connect(&dsn).await {
                    Ok(mut listener) => {
                        if let Err(error) = listener.listen(PENDING_CHANNEL).await {
                            failures = failures.saturating_add(1);
                            tracing::warn!(
                                %error,
                                "search job notification listener subscription failed"
                            );
                        } else {
                            tracing::info!(
                                channel = PENDING_CHANNEL,
                                "search job notification listener started"
                            );
                            loop {
                                match listener.recv().await {
                                    Ok(_) => {
                                        failures = 0;
                                        self.wakeup.notify_one();
                                    }
                                    Err(error) => {
                                        failures = failures.saturating_add(1);
                                        tracing::warn!(
                                            %error,
                                            "search job notification listener disconnected"
                                        );
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    Err(_) => {
                        failures = failures.saturating_add(1);
                        tracing::warn!("search job notification listener connect failed");
                    }
                }
                tokio::time::sleep(notification_reconnect_delay(failures)).await;
            }
        })
    }
}

fn notification_reconnect_delay(failures: u32) -> Duration {
    let shift = failures.saturating_sub(1).min(6);
    let cap_millis = 1_000_u64.saturating_mul(1_u64 << shift).min(60_000);
    let jitter_seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64;
    let lower = cap_millis.saturating_mul(4) / 5;
    let width = cap_millis.saturating_mul(2) / 5;
    Duration::from_millis(lower.saturating_add(jitter_seed % width.max(1)))
}

const COLS: &str = "id, org_id, user_id, role_key, request_json, state, attempt,
                    result_object_key, result_rows, error, submitted_at_micros,
                    started_at_micros, finished_at_micros, expires_at_micros, trace_link";

fn row_to(r: sqlx::postgres::PgRow) -> SearchJob {
    let req: Json<Value> = r.try_get("request_json").unwrap_or(Json(Value::Null));
    SearchJob {
        id: Id(r.try_get::<String, _>("id").unwrap_or_default()),
        org_id: Id(r.try_get::<String, _>("org_id").unwrap_or_default()),
        user_id: Id(r.try_get::<String, _>("user_id").unwrap_or_default()),
        role_key: r.try_get::<String, _>("role_key").unwrap_or_default(),
        request_json: req.0,
        trace_link: r
            .try_get::<Option<Json<crate::shared::trace_context::SerializedTraceLink>>, _>(
                "trace_link",
            )
            .unwrap_or_default()
            .map(|link| link.0),
        state: SearchJobState::parse(&r.try_get::<String, _>("state").unwrap_or_default()),
        attempt: r.try_get::<i32, _>("attempt").unwrap_or_default(),
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
            "WITH inserted AS (
                 INSERT INTO search_jobs
                    (id, org_id, user_id, role_key, request_json, trace_link, state,
                     submitted_at_micros, expires_at_micros)
                 VALUES ($1, $2, $3, $4, $5, $6, 'pending', $7, $8)
                 RETURNING 1
             )
             SELECT pg_notify($9, '') AS pending_notification FROM inserted",
        )
        .bind(&j.id.0)
        .bind(&j.org_id.0)
        .bind(&j.user_id.0)
        .bind(&j.role_key)
        .bind(Json(&j.request_json))
        .bind(j.trace_link.as_ref().map(Json))
        .bind(j.submitted_at.0)
        .bind(j.expires_at.0)
        .bind(PENDING_CHANNEL)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;
        self.wakeup.notify_one();
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

    async fn wait_for_pending(&self, timeout: Duration) -> bool {
        tokio::select! {
            _ = self.wakeup.notified() => true,
            _ = tokio::time::sleep(timeout) => false,
        }
    }

    async fn mark_done(
        &self,
        org_id: &Id,
        id: &Id,
        attempt: i32,
        result_object_key: &str,
        result_rows: i64,
        finished_at: TimestampMicros,
    ) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE search_jobs
             SET state = 'done', result_object_key = $3, result_rows = $4,
                 error = NULL, finished_at_micros = $5
             WHERE org_id = $1 AND id = $2 AND attempt = $6 AND state = 'running'",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .bind(result_object_key)
        .bind(result_rows)
        .bind(finished_at.0)
        .bind(attempt)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(result.rows_affected() == 1)
    }

    async fn mark_failed(
        &self,
        org_id: &Id,
        id: &Id,
        attempt: i32,
        error: &str,
        finished_at: TimestampMicros,
    ) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE search_jobs
             SET state = 'failed', error = $3, finished_at_micros = $4
             WHERE org_id = $1 AND id = $2 AND attempt = $5 AND state = 'running'",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .bind(error)
        .bind(finished_at.0)
        .bind(attempt)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(result.rows_affected() == 1)
    }

    async fn cancel(
        &self,
        org_id: &Id,
        id: &Id,
        finished_at: TimestampMicros,
    ) -> Result<SearchJob> {
        let sql = format!(
            "UPDATE search_jobs
             SET state = 'cancelled', error = NULL, finished_at_micros = $3
             WHERE org_id = $1 AND id = $2 AND state IN ('pending', 'running')
             RETURNING {COLS}"
        );
        if let Some(row) = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&id.0)
            .bind(finished_at.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?
        {
            return Ok(row_to(row));
        }
        let current = self.get(org_id, id).await?;
        if current.state == SearchJobState::Cancelled {
            Ok(current)
        } else {
            Err(crate::shared::Error::invalid(format!(
                "search job in state `{}` cannot be cancelled",
                current.state.as_str()
            )))
        }
    }

    async fn retry(
        &self,
        org_id: &Id,
        id: &Id,
        submitted_at: TimestampMicros,
        expires_at: TimestampMicros,
    ) -> Result<SearchJob> {
        let sql = "WITH updated AS (
                       UPDATE search_jobs
                          SET state = 'pending', attempt = attempt + 1,
                              result_object_key = NULL, result_rows = NULL, error = NULL,
                              submitted_at_micros = $3, started_at_micros = NULL,
                              finished_at_micros = NULL, expires_at_micros = $4
                        WHERE org_id = $1
                          AND id = $2
                          AND state IN ('failed', 'cancelled')
                    RETURNING *
                   )
                   SELECT updated.*, pg_notify($5, '') AS pending_notification
                     FROM updated";
        let row = sqlx::query(sql)
            .bind(&org_id.0)
            .bind(&id.0)
            .bind(submitted_at.0)
            .bind(expires_at.0)
            .bind(PENDING_CHANNEL)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?;
        match row {
            Some(row) => {
                self.wakeup.notify_one();
                Ok(row_to(row))
            }
            None => {
                let current = self.get(org_id, id).await?;
                Err(crate::shared::Error::invalid(format!(
                    "search job in state `{}` cannot be retried",
                    current.state.as_str()
                )))
            }
        }
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

    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM search_jobs WHERE org_id = $1 AND id = $2")
            .bind(&org_id.0)
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
            SearchJobState::Cancelled,
        ] {
            assert_eq!(SearchJobState::parse(s.as_str()), s);
        }
        // 未知 fallback 到 Pending
        assert_eq!(SearchJobState::parse("garbage"), SearchJobState::Pending);
    }
}
