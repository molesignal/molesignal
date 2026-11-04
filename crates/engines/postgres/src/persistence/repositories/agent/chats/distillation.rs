// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 会话结束后提炼记忆：挑出该提炼的会话，并记下提炼到的位置。

use sqlx::{PgPool, Row};

use super::{
    Chat,
    rows::{CHAT_COLS_QUALIFIED, chat_row},
};
use crate::{
    repositories::org_settings::sections::AGENT_MEMORY,
    shared::{Result, time::TimestampMicros},
    sqlx_err,
};

/// 组织打开了提炼、停下超过 `idle_before`、上次提炼之后有新消息的会话（自动化的不算）。挑中的同时记下这次
/// 提炼到的位置；先锁住再改，两个节点同时挑时 SKIP LOCKED 让它们分开。返回会话与上一次的位置。
pub(super) async fn due(
    pool: &PgPool,
    idle_before: TimestampMicros,
    limit: i64,
) -> Result<Vec<(Chat, Option<i64>)>> {
    let rows = sqlx::query(&format!(
        "WITH due AS (
             SELECT c.id, c.memory_distilled_at AS previous
             FROM agent_chats c
             JOIN org_settings s ON s.org_id = c.org_id AND s.section = '{AGENT_MEMORY}'
                 AND (s.value->>'distill_after_chat')::BOOLEAN
             WHERE c.deleted_at IS NULL
               AND c.automation_id IS NULL
               AND c.updated_at < $1
               AND c.updated_at > COALESCE(c.memory_distilled_at, TIMESTAMPTZ 'epoch')
             ORDER BY c.updated_at
             LIMIT $2
             FOR UPDATE OF c SKIP LOCKED
         )
         UPDATE agent_chats c SET memory_distilled_at = c.updated_at
         FROM due WHERE c.id = due.id
         RETURNING {CHAT_COLS_QUALIFIED}, due.previous"
    ))
    .bind(idle_before)
    .bind(limit.clamp(1, 100))
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?;

    Ok(rows
        .into_iter()
        .map(|row| {
            let previous = row
                .try_get::<Option<TimestampMicros>, _>("previous")
                .unwrap_or_default()
                .map(|previous| previous.0);

            (chat_row(row), previous)
        })
        .collect())
}
