// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 开了「在 IM 里回答」的连接器，它们的状态与租约。

use sqlx::{PgPool, postgres::PgRow};

use super::{micros, optional_micros, optional_text, sqlx_err, text};
use crate::{
    agent::im::{BotStatus, ImBot},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const COLUMNS: &str = "connector_id, org_id, enabled, default_team_id, status, status_detail, \
                       status_at, last_message_at, created_by, updated_by, \
                       created_at, updated_at";

fn bot_row(row: PgRow) -> Result<ImBot> {
    use sqlx::Row;

    Ok(ImBot {
        connector_id: Id(text(&row, "connector_id")?),
        org_id: Id(text(&row, "org_id")?),
        enabled: row.try_get("enabled").map_err(sqlx_err)?,
        default_team_id: optional_text(&row, "default_team_id")?.map(Id),
        status: BotStatus::parse(&text(&row, "status")?)?,
        status_detail: optional_text(&row, "status_detail")?,
        status_at: optional_micros(&row, "status_at")?,
        last_message_at: optional_micros(&row, "last_message_at")?,
        created_by: Id(text(&row, "created_by")?),
        updated_by: Id(text(&row, "updated_by")?),
        created_at: micros(&row, "created_at")?,
        updated_at: micros(&row, "updated_at")?,
    })
}

/// 一个组织的机器人，或（`org_id` 为空时）所有组织开着的。
pub(super) async fn list(
    pool: &PgPool,
    org_id: Option<&Id>,
    enabled_only: bool,
) -> Result<Vec<ImBot>> {
    sqlx::query(&format!(
        "SELECT {COLUMNS} FROM agent_im_bots
         WHERE ($1::TEXT IS NULL OR org_id = $1) AND (NOT $2 OR enabled)
         ORDER BY created_at"
    ))
    .bind(org_id.map(|id| id.0.as_str()))
    .bind(enabled_only)
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?
    .into_iter()
    .map(bot_row)
    .collect()
}

pub(super) async fn get(pool: &PgPool, org_id: &Id, connector_id: &Id) -> Result<Option<ImBot>> {
    sqlx::query(&format!(
        "SELECT {COLUMNS} FROM agent_im_bots WHERE org_id = $1 AND connector_id = $2"
    ))
    .bind(&org_id.0)
    .bind(&connector_id.0)
    .fetch_optional(pool)
    .await
    .map_err(sqlx_err)?
    .map(bot_row)
    .transpose()
}

pub(super) async fn find(pool: &PgPool, connector_id: &Id) -> Result<Option<ImBot>> {
    sqlx::query(&format!(
        "SELECT {COLUMNS} FROM agent_im_bots WHERE connector_id = $1"
    ))
    .bind(&connector_id.0)
    .fetch_optional(pool)
    .await
    .map_err(sqlx_err)?
    .map(bot_row)
    .transpose()
}

pub(super) async fn put(pool: &PgPool, bot: &ImBot) -> Result<ImBot> {
    let row = sqlx::query(&format!(
        "INSERT INTO agent_im_bots
         (connector_id, org_id, enabled, default_team_id, status, created_by, updated_by,
          created_at, updated_at)
         VALUES ($1, $2, $3, $4, 'idle', $5, $6, $7, $8)
         ON CONFLICT (connector_id) DO UPDATE
         SET enabled = EXCLUDED.enabled, default_team_id = EXCLUDED.default_team_id,
             updated_by = EXCLUDED.updated_by, updated_at = EXCLUDED.updated_at
         WHERE agent_im_bots.org_id = EXCLUDED.org_id
         RETURNING {COLUMNS}"
    ))
    .bind(&bot.connector_id.0)
    .bind(&bot.org_id.0)
    .bind(bot.enabled)
    .bind(bot.default_team_id.as_ref().map(|id| id.0.as_str()))
    .bind(&bot.created_by.0)
    .bind(&bot.updated_by.0)
    .bind(bot.created_at)
    .bind(bot.updated_at)
    .fetch_optional(pool)
    .await
    .map_err(sqlx_err)?
    .ok_or_else(|| Error::not_found("the connector is not in this organization"))?;

    bot_row(row)
}

pub(super) async fn delete(pool: &PgPool, org_id: &Id, connector_id: &Id) -> Result<()> {
    let mut tx = sqlx::begin(pool).await.map_err(sqlx_err)?;

    sqlx::query("DELETE FROM agent_im_bots WHERE org_id = $1 AND connector_id = $2")
        .bind(&org_id.0)
        .bind(&connector_id.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

    // 还没用的绑定链接随机器人一起作废。
    sqlx::query(
        "DELETE FROM one_time_tokens
         WHERE kind = 'im_link' AND org_id = $1 AND payload->>'connector_id' = $2",
    )
    .bind(&org_id.0)
    .bind(&connector_id.0)
    .execute(&mut *tx)
    .await
    .map_err(sqlx_err)?;

    tx.commit().await.map_err(sqlx_err)?;
    Ok(())
}

/// 领到或续上租约：没人持有、持有的是自己，或别人的已过期。
pub(super) async fn lease(
    pool: &PgPool,
    connector_id: &Id,
    node: &str,
    now: TimestampMicros,
    until: TimestampMicros,
) -> Result<bool> {
    let leased = sqlx::query(
        "UPDATE agent_im_bots
         SET lease_owner = $2, lease_expires_at = $4
         WHERE connector_id = $1 AND enabled
           AND (lease_owner IS NULL OR lease_owner = $2 OR lease_expires_at < $3)",
    )
    .bind(&connector_id.0)
    .bind(node)
    .bind(now)
    .bind(until)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;

    Ok(leased.rows_affected() == 1)
}

pub(super) async fn release(pool: &PgPool, connector_id: &Id, node: &str) -> Result<()> {
    sqlx::query(
        "UPDATE agent_im_bots
         SET lease_owner = NULL, lease_expires_at = NULL
         WHERE connector_id = $1 AND lease_owner = $2",
    )
    .bind(&connector_id.0)
    .bind(node)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;

    Ok(())
}

pub(super) async fn set_status(
    pool: &PgPool,
    connector_id: &Id,
    node: &str,
    status: BotStatus,
    detail: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "UPDATE agent_im_bots
         SET status = $2, status_detail = $3, status_at = $4
         WHERE connector_id = $1 AND lease_owner = $5",
    )
    .bind(&connector_id.0)
    .bind(status.as_str())
    .bind(detail)
    .bind(TimestampMicros::now())
    .bind(node)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;

    Ok(())
}

pub(super) async fn touch(pool: &PgPool, connector_id: &Id, at: TimestampMicros) -> Result<()> {
    sqlx::query("UPDATE agent_im_bots SET last_message_at = $2 WHERE connector_id = $1")
        .bind(&connector_id.0)
        .bind(at)
        .execute(pool)
        .await
        .map_err(sqlx_err)?;

    Ok(())
}
