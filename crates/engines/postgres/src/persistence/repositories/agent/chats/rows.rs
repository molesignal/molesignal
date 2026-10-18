// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `agent_chats` 与 `agent_messages` 的列，以及行到 [`Chat`] / [`ChatMessage`] 的对应。读不出的
//! 可选列当作空。

use serde_json::Value;
use sqlx::{PgExecutor, Row, types::Json};

use super::{super::super::sqlx_err, Chat, ChatMessage};
use crate::{
    agent::usage::Cost,
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub(super) const CHAT_COLS: &str = "id, org_id, user_id, provider, model, title, provider_id, \
    analysis_mode, time_range_start_micros, time_range_end_micros, archive_object_key, \
    deleted_at_micros, scope, team_id, mounted_team_ids, created_at_micros, updated_at_micros, \
    reply_language, context_summary, context_summary_upto_micros, subject_kind, subject_id, \
    investigation_id, memory_distilled_at_micros, parent_chat_id, title_provisional";

/// 与 [`CHAT_COLS`] 相同，带上 `c.` 前缀，给连了别的表的语句用。
pub(super) const CHAT_COLS_QUALIFIED: &str = "c.id, c.org_id, c.user_id, c.provider, c.model, \
    c.title, c.provider_id, c.analysis_mode, c.time_range_start_micros, c.time_range_end_micros, \
    c.archive_object_key, c.deleted_at_micros, c.scope, c.team_id, c.mounted_team_ids, \
    c.created_at_micros, c.updated_at_micros, c.reply_language, c.context_summary, \
    c.context_summary_upto_micros, c.subject_kind, c.subject_id, c.investigation_id, \
    c.memory_distilled_at_micros, c.parent_chat_id, c.title_provisional";

pub(super) const MESSAGE_COLS: &str = "id, chat_id, org_id, role, content, user_id, tool_calls_json, \
    tool_result_for, prompt_tokens, completion_tokens, cost, cost_currency, prompt_template_id, \
    prompt_builtin_key, prompt_version, prompt_hash, evidence_json, command_json, \
    created_at_micros, run_id, tool_transcript, references_json";

pub(super) fn chat_row(r: sqlx::postgres::PgRow) -> Chat {
    Chat {
        id: Id(r.try_get::<String, _>("id").unwrap_or_default()),
        org_id: Id(r.try_get::<String, _>("org_id").unwrap_or_default()),
        user_id: Id(r.try_get::<String, _>("user_id").unwrap_or_default()),
        provider: r.try_get::<String, _>("provider").unwrap_or_default(),
        model: r.try_get::<String, _>("model").unwrap_or_default(),
        title: r.try_get::<String, _>("title").unwrap_or_default(),
        provider_id: r
            .try_get::<Option<String>, _>("provider_id")
            .unwrap_or_default(),
        analysis_mode: r
            .try_get::<Option<String>, _>("analysis_mode")
            .unwrap_or_default(),
        time_range_start_micros: r
            .try_get::<Option<i64>, _>("time_range_start_micros")
            .unwrap_or_default(),
        time_range_end_micros: r
            .try_get::<Option<i64>, _>("time_range_end_micros")
            .unwrap_or_default(),
        archive_object_key: r
            .try_get::<Option<String>, _>("archive_object_key")
            .unwrap_or_default(),
        deleted_at_micros: r
            .try_get::<Option<i64>, _>("deleted_at_micros")
            .unwrap_or_default(),
        scope: r
            .try_get::<String, _>("scope")
            .unwrap_or_else(|_| "personal".into()),
        team_id: r
            .try_get::<Option<String>, _>("team_id")
            .unwrap_or_default(),
        mounted_team_ids: r
            .try_get::<Json<Vec<String>>, _>("mounted_team_ids")
            .map(|ids| ids.0)
            .unwrap_or_default(),
        reply_language: r.try_get::<String, _>("reply_language").unwrap_or_default(),
        context_summary: r
            .try_get::<Option<String>, _>("context_summary")
            .unwrap_or_default(),
        context_summary_upto_micros: r
            .try_get::<Option<i64>, _>("context_summary_upto_micros")
            .unwrap_or_default(),
        subject_kind: r
            .try_get::<Option<String>, _>("subject_kind")
            .unwrap_or_default(),
        subject_id: r
            .try_get::<Option<String>, _>("subject_id")
            .unwrap_or_default(),
        investigation_id: r
            .try_get::<Option<String>, _>("investigation_id")
            .unwrap_or_default(),
        memory_distilled_at_micros: r
            .try_get::<Option<i64>, _>("memory_distilled_at_micros")
            .unwrap_or_default(),
        parent_chat_id: r
            .try_get::<Option<String>, _>("parent_chat_id")
            .unwrap_or_default(),
        title_provisional: r
            .try_get::<bool, _>("title_provisional")
            .unwrap_or_default(),
        created_at: TimestampMicros(r.try_get::<i64, _>("created_at_micros").unwrap_or_default()),
        updated_at: TimestampMicros(r.try_get::<i64, _>("updated_at_micros").unwrap_or_default()),
    }
}

pub(super) fn message_row(r: sqlx::postgres::PgRow) -> ChatMessage {
    let tools: Option<Json<Value>> = r.try_get("tool_calls_json").ok();
    let evidence: Option<Json<Value>> = r.try_get("evidence_json").ok();
    let command: Option<Json<Value>> = r.try_get("command_json").ok().flatten();
    let transcript: Option<Json<Value>> = r.try_get("tool_transcript").ok().flatten();
    let references: Option<Json<Value>> = r.try_get("references_json").ok().flatten();

    ChatMessage {
        id: Id(r.try_get::<String, _>("id").unwrap_or_default()),
        chat_id: Id(r.try_get::<String, _>("chat_id").unwrap_or_default()),
        org_id: Id(r.try_get::<String, _>("org_id").unwrap_or_default()),
        role: r.try_get::<String, _>("role").unwrap_or_default(),
        content: r.try_get::<String, _>("content").unwrap_or_default(),
        user_id: r
            .try_get::<Option<String>, _>("user_id")
            .unwrap_or_default()
            .map(Id),
        tool_calls_json: tools.map(|j| j.0),
        tool_result_for: r
            .try_get::<Option<String>, _>("tool_result_for")
            .unwrap_or_default(),
        prompt_tokens: r
            .try_get::<Option<i64>, _>("prompt_tokens")
            .unwrap_or_default(),
        completion_tokens: r
            .try_get::<Option<i64>, _>("completion_tokens")
            .unwrap_or_default(),
        cost: Cost::from_columns(
            r.try_get::<Option<f64>, _>("cost").unwrap_or_default(),
            r.try_get::<Option<String>, _>("cost_currency")
                .unwrap_or_default()
                .as_deref(),
        )
        .unwrap_or_default(),
        prompt_template_id: r
            .try_get::<Option<String>, _>("prompt_template_id")
            .unwrap_or_default(),
        prompt_builtin_key: r
            .try_get::<Option<String>, _>("prompt_builtin_key")
            .unwrap_or_default(),
        prompt_version: r
            .try_get::<Option<i32>, _>("prompt_version")
            .unwrap_or_default(),
        prompt_hash: r
            .try_get::<Option<String>, _>("prompt_hash")
            .unwrap_or_default(),
        evidence_json: evidence.map(|j| j.0),
        command_json: command.map(|j| j.0),
        run_id: r
            .try_get::<Option<String>, _>("run_id")
            .unwrap_or_default()
            .map(Id),
        tool_transcript: transcript.map(|j| j.0),
        references_json: references.map(|j| j.0),
        created_at: TimestampMicros(r.try_get::<i64, _>("created_at_micros").unwrap_or_default()),
    }
}

/// 写入一个会话；建会话与分叉都用它。
pub(super) async fn insert_chat<'e>(executor: impl PgExecutor<'e>, s: &Chat) -> Result<()> {
    sqlx::query(
        "INSERT INTO agent_chats
            (id, org_id, user_id, provider, model, title, provider_id, analysis_mode,
             time_range_start_micros, time_range_end_micros, archive_object_key,
             deleted_at_micros, scope, team_id, mounted_team_ids, created_at_micros,
             updated_at_micros, reply_language, context_summary, context_summary_upto_micros,
             subject_kind, subject_id, investigation_id, memory_distilled_at_micros,
             parent_chat_id, title_provisional)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16,
                 $17, $18, $19, $20, $21, $22, $23, $24, $25, $26)",
    )
    .bind(&s.id.0)
    .bind(&s.org_id.0)
    .bind(&s.user_id.0)
    .bind(&s.provider)
    .bind(&s.model)
    .bind(&s.title)
    .bind(&s.provider_id)
    .bind(&s.analysis_mode)
    .bind(s.time_range_start_micros)
    .bind(s.time_range_end_micros)
    .bind(&s.archive_object_key)
    .bind(s.deleted_at_micros)
    .bind(&s.scope)
    .bind(&s.team_id)
    .bind(Json(&s.mounted_team_ids))
    .bind(s.created_at.0)
    .bind(s.updated_at.0)
    .bind(&s.reply_language)
    .bind(&s.context_summary)
    .bind(s.context_summary_upto_micros)
    .bind(&s.subject_kind)
    .bind(&s.subject_id)
    .bind(&s.investigation_id)
    .bind(s.memory_distilled_at_micros)
    .bind(&s.parent_chat_id)
    .bind(s.title_provisional)
    .execute(executor)
    .await
    .map_err(sqlx_err)?;

    Ok(())
}
