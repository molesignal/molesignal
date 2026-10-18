// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `agent_runs` 与 `agent_run_events` 的 Pg 实装。
//!
//! - 领取走 `FOR UPDATE SKIP LOCKED`；同一会话里前面还有没结束的 Run 时不领后面的。
//! - 排队与结束时 NOTIFY [`PENDING_CHANNEL`]，叫醒各节点的领取循环；追加事件、停止、回答时
//!   NOTIFY [`SIGNAL_CHANNEL`]（内容是 Run 的 ID），各节点转给自己的 [`RunSignalHub`]。同一节点
//!   不等 NOTIFY，直接提醒。
//! - 序号在 `agent_runs.last_seq` 上分配，与写事件在同一个事务里；Run 结束后不再接受事件。
//!
//! 事件的读写在 [`events`]。

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use serde_json::json;
use sqlx::{PgPool, Row, postgres::PgRow, types::Json};
use tokio::{sync::Notify, task::JoinHandle};

use super::super::search::jobs::notification_reconnect_delay;
use crate::{
    agent::{
        runs::{
            NewRunEvent, Pending, Run, RunEvent, RunEventKind, RunOutcome, RunPrincipal,
            RunRepository, RunResponse, RunSignalHub, RunSignals, RunSource, RunStatus,
        },
        usage::Cost,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
    sqlx_err,
};

mod events;

/// 有 Run 排进队列或结束了。
pub const PENDING_CHANNEL: &str = "molesignal_agent_runs_pending";
/// 某个 Run 有新事件、被要求停止或有了回答；内容是 Run 的 ID。
pub const SIGNAL_CHANNEL: &str = "molesignal_agent_run_signals";

/// 执行中的状态，SQL 里用。
const LIVE: &str = "('running', 'waiting_input', 'waiting_approval')";
/// 结束了的状态，SQL 里用。
const TERMINAL: &str = "('succeeded', 'failed', 'cancelled', 'interrupted')";

const RUN_COLS: &str = "id, org_id, chat_id, source, status, principal, request, \
    user_message_id, assistant_message_id, node, lease_expires_at_micros, cancel_requested, \
    pending, response, prompt_tokens, completion_tokens, cost, cost_currency, error, last_seq, \
    created_at_micros, started_at_micros, finished_at_micros, updated_at_micros";

pub struct PgRunRepository {
    pool: PgPool,
    wakeup: Arc<Notify>,
    signals: RunSignalHub,
}

impl PgRunRepository {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            wakeup: Arc::new(Notify::new()),
            signals: RunSignalHub::default(),
        }
    }

    /// 在单独的连接上 LISTEN 两个频道，不占共享连接池。断开后带退避重连。
    pub fn spawn_notification_listener(self: Arc<Self>, dsn: String) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut failures = 0_u32;

            loop {
                match sqlx::postgres::PgListener::connect(&dsn).await {
                    Ok(mut listener) => {
                        if let Err(error) =
                            listener.listen_all([PENDING_CHANNEL, SIGNAL_CHANNEL]).await
                        {
                            failures = failures.saturating_add(1);
                            tracing::warn!(%error, "agent run notification listener subscription failed");
                        } else {
                            tracing::info!("agent run notification listener started");

                            loop {
                                match listener.recv().await {
                                    Ok(notification) => {
                                        failures = 0;
                                        self.heard(notification.channel(), notification.payload());
                                    }
                                    Err(error) => {
                                        failures = failures.saturating_add(1);
                                        tracing::warn!(%error, "agent run notification listener disconnected");
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    Err(_) => {
                        failures = failures.saturating_add(1);
                        tracing::warn!("agent run notification listener connect failed");
                    }
                }

                tokio::time::sleep(notification_reconnect_delay(failures)).await;
            }
        })
    }

    fn heard(&self, channel: &str, payload: &str) {
        if channel == PENDING_CHANNEL {
            self.wakeup.notify_one();
        } else {
            self.signals.notify(payload);
        }
    }

    async fn notify_signal(&self, run_id: &Id) -> Result<()> {
        sqlx::query("SELECT pg_notify($1, $2)")
            .bind(SIGNAL_CHANNEL)
            .bind(&run_id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        self.signals.notify(&run_id.0);
        Ok(())
    }
}

fn run_row(row: PgRow) -> Result<Run> {
    let source: String = row.try_get("source").map_err(sqlx_err)?;
    let status: String = row.try_get("status").map_err(sqlx_err)?;
    let principal: Json<RunPrincipal> = row.try_get("principal").map_err(sqlx_err)?;
    let pending: Option<Json<Pending>> = row.try_get("pending").map_err(sqlx_err)?;
    let response: Option<Json<RunResponse>> = row.try_get("response").map_err(sqlx_err)?;
    let request: Json<serde_json::Value> = row.try_get("request").map_err(sqlx_err)?;
    let id = |column: &str| {
        row.try_get::<Option<String>, _>(column)
            .map(|id| id.map(Id))
    };
    let micros = |column: &str| {
        row.try_get::<Option<i64>, _>(column)
            .map(|at| at.map(TimestampMicros))
    };

    Ok(Run {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        chat_id: Id(row.try_get("chat_id").map_err(sqlx_err)?),
        source: RunSource::parse(&source)
            .ok_or_else(|| Error::internal(format!("unknown agent run source `{source}`")))?,
        status: RunStatus::parse(&status)
            .ok_or_else(|| Error::internal(format!("unknown agent run status `{status}`")))?,
        principal: principal.0,
        request: request.0,
        user_message_id: id("user_message_id").map_err(sqlx_err)?,
        assistant_message_id: id("assistant_message_id").map_err(sqlx_err)?,
        node: row.try_get("node").map_err(sqlx_err)?,
        lease_expires_at: micros("lease_expires_at_micros").map_err(sqlx_err)?,
        cancel_requested: row.try_get("cancel_requested").map_err(sqlx_err)?,
        pending: pending.map(|pending| pending.0),
        response: response.map(|response| response.0),
        prompt_tokens: row.try_get("prompt_tokens").map_err(sqlx_err)?,
        completion_tokens: row.try_get("completion_tokens").map_err(sqlx_err)?,
        cost: Cost::from_columns(
            row.try_get("cost").map_err(sqlx_err)?,
            row.try_get::<Option<String>, _>("cost_currency")
                .map_err(sqlx_err)?
                .as_deref(),
        )?,
        error: row.try_get("error").map_err(sqlx_err)?,
        last_seq: row.try_get("last_seq").map_err(sqlx_err)?,
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        started_at: micros("started_at_micros").map_err(sqlx_err)?,
        finished_at: micros("finished_at_micros").map_err(sqlx_err)?,
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

#[async_trait]
impl RunRepository for PgRunRepository {
    async fn create(&self, run: Run) -> Result<Run> {
        sqlx::query(
            "WITH inserted AS (
                 INSERT INTO agent_runs
                    (id, org_id, chat_id, source, status, principal, request, user_message_id,
                     created_at_micros, updated_at_micros)
                 VALUES ($1, $2, $3, $4, 'queued', $5, $6, $7, $8, $8)
                 RETURNING 1
             )
             SELECT pg_notify($9, '') AS pending_notification FROM inserted",
        )
        .bind(&run.id.0)
        .bind(&run.org_id.0)
        .bind(&run.chat_id.0)
        .bind(run.source.as_str())
        .bind(Json(&run.principal))
        .bind(Json(&run.request))
        .bind(run.user_message_id.as_ref().map(|id| id.0.as_str()))
        .bind(run.created_at.0)
        .bind(PENDING_CHANNEL)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;

        self.wakeup.notify_one();
        Ok(run)
    }

    async fn get(&self, org_id: &Id, id: &Id) -> Result<Run> {
        let row = sqlx::query(&format!(
            "SELECT {RUN_COLS} FROM agent_runs WHERE org_id = $1 AND id = $2"
        ))
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| Error::not_found(format!("agent run `{}` not found", id.0)))?;

        run_row(row)
    }

    async fn for_chat(&self, org_id: &Id, chat_id: &Id, limit: i64) -> Result<Vec<Run>> {
        let rows = sqlx::query(&format!(
            "SELECT {RUN_COLS} FROM agent_runs
             WHERE org_id = $1 AND chat_id = $2
             ORDER BY created_at_micros DESC, id DESC
             LIMIT $3"
        ))
        .bind(&org_id.0)
        .bind(&chat_id.0)
        .bind(limit.clamp(1, 200))
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(run_row).collect()
    }

    async fn claim_next(&self, node: &str, lease_until: TimestampMicros) -> Result<Option<Run>> {
        // 会话里还有更早的 Run 没结束（排队中或执行中）时跳过它；领取与这个判断在同一条语句里，
        // 两个节点同时领也不会让同一会话的两个 Run 同时执行。
        let row = sqlx::query(&format!(
            "UPDATE agent_runs SET status = 'running', node = $1, lease_expires_at_micros = $2,
                    started_at_micros = $3, updated_at_micros = $3
             WHERE id = (
                 SELECT r.id FROM agent_runs r
                 WHERE r.status = 'queued'
                   AND NOT EXISTS (
                       SELECT 1 FROM agent_runs earlier
                       WHERE earlier.chat_id = r.chat_id
                         AND earlier.id <> r.id
                         AND (earlier.status IN {LIVE}
                              OR (earlier.status = 'queued'
                                  AND (earlier.created_at_micros, earlier.id)
                                      < (r.created_at_micros, r.id)))
                   )
                 ORDER BY r.created_at_micros, r.id
                 LIMIT 1
                 FOR UPDATE SKIP LOCKED
             )
             RETURNING {RUN_COLS}"
        ))
        .bind(node)
        .bind(lease_until.0)
        .bind(TimestampMicros::now().0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        row.map(run_row).transpose()
    }

    async fn wait_for_pending(&self, timeout: Duration) -> bool {
        tokio::select! {
            _ = self.wakeup.notified() => true,
            _ = tokio::time::sleep(timeout) => false,
        }
    }

    fn signals(&self) -> RunSignals {
        self.signals.subscribe()
    }

    async fn renew(&self, id: &Id, node: &str, lease_until: TimestampMicros) -> Result<bool> {
        let result = sqlx::query(&format!(
            "UPDATE agent_runs SET lease_expires_at_micros = $3, updated_at_micros = $4
             WHERE id = $1 AND node = $2 AND status IN {LIVE}"
        ))
        .bind(&id.0)
        .bind(node)
        .bind(lease_until.0)
        .bind(TimestampMicros::now().0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(result.rows_affected() == 1)
    }

    async fn wait_for(&self, id: &Id, pending: &Pending) -> Result<()> {
        let result = sqlx::query(&format!(
            "UPDATE agent_runs SET pending = $2, response = NULL, status = $3,
                    updated_at_micros = $4
             WHERE id = $1 AND status IN {LIVE}"
        ))
        .bind(&id.0)
        .bind(Json(pending))
        .bind(pending.status().as_str())
        .bind(TimestampMicros::now().0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        if result.rows_affected() == 0 {
            return Err(Error::conflict("the answer is no longer running"));
        }

        Ok(())
    }

    async fn respond(&self, org_id: &Id, id: &Id, response: &RunResponse) -> Result<Run> {
        let run = self.get(org_id, id).await?;

        let waiting = run
            .pending
            .as_ref()
            .is_some_and(|pending| response.answers(pending));

        if !waiting || run.response.is_some() {
            return Err(Error::conflict(
                "the answer is not waiting for this question or confirmation",
            ));
        }

        let row = sqlx::query(&format!(
            "UPDATE agent_runs SET response = $3, updated_at_micros = $5
             WHERE org_id = $1 AND id = $2 AND response IS NULL
               AND pending->>'call_id' = $4
               AND status IN ('waiting_input', 'waiting_approval')
             RETURNING {RUN_COLS}"
        ))
        .bind(&org_id.0)
        .bind(&id.0)
        .bind(Json(response))
        .bind(response.call_id())
        .bind(TimestampMicros::now().0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| {
            Error::conflict("the answer is not waiting for this question or confirmation")
        })?;

        self.notify_signal(id).await?;
        run_row(row)
    }

    async fn take_response(&self, id: &Id) -> Result<Option<RunResponse>> {
        let row = sqlx::query(
            "UPDATE agent_runs r
             SET response = NULL, pending = NULL, status = 'running', updated_at_micros = $2
             FROM (
                 SELECT id, response FROM agent_runs
                 WHERE id = $1 AND response IS NOT NULL
                 FOR UPDATE
             ) answered
             WHERE r.id = answered.id
             RETURNING answered.response",
        )
        .bind(&id.0)
        .bind(TimestampMicros::now().0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        row.map(|row| {
            row.try_get::<Json<RunResponse>, _>("response")
                .map(|response| response.0)
                .map_err(sqlx_err)
        })
        .transpose()
    }

    async fn stop_waiting(&self, id: &Id) -> Result<()> {
        sqlx::query(
            "UPDATE agent_runs
             SET pending = NULL, response = NULL, status = 'running', updated_at_micros = $2
             WHERE id = $1 AND status IN ('waiting_input', 'waiting_approval')",
        )
        .bind(&id.0)
        .bind(TimestampMicros::now().0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn request_cancel(&self, org_id: &Id, id: &Id) -> Result<Run> {
        let run = self.get(org_id, id).await?;

        if run.status.is_terminal() {
            return Ok(run);
        }

        // 排队中的直接结束，补一个结束事件；已经被领走的只记下请求。
        if run.status == RunStatus::Queued {
            let outcome = RunOutcome {
                status: RunStatus::Cancelled,
                assistant_message_id: None,
                prompt_tokens: 0,
                completion_tokens: 0,
                cost: None,
                error: None,
            };

            let done = NewRunEvent::new(RunEventKind::Done, json!({ "status": "cancelled" }));

            if events::finish_queued(self, &run, &outcome, &done).await? {
                return self.get(org_id, id).await;
            }
        }

        sqlx::query(&format!(
            "UPDATE agent_runs SET cancel_requested = TRUE, updated_at_micros = $3
             WHERE org_id = $1 AND id = $2 AND status NOT IN {TERMINAL}"
        ))
        .bind(&org_id.0)
        .bind(&id.0)
        .bind(TimestampMicros::now().0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        self.notify_signal(id).await?;
        self.get(org_id, id).await
    }

    async fn cancel_requested(&self, id: &Id) -> Result<bool> {
        let requested = sqlx::query("SELECT cancel_requested FROM agent_runs WHERE id = $1")
            .bind(&id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?
            .map(|row| row.try_get::<bool, _>("cancel_requested"))
            .transpose()
            .map_err(sqlx_err)?;

        // 找不到的 Run 没有什么可继续的。
        Ok(requested.unwrap_or(true))
    }

    async fn append_events(&self, org_id: &Id, run_id: &Id, events: &[NewRunEvent]) -> Result<i64> {
        events::append(self, org_id, run_id, events).await
    }

    async fn finish(
        &self,
        run: &Run,
        outcome: &RunOutcome,
        events: &[NewRunEvent],
    ) -> Result<bool> {
        events::finish(self, run, outcome, events).await
    }

    async fn events_after(&self, run_id: &Id, after: i64, limit: i64) -> Result<Vec<RunEvent>> {
        events::after(&self.pool, run_id, after, limit).await
    }

    async fn expire_leases(&self, now: TimestampMicros) -> Result<Vec<Run>> {
        events::expire_leases(self, now).await
    }

    async fn purge_events(&self, before: TimestampMicros) -> Result<u64> {
        events::purge(&self.pool, before).await
    }
}
