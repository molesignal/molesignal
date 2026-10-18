// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `agent_chats` + `agent_messages` 表 Pg 实装。
//!
//! Mole Agent 扩展：chat 记 provider_id / analysis_mode / time_range / archive 指针 /
//! soft-delete / 回复语言 / 上下文摘要；message 记 prompt 引用、evidence 摘要、出自哪个 Run
//! 与模型走过的步骤。列与行的对应在 [`rows`]。

use async_trait::async_trait;
use rows::{CHAT_COLS, MESSAGE_COLS, chat_row, message_row};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgPool, types::Json};

use super::super::sqlx_err;
use crate::{
    agent::usage::Cost,
    shared::{Result, ids::Id, time::TimestampMicros},
};

mod distillation;
mod fork;
mod rows;
mod titles;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chat {
    pub id: Id,
    pub org_id: Id,
    pub user_id: Id,
    pub provider: String,
    pub model: String,
    pub title: String,
    /// Mole Agent chat 绑定的 PG provider 行 id（区别于 `provider` 类型串）。
    pub provider_id: Option<String>,
    /// 分析模式：anomaly_analysis | root_cause | alert_explain | query_generation | 自由问答。
    pub analysis_mode: Option<String>,
    pub time_range_start_micros: Option<i64>,
    pub time_range_end_micros: Option<i64>,
    /// 归档对象存储 key（最近一次成功归档）。
    pub archive_object_key: Option<String>,
    /// 软删时间戳；非 NULL = 已删除（normal history 列表过滤掉）。
    pub deleted_at_micros: Option<i64>,
    /// `personal` 或 `team`；第一条消息前选定，之后不再改变。
    pub scope: String,
    /// 团队会话所属的团队。
    pub team_id: Option<String>,
    /// 个人会话里挂进来的团队。
    pub mounted_team_ids: Vec<String>,
    /// 回答用的语言：建会话时发起人的界面语言；空表示不指定。
    pub reply_language: String,
    /// 上下文压缩的摘要：截至 `context_summary_upto_micros`（含）的消息交给模型时换成它。
    pub context_summary: Option<String>,
    pub context_summary_upto_micros: Option<i64>,
    /// 会话调查的对象，例如从事件页发起时的 `incident` 与事件 ID。
    pub subject_kind: Option<String>,
    pub subject_id: Option<String>,
    /// 这个会话写的调查记录。
    pub investigation_id: Option<String>,
    /// 会话结束后提炼记忆：已经提炼到的位置（当时的 `updated_at`）。
    pub memory_distilled_at_micros: Option<i64>,
    /// 从哪个会话分叉出来的。
    pub parent_chat_id: Option<String>,
    /// 标题还是对话页取自第一个问题的临时标题，回答时由模型起名。
    pub title_provisional: bool,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

impl Chat {
    /// 最小构造：仅必填字段，其余 Mole Agent 扩展列留空。
    pub fn minimal(
        id: Id,
        org_id: Id,
        user_id: Id,
        provider: String,
        model: String,
        title: String,
        now: TimestampMicros,
    ) -> Self {
        Self {
            id,
            org_id,
            user_id,
            provider,
            model,
            title,
            provider_id: None,
            analysis_mode: None,
            time_range_start_micros: None,
            time_range_end_micros: None,
            archive_object_key: None,
            deleted_at_micros: None,
            scope: "personal".into(),
            team_id: None,
            mounted_team_ids: Vec::new(),
            reply_language: String::new(),
            context_summary: None,
            context_summary_upto_micros: None,
            subject_kind: None,
            subject_id: None,
            investigation_id: None,
            memory_distilled_at_micros: None,
            parent_chat_id: None,
            title_provisional: false,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: Id,
    pub chat_id: Id,
    pub org_id: Id,
    pub role: String,
    pub content: String,
    /// 用户消息的发送人。
    pub user_id: Option<Id>,
    pub tool_calls_json: Option<Value>,
    pub tool_result_for: Option<String>,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    /// 这次回答的成本，用模型服务选的币种记；没有单价时为空。
    pub cost: Option<Cost>,
    /// 本条 Mole Agent 消息使用的 prompt 引用 + 渲染 hash（可追溯）。
    pub prompt_template_id: Option<String>,
    pub prompt_builtin_key: Option<String>,
    pub prompt_version: Option<i32>,
    pub prompt_hash: Option<String>,
    /// tool-call evidence 摘要数组（compact；大原始结果 spill 到对象存储，这里只留 object_key）。
    pub evidence_json: Option<Value>,
    /// 用户消息是斜杠命令时记下的命令（`agent::commands::CommandRecord`）；`content` 仍是原文。
    pub command_json: Option<Value>,
    /// 回答出自哪个 Run。
    pub run_id: Option<Id>,
    /// 回答过程中模型走过的步骤（`agent::runs::TranscriptStep` 数组）。
    pub tool_transcript: Option<Value>,
    /// 用户消息里用 @ 插入的引用（`agent::references::MessageReference` 数组）。
    pub references_json: Option<Value>,
    pub created_at: TimestampMicros,
}

impl ChatMessage {
    /// 最小构造：核心字段，prompt/evidence 扩展列留空。
    #[allow(clippy::too_many_arguments)]
    pub fn minimal(
        id: Id,
        chat_id: Id,
        org_id: Id,
        role: impl Into<String>,
        content: impl Into<String>,
        now: TimestampMicros,
    ) -> Self {
        Self {
            id,
            chat_id,
            org_id,
            role: role.into(),
            content: content.into(),
            user_id: None,
            tool_calls_json: None,
            tool_result_for: None,
            prompt_tokens: None,
            completion_tokens: None,
            cost: None,
            prompt_template_id: None,
            prompt_builtin_key: None,
            prompt_version: None,
            prompt_hash: None,
            evidence_json: None,
            command_json: None,
            run_id: None,
            tool_transcript: None,
            references_json: None,
            created_at: now,
        }
    }
}

#[async_trait]
pub trait ChatRepository: Send + Sync {
    async fn create_chat(&self, s: Chat) -> Result<Chat>;
    /// 建一个分叉会话（`fork.parent_chat_id` 是来源），并在同一个事务里复制来源的消息：到
    /// `upto`（含）为止，没有时全部。
    async fn fork_chat(&self, fork: Chat, upto: Option<&ChatMessage>) -> Result<Chat>;
    /// 取活跃会话（deleted_at IS NULL）。
    async fn get_chat(&self, org_id: &Id, id: &Id) -> Result<Chat>;
    /// 取会话（含软删）。供归档 / 审计保留路径使用。
    async fn get_chat_any(&self, org_id: &Id, id: &Id) -> Result<Chat>;
    /// 取这些活跃会话；不存在或已删除的不在结果里。
    async fn get_chats(&self, org_id: &Id, ids: &[Id]) -> Result<Vec<Chat>>;
    /// 给标题还是临时的会话起名；标题已经起过时不变，返回是否起了。
    async fn name_chat(&self, id: &Id, title: &str) -> Result<bool>;
    /// 人给会话改名：之后不再由模型起名。
    async fn rename_chat(&self, id: &Id, title: &str) -> Result<()>;
    /// 一个人的个人会话。
    async fn list_chats(&self, org_id: &Id, user_id: &Id) -> Result<Vec<Chat>>;
    /// 这些团队的团队会话。
    async fn list_team_chats(&self, org_id: &Id, team_ids: &[String]) -> Result<Vec<Chat>>;
    /// 把一个团队挂进个人会话；已经挂过时不变。
    async fn mount_team(&self, id: &Id, team_id: &str) -> Result<()>;
    async fn touch_chat(&self, id: &Id, at: TimestampMicros) -> Result<()>;
    /// 软删：标记 deleted_at_micros，不物删消息。
    async fn delete_chat(&self, org_id: &Id, id: &Id) -> Result<()>;
    /// 记录归档 object_key（最近一次成功归档）。
    async fn set_archive_object_key(&self, id: &Id, object_key: &str) -> Result<()>;
    /// 持久化本次请求的 analysis_mode + time_range 到会话（COALESCE：传 None 不覆盖旧值）。
    async fn set_chat_context(
        &self,
        id: &Id,
        analysis_mode: Option<&str>,
        time_range_start: Option<i64>,
        time_range_end: Option<i64>,
    ) -> Result<()>;
    /// Sync the provider/model actually used by a request back onto the chat.
    async fn set_chat_provider(
        &self,
        id: &Id,
        provider: &str,
        model: &str,
        provider_id: Option<&str>,
    ) -> Result<()>;

    /// 记下上下文压缩的摘要，以及它替代的消息到哪一条为止（含）。
    async fn set_context_summary(&self, id: &Id, summary: &str, upto_micros: i64) -> Result<()>;
    /// 挑出该提炼记忆的会话并记下提炼到的位置：组织打开了提炼、停下超过 `idle_before`、之后
    /// 有新消息。返回会话与上一次提炼到的位置；两个节点同时挑时一个会话只给一个。
    async fn due_for_distillation(
        &self,
        idle_before: TimestampMicros,
        limit: i64,
    ) -> Result<Vec<(Chat, Option<i64>)>>;

    async fn append_message(&self, m: ChatMessage) -> Result<ChatMessage>;
    /// 按时间先后；同一时刻的按 ID。
    async fn list_messages(&self, chat_id: &Id) -> Result<Vec<ChatMessage>>;
}

pub struct PgChatRepository {
    pool: PgPool,
}

impl PgChatRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ChatRepository for PgChatRepository {
    async fn create_chat(&self, s: Chat) -> Result<Chat> {
        rows::insert_chat(&self.pool, &s).await?;
        Ok(s)
    }

    async fn fork_chat(&self, fork: Chat, upto: Option<&ChatMessage>) -> Result<Chat> {
        fork::fork(&self.pool, fork, upto).await
    }

    async fn get_chat(&self, org_id: &Id, id: &Id) -> Result<Chat> {
        let row = sqlx::query(&format!(
            "SELECT {CHAT_COLS} FROM agent_chats
             WHERE org_id = $1 AND id = $2 AND deleted_at_micros IS NULL"
        ))
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(chat_row(row))
    }

    async fn get_chat_any(&self, org_id: &Id, id: &Id) -> Result<Chat> {
        let row = sqlx::query(&format!(
            "SELECT {CHAT_COLS} FROM agent_chats WHERE org_id = $1 AND id = $2"
        ))
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(chat_row(row))
    }

    async fn get_chats(&self, org_id: &Id, ids: &[Id]) -> Result<Vec<Chat>> {
        titles::get_chats(&self.pool, org_id, ids).await
    }

    async fn name_chat(&self, id: &Id, title: &str) -> Result<bool> {
        titles::name_chat(&self.pool, id, title).await
    }

    async fn rename_chat(&self, id: &Id, title: &str) -> Result<()> {
        titles::rename_chat(&self.pool, id, title).await
    }

    async fn list_chats(&self, org_id: &Id, user_id: &Id) -> Result<Vec<Chat>> {
        let rows = sqlx::query(&format!(
            "SELECT {CHAT_COLS} FROM agent_chats
             WHERE org_id = $1 AND user_id = $2 AND deleted_at_micros IS NULL
             ORDER BY updated_at_micros DESC LIMIT 200"
        ))
        .bind(&org_id.0)
        .bind(&user_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(rows.into_iter().map(chat_row).collect())
    }

    async fn list_team_chats(&self, org_id: &Id, team_ids: &[String]) -> Result<Vec<Chat>> {
        if team_ids.is_empty() {
            return Ok(Vec::new());
        }

        let rows = sqlx::query(&format!(
            "SELECT {CHAT_COLS} FROM agent_chats
             WHERE org_id = $1 AND team_id = ANY($2) AND deleted_at_micros IS NULL
             ORDER BY updated_at_micros DESC LIMIT 200"
        ))
        .bind(&org_id.0)
        .bind(team_ids)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(rows.into_iter().map(chat_row).collect())
    }

    async fn mount_team(&self, id: &Id, team_id: &str) -> Result<()> {
        sqlx::query(
            "UPDATE agent_chats
             SET mounted_team_ids = mounted_team_ids || jsonb_build_array($2::TEXT)
             WHERE id = $1 AND scope = 'personal'
               AND NOT mounted_team_ids @> jsonb_build_array($2::TEXT)",
        )
        .bind(&id.0)
        .bind(team_id)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn touch_chat(&self, id: &Id, at: TimestampMicros) -> Result<()> {
        sqlx::query("UPDATE agent_chats SET updated_at_micros = $2 WHERE id = $1")
            .bind(&id.0)
            .bind(at.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(())
    }

    async fn delete_chat(&self, org_id: &Id, id: &Id) -> Result<()> {
        // 软删：标记时间戳，保留消息行供归档/审计保留路径解析。
        sqlx::query(
            "UPDATE agent_chats SET deleted_at_micros = $3
             WHERE org_id = $1 AND id = $2 AND deleted_at_micros IS NULL",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .bind(TimestampMicros::now().0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn set_archive_object_key(&self, id: &Id, object_key: &str) -> Result<()> {
        sqlx::query("UPDATE agent_chats SET archive_object_key = $2 WHERE id = $1")
            .bind(&id.0)
            .bind(object_key)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(())
    }

    async fn set_chat_context(
        &self,
        id: &Id,
        analysis_mode: Option<&str>,
        time_range_start: Option<i64>,
        time_range_end: Option<i64>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE agent_chats SET
                analysis_mode = COALESCE($2, analysis_mode),
                time_range_start_micros = COALESCE($3, time_range_start_micros),
                time_range_end_micros = COALESCE($4, time_range_end_micros)
             WHERE id = $1",
        )
        .bind(&id.0)
        .bind(analysis_mode)
        .bind(time_range_start)
        .bind(time_range_end)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn set_chat_provider(
        &self,
        id: &Id,
        provider: &str,
        model: &str,
        provider_id: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE agent_chats SET
                provider = $2,
                model = $3,
                provider_id = $4,
                updated_at_micros = $5
             WHERE id = $1",
        )
        .bind(&id.0)
        .bind(provider)
        .bind(model)
        .bind(provider_id)
        .bind(TimestampMicros::now().0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn set_context_summary(&self, id: &Id, summary: &str, upto_micros: i64) -> Result<()> {
        sqlx::query(
            "UPDATE agent_chats SET context_summary = $2, context_summary_upto_micros = $3
             WHERE id = $1",
        )
        .bind(&id.0)
        .bind(summary)
        .bind(upto_micros)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn due_for_distillation(
        &self,
        idle_before: TimestampMicros,
        limit: i64,
    ) -> Result<Vec<(Chat, Option<i64>)>> {
        distillation::due(&self.pool, idle_before, limit).await
    }

    async fn append_message(&self, m: ChatMessage) -> Result<ChatMessage> {
        sqlx::query(
            "INSERT INTO agent_messages
                (id, chat_id, org_id, role, content, user_id, tool_calls_json, tool_result_for,
                 prompt_tokens, completion_tokens, cost, cost_currency, prompt_template_id,
                 prompt_builtin_key, prompt_version, prompt_hash, evidence_json, command_json,
                 created_at_micros, run_id, tool_transcript, references_json)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16,
                     $17, $18, $19, $20, $21, $22)",
        )
        .bind(&m.id.0)
        .bind(&m.chat_id.0)
        .bind(&m.org_id.0)
        .bind(&m.role)
        .bind(&m.content)
        .bind(m.user_id.as_ref().map(|id| id.0.as_str()))
        .bind(m.tool_calls_json.as_ref().map(Json))
        .bind(&m.tool_result_for)
        .bind(m.prompt_tokens)
        .bind(m.completion_tokens)
        .bind(m.cost.map(|cost| cost.amount))
        .bind(m.cost.map(|cost| cost.currency.as_str().to_string()))
        .bind(&m.prompt_template_id)
        .bind(&m.prompt_builtin_key)
        .bind(m.prompt_version)
        .bind(&m.prompt_hash)
        .bind(m.evidence_json.as_ref().map(Json))
        .bind(m.command_json.as_ref().map(Json))
        .bind(m.created_at.0)
        .bind(m.run_id.as_ref().map(|id| id.0.as_str()))
        .bind(m.tool_transcript.as_ref().map(Json))
        .bind(m.references_json.as_ref().map(Json))
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(m)
    }

    async fn list_messages(&self, chat_id: &Id) -> Result<Vec<ChatMessage>> {
        let rows = sqlx::query(&format!(
            "SELECT {MESSAGE_COLS} FROM agent_messages
             WHERE chat_id = $1
             ORDER BY created_at_micros ASC, id ASC"
        ))
        .bind(&chat_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(rows.into_iter().map(message_row).collect())
    }
}
