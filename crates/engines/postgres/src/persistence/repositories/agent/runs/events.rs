// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Run 的事件：追加、续读、随结束一起写下的最后几条，以及过期 Run 的中断与旧事件的清理。

use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool, Row, types::Json};

use super::{LIVE, PENDING_CHANNEL, PgRunRepository, RUN_COLS, SIGNAL_CHANNEL, TERMINAL, run_row};
use crate::{
    agent::runs::{NewRunEvent, Run, RunEvent, RunEventKind, RunOutcome},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
    sqlx_err,
};

/// 节点没了时写给 Run 的原因。
const INTERRUPTED: &str = "the server running this answer stopped before it finished";
/// 一次最多把这么多个过期 Run 标为中断。
const EXPIRE_BATCH: i64 = 100;
/// 清理事件时一次最多删这么多条。
const PURGE_BATCH: i64 = 5_000;

/// 把事件写成 `seq` 从 `first` 起的行。
async fn insert(
    connection: &mut PgConnection,
    org_id: &Id,
    run_id: &Id,
    first: i64,
    events: &[NewRunEvent],
    at: TimestampMicros,
) -> Result<()> {
    let rows = events
        .iter()
        .zip(first..)
        .map(|(event, seq)| json!({ "seq": seq, "kind": event.kind.as_str(), "payload": event.payload }))
        .collect::<Vec<_>>();

    sqlx::query(
        "INSERT INTO agent_run_events (run_id, seq, org_id, kind, payload, created_at_micros)
         SELECT $1, e.seq, $2, e.kind, e.payload, $3
         FROM jsonb_to_recordset($4::JSONB) AS e(seq BIGINT, kind TEXT, payload JSONB)",
    )
    .bind(&run_id.0)
    .bind(&org_id.0)
    .bind(at.0)
    .bind(Json(Value::Array(rows)))
    .execute(connection)
    .await
    .map_err(sqlx_err)?;

    Ok(())
}

async fn notify(connection: &mut PgConnection, channel: &str, payload: &str) -> Result<()> {
    sqlx::query("SELECT pg_notify($1, $2)")
        .bind(channel)
        .bind(payload)
        .execute(connection)
        .await
        .map_err(sqlx_err)?;

    Ok(())
}

pub(super) async fn append(
    repository: &PgRunRepository,
    org_id: &Id,
    run_id: &Id,
    events: &[NewRunEvent],
) -> Result<i64> {
    let now = TimestampMicros::now();
    let count = i64::try_from(events.len()).unwrap_or(i64::MAX);
    let mut tx = sqlx::begin(&repository.pool).await.map_err(sqlx_err)?;

    let last = sqlx::query(&format!(
        "UPDATE agent_runs SET last_seq = last_seq + $2, updated_at_micros = $3
         WHERE id = $1 AND status NOT IN {TERMINAL}
         RETURNING last_seq"
    ))
    .bind(&run_id.0)
    .bind(count)
    .bind(now.0)
    .fetch_optional(&mut *tx)
    .await
    .map_err(sqlx_err)?
    .map(|row| row.try_get::<i64, _>("last_seq"))
    .transpose()
    .map_err(sqlx_err)?
    .ok_or_else(|| Error::conflict("the answer is already over"))?;

    if events.is_empty() {
        return Ok(last);
    }

    insert(&mut tx, org_id, run_id, last - count + 1, events, now).await?;
    notify(&mut tx, SIGNAL_CHANNEL, &run_id.0).await?;
    tx.commit().await.map_err(sqlx_err)?;

    repository.signals.notify(&run_id.0);
    Ok(last)
}

pub(super) async fn finish(
    repository: &PgRunRepository,
    run: &Run,
    outcome: &RunOutcome,
    events: &[NewRunEvent],
) -> Result<bool> {
    finish_where(
        repository,
        run,
        outcome,
        events,
        &format!("NOT IN {TERMINAL}"),
    )
    .await
}

/// 排队中的 Run 直接结束；已经被领走时什么都不写，返回 false。
pub(super) async fn finish_queued(
    repository: &PgRunRepository,
    run: &Run,
    outcome: &RunOutcome,
    done: &NewRunEvent,
) -> Result<bool> {
    finish_where(
        repository,
        run,
        outcome,
        std::slice::from_ref(done),
        "= 'queued'",
    )
    .await
}

