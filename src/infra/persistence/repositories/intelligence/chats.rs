// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `intelligence_chats` + `intelligence_messages` 表 Pg 实装。
//!
//! Mole Intelligence 扩展：chat 记 provider_id / analysis_mode /
//! time_range / archive 指针 / soft-delete；message 记 prompt 引用 + evidence 摘要。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgPool, Row, types::Json};

use super::super::sqlx_err;
use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chat {
    pub id: Id,
    pub org_id: Id,
    pub user_id: Id,
    pub provider: String,
    pub model: String,
    pub title: String,
    /// Mole Intelligence chat 绑定的 PG provider 行 id（区别于 `provider` 类型串）。
    pub provider_id: Option<String>,
    /// 分析模式：anomaly_analysis | root_cause | alert_explain | query_generation | 自由问答。
    pub analysis_mode: Option<String>,
    pub time_range_start_micros: Option<i64>,
    pub time_range_end_micros: Option<i64>,
    /// 归档对象存储 key（最近一次成功归档）。
    pub archive_object_key: Option<String>,
    /// 软删时间戳；非 NULL = 已删除（normal history 列表过滤掉）。
    pub deleted_at_micros: Option<i64>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

impl Chat {
    /// 最小构造：仅必填字段，其余 Mole Intelligence 扩展列留空。
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
    pub tool_calls_json: Option<Value>,
    pub tool_result_for: Option<String>,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub cost_usd: Option<f64>,
    /// 本条 Mole Agent 消息使用的 prompt 引用 + 渲染 hash（可追溯）。
    pub prompt_template_id: Option<String>,
    pub prompt_builtin_key: Option<String>,
    pub prompt_version: Option<i32>,
    pub prompt_hash: Option<String>,
    /// tool-call evidence 摘要数组（compact；大原始结果 spill 到对象存储，这里只留 object_key）。
    pub evidence_json: Option<Value>,
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
            tool_calls_json: None,
            tool_result_for: None,
            prompt_tokens: None,
            completion_tokens: None,
            cost_usd: None,
            prompt_template_id: None,
            prompt_builtin_key: None,
            prompt_version: None,
            prompt_hash: None,
            evidence_json: None,
            created_at: now,
        }
    }
}

#[async_trait]
pub trait ChatRepository: Send + Sync {
    async fn create_chat(&self, s: Chat) -> Result<Chat>;
    /// 取活跃会话（deleted_at IS NULL）。
    async fn get_chat(&self, org_id: &Id, id: &Id) -> Result<Chat>;
    /// 取会话（含软删）。供归档 / 审计保留路径使用。
    async fn get_chat_any(&self, org_id: &Id, id: &Id) -> Result<Chat>;
    async fn list_chats(&self, org_id: &Id, user_id: &Id) -> Result<Vec<Chat>>;
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

    async fn append_message(&self, m: ChatMessage) -> Result<ChatMessage>;
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

const CHAT_COLS: &str = "id, org_id, user_id, provider, model, title, provider_id, \
    analysis_mode, time_range_start_micros, time_range_end_micros, archive_object_key, \
    deleted_at_micros, created_at_micros, updated_at_micros";

const MESSAGE_COLS: &str = "id, chat_id, org_id, role, content, tool_calls_json, \
    tool_result_for, prompt_tokens, completion_tokens, cost_usd, prompt_template_id, \
    prompt_builtin_key, prompt_version, prompt_hash, evidence_json, created_at_micros";

fn chat_row(r: sqlx::postgres::PgRow) -> Chat {
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
        created_at: TimestampMicros(r.try_get::<i64, _>("created_at_micros").unwrap_or_default()),
        updated_at: TimestampMicros(r.try_get::<i64, _>("updated_at_micros").unwrap_or_default()),
    }
}

fn message_row(r: sqlx::postgres::PgRow) -> ChatMessage {
    let tools: Option<Json<Value>> = r.try_get("tool_calls_json").ok();
    let evidence: Option<Json<Value>> = r.try_get("evidence_json").ok();
    ChatMessage {
        id: Id(r.try_get::<String, _>("id").unwrap_or_default()),
        chat_id: Id(r.try_get::<String, _>("chat_id").unwrap_or_default()),
        org_id: Id(r.try_get::<String, _>("org_id").unwrap_or_default()),
        role: r.try_get::<String, _>("role").unwrap_or_default(),
        content: r.try_get::<String, _>("content").unwrap_or_default(),
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
        cost_usd: r.try_get::<Option<f64>, _>("cost_usd").unwrap_or_default(),
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
        created_at: TimestampMicros(r.try_get::<i64, _>("created_at_micros").unwrap_or_default()),
    }
}

#[async_trait]
impl ChatRepository for PgChatRepository {
    async fn create_chat(&self, s: Chat) -> Result<Chat> {
        sqlx::query(
            "INSERT INTO intelligence_chats
                (id, org_id, user_id, provider, model, title, provider_id, analysis_mode,
                 time_range_start_micros, time_range_end_micros, archive_object_key,
                 deleted_at_micros, created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
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
        .bind(s.created_at.0)
        .bind(s.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(s)
    }

    async fn get_chat(&self, org_id: &Id, id: &Id) -> Result<Chat> {
        let row = sqlx::query(&format!(
            "SELECT {CHAT_COLS} FROM intelligence_chats
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
            "SELECT {CHAT_COLS} FROM intelligence_chats WHERE org_id = $1 AND id = $2"
        ))
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(chat_row(row))
    }

    async fn list_chats(&self, org_id: &Id, user_id: &Id) -> Result<Vec<Chat>> {
        let rows = sqlx::query(&format!(
            "SELECT {CHAT_COLS} FROM intelligence_chats
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

    async fn touch_chat(&self, id: &Id, at: TimestampMicros) -> Result<()> {
        sqlx::query("UPDATE intelligence_chats SET updated_at_micros = $2 WHERE id = $1")
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
            "UPDATE intelligence_chats SET deleted_at_micros = $3
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
        sqlx::query("UPDATE intelligence_chats SET archive_object_key = $2 WHERE id = $1")
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
            "UPDATE intelligence_chats SET
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
            "UPDATE intelligence_chats SET
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

    async fn append_message(&self, m: ChatMessage) -> Result<ChatMessage> {
        sqlx::query(
            "INSERT INTO intelligence_messages
                (id, chat_id, org_id, role, content, tool_calls_json, tool_result_for,
                 prompt_tokens, completion_tokens, cost_usd, prompt_template_id,
                 prompt_builtin_key, prompt_version, prompt_hash, evidence_json, created_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)",
        )
        .bind(&m.id.0)
        .bind(&m.chat_id.0)
        .bind(&m.org_id.0)
        .bind(&m.role)
        .bind(&m.content)
        .bind(m.tool_calls_json.as_ref().map(Json))
        .bind(&m.tool_result_for)
        .bind(m.prompt_tokens)
        .bind(m.completion_tokens)
        .bind(m.cost_usd)
        .bind(&m.prompt_template_id)
        .bind(&m.prompt_builtin_key)
        .bind(m.prompt_version)
        .bind(&m.prompt_hash)
        .bind(m.evidence_json.as_ref().map(Json))
        .bind(m.created_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(m)
    }

    async fn list_messages(&self, chat_id: &Id) -> Result<Vec<ChatMessage>> {
        let rows = sqlx::query(&format!(
            "SELECT {MESSAGE_COLS} FROM intelligence_messages
             WHERE chat_id = $1
             ORDER BY created_at_micros ASC"
        ))
        .bind(&chat_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(rows.into_iter().map(message_row).collect())
    }
}
