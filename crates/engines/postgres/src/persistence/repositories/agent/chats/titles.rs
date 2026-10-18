// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 会话的名字：给临时标题的会话起名，人给会话改名，按 ID 一次取几个会话（例如记忆页写出处）。

use sqlx::PgPool;

use super::{
    super::super::sqlx_err,
    Chat,
    rows::{CHAT_COLS, chat_row},
};
use crate::shared::{Result, ids::Id};

pub(super) async fn get_chats(pool: &PgPool, org_id: &Id, ids: &[Id]) -> Result<Vec<Chat>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let ids: Vec<&str> = ids.iter().map(|id| id.0.as_str()).collect();

    let rows = sqlx::query(&format!(
        "SELECT {CHAT_COLS} FROM agent_chats
         WHERE org_id = $1 AND id = ANY($2) AND deleted_at_micros IS NULL"
    ))
    .bind(&org_id.0)
    .bind(&ids)
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?;

    Ok(rows.into_iter().map(chat_row).collect())
}

/// 只改标题还是临时的会话：两个回答同时起名时只有一个算数。
pub(super) async fn name_chat(pool: &PgPool, id: &Id, title: &str) -> Result<bool> {
    let named = sqlx::query(
        "UPDATE agent_chats SET title = $2, title_provisional = FALSE
         WHERE id = $1 AND title_provisional AND deleted_at_micros IS NULL",
    )
    .bind(&id.0)
    .bind(title)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;

    Ok(named.rows_affected() > 0)
}

/// 改名不动会话的更新时间：列表里的位置照旧。
pub(super) async fn rename_chat(pool: &PgPool, id: &Id, title: &str) -> Result<()> {
    sqlx::query(
        "UPDATE agent_chats SET title = $2, title_provisional = FALSE
         WHERE id = $1 AND deleted_at_micros IS NULL",
    )
    .bind(&id.0)
    .bind(title)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;

    Ok(())
}
