// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 会话结束后提炼记忆：挑出该提炼的会话，并记下提炼到的位置。

use sqlx::{PgPool, Row};

use super::{
    Chat,
    rows::{CHAT_COLS_QUALIFIED, chat_row},
};
use crate::{
    shared::{Result, time::TimestampMicros},
    sqlx_err,
};

/// 组织打开了提炼、停下超过 `idle_before`、上次提炼之后有新消息的会话。挑中的同时记下这次
/// 提炼到的位置；先锁住再改，两个节点同时挑时 SKIP LOCKED 让它们分开。返回会话与上一次的位置。
pub(super) async fn due(
    pool: &PgPool,
    idle_before: TimestampMicros,
    limit: i64,
) -> Result<Vec<(Chat, Option<i64>)>> {
    let rows = sqlx::query(&format!(
        "WITH due AS (
             SELECT c.id, c.memory_distilled_at_micros AS previous
             FROM agent_chats c
             JOIN agent_memory_settings s ON s.org_id = c.org_id AND s.distill_after_chat
             WHERE c.deleted_at_micros IS NULL
               AND c.updated_at_micros < $1
               AND c.updated_at_micros > COALESCE(c.memory_distilled_at_micros, 0)
             ORDER BY c.updated_at_micros
             LIMIT $2
             FOR UPDATE OF c SKIP LOCKED
         )
         UPDATE agent_chats c SET memory_distilled_at_micros = c.updated_at_micros
         FROM due WHERE c.id = due.id
         RETURNING {CHAT_COLS_QUALIFIED}, due.previous"
    ))
    .bind(idle_before.0)
    .bind(limit.clamp(1, 100))
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?;

    Ok(rows
        .into_iter()
        .map(|row| {
            let previous = row
                .try_get::<Option<i64>, _>("previous")
                .unwrap_or_default();

            (chat_row(row), previous)
        })
        .collect())
}
