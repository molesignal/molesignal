// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 话题对应的会话、收到的消息，以及一次性的绑定链接。

use serde_json::{Value, json};
use sqlx::{PgPool, Row, postgres::PgRow, types::Json};

use super::{micros, optional_micros, optional_text, sqlx_err, text};
use crate::{
    agent::im::{ImLink, ImMessage, ImThread, MessageStatus},
    shared::{Result, ids::Id, time::TimestampMicros},
};

const THREAD_COLUMNS: &str = "id, org_id, connector_id, conversation_id, thread_key, chat_id, \
                              created_by, created_at, updated_at";

const MESSAGE_COLUMNS: &str = "id, org_id, connector_id, external_message_id, conversation_id, \
                               thread_key, sender_id, user_id, chat_id, run_id, reply, \
                               reaction_id, status, detail, created_at, replied_at";

/// 绑定链接存在 `one_time_tokens` 里（`kind = 'im_link'`）：`subject` 是 IM 里的身份，`payload` 里有
/// `connector_id` 与 `display_name`。
const LINK_COLUMNS: &str = "token_hash, org_id, payload->>'connector_id' AS connector_id, \
                            subject AS external_identity, payload->>'display_name' AS display_name, \
                            expires_at, created_at";

fn thread_row(row: PgRow) -> Result<ImThread> {
    Ok(ImThread {
        id: Id(text(&row, "id")?),
        org_id: Id(text(&row, "org_id")?),
        connector_id: Id(text(&row, "connector_id")?),
        conversation_id: text(&row, "conversation_id")?,
        thread_key: text(&row, "thread_key")?,
        chat_id: Id(text(&row, "chat_id")?),
        created_by: Id(text(&row, "created_by")?),
        created_at: micros(&row, "created_at")?,
        updated_at: micros(&row, "updated_at")?,
    })
}

fn message_row(row: PgRow) -> Result<ImMessage> {
    let reply: Json<Value> = row.try_get("reply").map_err(sqlx_err)?;

    Ok(ImMessage {
        id: Id(text(&row, "id")?),
        org_id: Id(text(&row, "org_id")?),
        connector_id: Id(text(&row, "connector_id")?),
        external_message_id: text(&row, "external_message_id")?,
        conversation_id: text(&row, "conversation_id")?,
        thread_key: text(&row, "thread_key")?,
        sender_id: text(&row, "sender_id")?,
        user_id: optional_text(&row, "user_id")?.map(Id),
        chat_id: optional_text(&row, "chat_id")?.map(Id),
        run_id: optional_text(&row, "run_id")?.map(Id),
        reply: reply.0,
        reaction_id: optional_text(&row, "reaction_id")?,
        status: MessageStatus::parse(&text(&row, "status")?)?,
        detail: optional_text(&row, "detail")?,
        created_at: micros(&row, "created_at")?,
        replied_at: optional_micros(&row, "replied_at")?,
    })
}

fn link_row(row: PgRow) -> Result<ImLink> {
    Ok(ImLink {
        token_hash: text(&row, "token_hash")?,
        org_id: Id(text(&row, "org_id")?),
        connector_id: Id(text(&row, "connector_id")?),
        external_identity: text(&row, "external_identity")?,
        display_name: optional_text(&row, "display_name")?,
        expires_at: micros(&row, "expires_at")?,
        created_at: micros(&row, "created_at")?,
    })
}

pub(super) async fn thread(
    pool: &PgPool,
    connector_id: &Id,
    conversation_id: &str,
    thread_key: &str,
) -> Result<Option<ImThread>> {
    sqlx::query(&format!(
        "SELECT {THREAD_COLUMNS} FROM agent_im_threads
         WHERE connector_id = $1 AND conversation_id = $2 AND thread_key = $3"
    ))
    .bind(&connector_id.0)
    .bind(conversation_id)
    .bind(thread_key)
    .fetch_optional(pool)
    .await
    .map_err(sqlx_err)?
    .map(thread_row)
    .transpose()
}

