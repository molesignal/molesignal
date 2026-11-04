// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 等连着机器人的节点发出去的回复与通知：`agent_im_outbox`。取走即删除，`SKIP LOCKED` 让两个节点
//! 不会取到同一条。

use serde_json::Value;
use sqlx::{PgPool, Row, postgres::PgRow, types::Json};

use super::{micros, optional_text, sqlx_err, text};
use crate::{
    agent::im::ImOutgoing,
    shared::{Result, ids::Id, time::TimestampMicros},
};

const COLUMNS: &str = "id, org_id, connector_id, message_id, reply, working, markdown, \
                       notification, created_at";

fn outgoing_row(row: PgRow) -> Result<ImOutgoing> {
    let reply: Json<Value> = row.try_get("reply").map_err(sqlx_err)?;
    let notification: Option<Json<Value>> = row.try_get("notification").map_err(sqlx_err)?;

    Ok(ImOutgoing {
        id: Id(text(&row, "id")?),
        org_id: Id(text(&row, "org_id")?),
        connector_id: Id(text(&row, "connector_id")?),
        message_id: optional_text(&row, "message_id")?.map(Id),
        reply: reply.0,
        working: optional_text(&row, "working")?,
        markdown: text(&row, "markdown")?,
        notification: notification.map(|notification| notification.0),
        created_at: micros(&row, "created_at")?,
    })
}

pub(super) async fn enqueue(pool: &PgPool, outgoing: &ImOutgoing) -> Result<()> {
    sqlx::query(
        "INSERT INTO agent_im_outbox
         (id, org_id, connector_id, message_id, reply, working, markdown, notification,
          created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(&outgoing.id.0)
    .bind(&outgoing.org_id.0)
    .bind(&outgoing.connector_id.0)
    .bind(outgoing.message_id.as_ref().map(|id| id.0.as_str()))
    .bind(Json(&outgoing.reply))
    .bind(outgoing.working.as_deref())
    .bind(&outgoing.markdown)
    .bind(outgoing.notification.as_ref().map(Json))
    .bind(outgoing.created_at)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;

    Ok(())
}

pub(super) async fn take(pool: &PgPool, connector_id: &Id, limit: i64) -> Result<Vec<ImOutgoing>> {
    let mut taken = sqlx::query(&format!(
        "DELETE FROM agent_im_outbox
         WHERE id IN (
             SELECT id FROM agent_im_outbox
             WHERE connector_id = $1
             ORDER BY created_at
             LIMIT $2
             FOR UPDATE SKIP LOCKED
         )
         RETURNING {COLUMNS}"
    ))
    .bind(&connector_id.0)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?
    .into_iter()
    .map(outgoing_row)
    .collect::<Result<Vec<_>>>()?;

    // RETURNING 不保证顺序：按记下的先后发。
    taken.sort_by_key(|outgoing| outgoing.created_at.0);
    Ok(taken)
}

pub(super) async fn purge(pool: &PgPool, before: TimestampMicros) -> Result<()> {
    sqlx::query("DELETE FROM agent_im_outbox WHERE created_at < $1")
        .bind(before)
        .execute(pool)
        .await
        .map_err(sqlx_err)?;

    Ok(())
}