async fn finish_where(
    repository: &PgRunRepository,
    run: &Run,
    outcome: &RunOutcome,
    events: &[NewRunEvent],
    status_condition: &str,
) -> Result<bool> {
    let now = TimestampMicros::now();
    let count = i64::try_from(events.len()).unwrap_or(i64::MAX);
    let mut tx = sqlx::begin(&repository.pool).await.map_err(sqlx_err)?;

    let last = sqlx::query(&format!(
        "UPDATE agent_runs SET status = $2, assistant_message_id = $3, prompt_tokens = $4,
                completion_tokens = $5, cost = $6, cost_currency = $10, error = $7,
                finished_at_micros = $8, updated_at_micros = $8, lease_expires_at_micros = NULL,
                pending = NULL, response = NULL,
                cancel_requested = cancel_requested OR $2 = 'cancelled', last_seq = last_seq + $9
         WHERE id = $1 AND status {status_condition}
         RETURNING last_seq"
    ))
    .bind(&run.id.0)
    .bind(outcome.status.as_str())
    .bind(
        outcome
            .assistant_message_id
            .as_ref()
            .map(|id| id.0.as_str()),
    )
    .bind(outcome.prompt_tokens)
    .bind(outcome.completion_tokens)
    .bind(outcome.cost.map(|cost| cost.amount))
    .bind(outcome.error.as_deref())
    .bind(now.0)
    .bind(count)
    .bind(outcome.cost.map(|cost| cost.currency.as_str().to_string()))
    .fetch_optional(&mut *tx)
    .await
    .map_err(sqlx_err)?
    .map(|row| row.try_get::<i64, _>("last_seq"))
    .transpose()
    .map_err(sqlx_err)?;

    let Some(last) = last else {
        return Ok(false);
    };

    if !events.is_empty() {
        insert(&mut tx, &run.org_id, &run.id, last - count + 1, events, now).await?;
    }

    // 会话里排在后面的 Run 这时可以领了。
    notify(&mut tx, SIGNAL_CHANNEL, &run.id.0).await?;
    notify(&mut tx, PENDING_CHANNEL, "").await?;
    tx.commit().await.map_err(sqlx_err)?;

    repository.signals.notify(&run.id.0);
    repository.wakeup.notify_one();
    Ok(true)
}

pub(super) async fn after(
    pool: &PgPool,
    run_id: &Id,
    after: i64,
    limit: i64,
) -> Result<Vec<RunEvent>> {
    let rows = sqlx::query(
        "SELECT run_id, seq, kind, payload, created_at_micros FROM agent_run_events
         WHERE run_id = $1 AND seq > $2
         ORDER BY seq
         LIMIT $3",
    )
    .bind(&run_id.0)
    .bind(after)
    .bind(limit.clamp(1, 1_000))
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?;

    rows.into_iter()
        .map(|row| {
            let kind: String = row.try_get("kind").map_err(sqlx_err)?;
            let payload: Json<Value> = row.try_get("payload").map_err(sqlx_err)?;

            Ok(RunEvent {
                run_id: Id(row.try_get("run_id").map_err(sqlx_err)?),
                seq: row.try_get("seq").map_err(sqlx_err)?,
                kind: RunEventKind::parse(&kind)
                    .ok_or_else(|| Error::internal(format!("unknown agent run event `{kind}`")))?,
                payload: payload.0,
                created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
            })
        })
        .collect()
}

pub(super) async fn expire_leases(
    repository: &PgRunRepository,
    now: TimestampMicros,
) -> Result<Vec<Run>> {
    let rows = sqlx::query(&format!(
        "WITH expired AS (
             SELECT id FROM agent_runs
             WHERE status IN {LIVE} AND lease_expires_at_micros < $1
             ORDER BY lease_expires_at_micros
             LIMIT $3
             FOR UPDATE SKIP LOCKED
         ), interrupted AS (
             UPDATE agent_runs r
             SET status = 'interrupted', error = $2, finished_at_micros = $1,
                 updated_at_micros = $1, lease_expires_at_micros = NULL, pending = NULL,
                 response = NULL, last_seq = r.last_seq + 1
             FROM expired
             WHERE r.id = expired.id
             RETURNING r.*
         ), done AS (
             INSERT INTO agent_run_events (run_id, seq, org_id, kind, payload, created_at_micros)
             SELECT id, last_seq, org_id, 'done',
                    jsonb_build_object('status', 'interrupted', 'error', $2::TEXT), $1
             FROM interrupted
         )
         SELECT {RUN_COLS}, pg_notify($4, id) AS signal_notification FROM interrupted"
    ))
    .bind(now.0)
    .bind(INTERRUPTED)
    .bind(EXPIRE_BATCH)
    .bind(SIGNAL_CHANNEL)
    .fetch_all(&repository.pool)
    .await
    .map_err(sqlx_err)?;

    let runs = rows.into_iter().map(run_row).collect::<Result<Vec<_>>>()?;

    if !runs.is_empty() {
        sqlx::query("SELECT pg_notify($1, '')")
            .bind(PENDING_CHANNEL)
            .execute(&repository.pool)
            .await
            .map_err(sqlx_err)?;

        for run in &runs {
            repository.signals.notify(&run.id.0);
        }

        repository.wakeup.notify_one();
    }

    Ok(runs)
}

pub(super) async fn purge(pool: &PgPool, before: TimestampMicros) -> Result<u64> {
    let mut purged = 0;

    loop {
        let deleted = sqlx::query(
            "DELETE FROM agent_run_events
             WHERE ctid IN (
                 SELECT ctid FROM agent_run_events WHERE created_at_micros < $1 LIMIT $2
             )",
        )
        .bind(before.0)
        .bind(PURGE_BATCH)
        .execute(pool)
        .await
        .map_err(sqlx_err)?
        .rows_affected();

        purged += deleted;

        if deleted < PURGE_BATCH as u64 {
            return Ok(purged);
        }
    }
}
