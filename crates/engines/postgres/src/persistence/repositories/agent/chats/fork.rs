// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 分叉会话：新会话与来源会话的消息（到分叉的那条为止），在同一个事务里写入。
//!
//! 复制的消息换了 ID、归到新会话，时间不变，所以问题与回答的先后、一个问题的几版回答都照旧；
//! 不带运行记录（运行属于来源会话）。消息按时间先后、同一时刻按 ID 排，"到那条为止"也照这个
//! 顺序算。

use sqlx::PgPool;

use super::{super::super::sqlx_err, Chat, ChatMessage, rows::insert_chat};
use crate::shared::{Error, Result};

pub(super) async fn fork(pool: &PgPool, fork: Chat, upto: Option<&ChatMessage>) -> Result<Chat> {
    let source = fork
        .parent_chat_id
        .clone()
        .ok_or_else(|| Error::invalid("a fork needs the chat it comes from"))?;

    let mut tx = sqlx::begin(pool).await.map_err(sqlx_err)?;
    insert_chat(&mut *tx, &fork).await?;

    sqlx::query(
        "INSERT INTO agent_messages
            (id, chat_id, org_id, role, content, user_id, tool_calls_json, tool_result_for,
             prompt_tokens, completion_tokens, cost, cost_currency, prompt_template_id,
             prompt_builtin_key, prompt_version, prompt_hash, evidence_json, command_json,
             created_at_micros, run_id, tool_transcript, references_json)
         SELECT gen_random_uuid()::TEXT, $1, org_id, role, content, user_id, tool_calls_json,
                tool_result_for, prompt_tokens, completion_tokens, cost, cost_currency,
                prompt_template_id, prompt_builtin_key, prompt_version, prompt_hash,
                evidence_json, command_json, created_at_micros, NULL, tool_transcript,
                references_json
         FROM agent_messages
         WHERE chat_id = $2 AND org_id = $3
           AND ($4::BIGINT IS NULL OR (created_at_micros, id) <= ($4, $5))",
    )
    .bind(&fork.id.0)
    .bind(&source)
    .bind(&fork.org_id.0)
    .bind(upto.map(|message| message.created_at.0))
    .bind(upto.map(|message| message.id.0.clone()).unwrap_or_default())
    .execute(&mut *tx)
    .await
    .map_err(sqlx_err)?;

    tx.commit().await.map_err(sqlx_err)?;
    Ok(fork)
}