pub(super) async fn put_thread(pool: &PgPool, thread: &ImThread) -> Result<ImThread> {
    let row = sqlx::query(&format!(
        "INSERT INTO agent_im_threads
         (id, org_id, connector_id, conversation_id, thread_key, chat_id, created_by,
          created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         ON CONFLICT (connector_id, conversation_id, thread_key) DO UPDATE
         SET chat_id = EXCLUDED.chat_id, created_by = EXCLUDED.created_by,
             updated_at = EXCLUDED.updated_at
         RETURNING {THREAD_COLUMNS}"
    ))
    .bind(&thread.id.0)
    .bind(&thread.org_id.0)
    .bind(&thread.connector_id.0)
    .bind(&thread.conversation_id)
    .bind(&thread.thread_key)
    .bind(&thread.chat_id.0)
    .bind(&thread.created_by.0)
    .bind(thread.created_at)
    .bind(thread.updated_at)
    .fetch_one(pool)
    .await
    .map_err(sqlx_err)?;

    thread_row(row)
}

pub(super) async fn record(pool: &PgPool, message: &ImMessage) -> Result<bool> {
    let recorded = sqlx::query(
        "INSERT INTO agent_im_messages
         (id, org_id, connector_id, external_message_id, conversation_id, thread_key, sender_id,
          user_id, chat_id, run_id, reply, reaction_id, status, detail, created_at,
          replied_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
         ON CONFLICT (connector_id, external_message_id) DO NOTHING",
    )
    .bind(&message.id.0)
    .bind(&message.org_id.0)
    .bind(&message.connector_id.0)
    .bind(&message.external_message_id)
    .bind(&message.conversation_id)
    .bind(&message.thread_key)
    .bind(&message.sender_id)
    .bind(message.user_id.as_ref().map(|id| id.0.as_str()))
    .bind(message.chat_id.as_ref().map(|id| id.0.as_str()))
    .bind(message.run_id.as_ref().map(|id| id.0.as_str()))
    .bind(Json(&message.reply))
    .bind(message.reaction_id.as_deref())
    .bind(message.status.as_str())
    .bind(message.detail.as_deref())
    .bind(message.created_at)
    .bind(message.replied_at)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;

    Ok(recorded.rows_affected() == 1)
}

pub(super) async fn update(pool: &PgPool, message: &ImMessage) -> Result<()> {
    sqlx::query(
        "UPDATE agent_im_messages
         SET thread_key = $2, user_id = $3, chat_id = $4, run_id = $5, reply = $6,
             reaction_id = $7, status = $8, detail = $9, replied_at = $10
         WHERE id = $1",
    )
    .bind(&message.id.0)
    .bind(&message.thread_key)
    .bind(message.user_id.as_ref().map(|id| id.0.as_str()))
    .bind(message.chat_id.as_ref().map(|id| id.0.as_str()))
    .bind(message.run_id.as_ref().map(|id| id.0.as_str()))
    .bind(Json(&message.reply))
    .bind(message.reaction_id.as_deref())
    .bind(message.status.as_str())
    .bind(message.detail.as_deref())
    .bind(message.replied_at)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;

    Ok(())
}

pub(super) async fn move_status(
    pool: &PgPool,
    id: &Id,
    from: MessageStatus,
    to: MessageStatus,
) -> Result<bool> {
    let moved =
        sqlx::query("UPDATE agent_im_messages SET status = $3 WHERE id = $1 AND status = $2")
            .bind(&id.0)
            .bind(from.as_str())
            .bind(to.as_str())
            .execute(pool)
            .await
            .map_err(sqlx_err)?;

    Ok(moved.rows_affected() == 1)
}

pub(super) async fn settle(
    pool: &PgPool,
    id: &Id,
    status: MessageStatus,
    detail: Option<&str>,
    replied_at: TimestampMicros,
) -> Result<()> {
    sqlx::query(
        "UPDATE agent_im_messages SET status = $2, detail = $3, replied_at = $4
         WHERE id = $1",
    )
    .bind(&id.0)
    .bind(status.as_str())
    .bind(detail)
    .bind(replied_at)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;

    Ok(())
}

pub(super) async fn for_run(pool: &PgPool, run_id: &Id) -> Result<Option<ImMessage>> {
    sqlx::query(&format!(
        "SELECT {MESSAGE_COLUMNS} FROM agent_im_messages WHERE run_id = $1"
    ))
    .bind(&run_id.0)
    .fetch_optional(pool)
    .await
    .map_err(sqlx_err)?
    .map(message_row)
    .transpose()
}

pub(super) async fn answering(
    pool: &PgPool,
    before: TimestampMicros,
    limit: i64,
) -> Result<Vec<ImMessage>> {
    sqlx::query(&format!(
        "SELECT {MESSAGE_COLUMNS} FROM agent_im_messages
         WHERE status = 'answering' AND created_at < $1
         ORDER BY created_at
         LIMIT $2"
    ))
    .bind(before)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?
    .into_iter()
    .map(message_row)
    .collect()
}

pub(super) async fn purge(pool: &PgPool, before: TimestampMicros) -> Result<u64> {
    let messages = sqlx::query(
        "DELETE FROM agent_im_messages
         WHERE created_at < $1 AND status <> 'answering'",
    )
    .bind(before)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;

    sqlx::query("DELETE FROM one_time_tokens WHERE kind = 'im_link' AND expires_at < $1")
        .bind(TimestampMicros::now())
        .execute(pool)
        .await
        .map_err(sqlx_err)?;

    Ok(messages.rows_affected())
}

pub(super) async fn create_link(pool: &PgPool, link: &ImLink) -> Result<()> {
    sqlx::query(
        "INSERT INTO one_time_tokens
         (token_hash, kind, org_id, subject, payload, expires_at, created_at)
         VALUES ($1, 'im_link', $2, $3, $4, $5, $6)",
    )
    .bind(&link.token_hash)
    .bind(&link.org_id.0)
    .bind(&link.external_identity)
    .bind(Json(json!({
        "connector_id": link.connector_id.0,
        "display_name": link.display_name,
    })))
    .bind(link.expires_at)
    .bind(link.created_at)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;

    Ok(())
}

pub(super) async fn take_link(
    pool: &PgPool,
    token_hash: &str,
    now: TimestampMicros,
) -> Result<Option<ImLink>> {
    sqlx::query(&format!(
        "DELETE FROM one_time_tokens
         WHERE token_hash = $1 AND kind = 'im_link' AND expires_at >= $2
         RETURNING {LINK_COLUMNS}"
    ))
    .bind(token_hash)
    .bind(now)
    .fetch_optional(pool)
    .await
    .map_err(sqlx_err)?
    .map(link_row)
    .transpose()
}

pub(super) async fn peek_link(
    pool: &PgPool,
    token_hash: &str,
    now: TimestampMicros,
) -> Result<Option<ImLink>> {
    sqlx::query(&format!(
        "SELECT {LINK_COLUMNS} FROM one_time_tokens
         WHERE token_hash = $1 AND kind = 'im_link' AND expires_at >= $2"
    ))
    .bind(token_hash)
    .bind(now)
    .fetch_optional(pool)
    .await
    .map_err(sqlx_err)?
    .map(link_row)
    .transpose()
}
