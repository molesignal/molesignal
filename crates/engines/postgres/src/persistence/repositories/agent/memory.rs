// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `agent_memories` 与 `agent_memory_settings`：个人与团队的记忆，组织对记忆的设置。
//!
//! 同一作用域里没忘掉的记忆名称不分大小写唯一（部分唯一索引）；按名称记住时替换同名的那条。

use async_trait::async_trait;
use sqlx::{PgPool, Row, postgres::PgRow};

use crate::{
    agent::{
        memory::{
            MAX_MEMORIES, Memory, MemoryDraft, MemoryKind, MemorySettings, MemoryStore,
            check_memory,
        },
        scope::Scope,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
    sqlx_err,
};

const COLS: &str = "id, org_id, scope, team_id, owner_id, kind, name, description, body, \
    source_chat_id, created_by, updated_by, created_at_micros, updated_at_micros, \
    forgotten_at_micros";

pub struct PgMemoryStore {
    pool: PgPool,
}

impl PgMemoryStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn memory_row(row: PgRow) -> Result<Memory> {
    let scope: String = row.try_get("scope").map_err(sqlx_err)?;
    let team_id: Option<String> = row.try_get("team_id").map_err(sqlx_err)?;
    let owner_id: Option<String> = row.try_get("owner_id").map_err(sqlx_err)?;
    let kind: String = row.try_get("kind").map_err(sqlx_err)?;

    Ok(Memory {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        scope: Scope::from_parts(&scope, team_id.as_deref(), owner_id.as_deref())?,
        kind: MemoryKind::parse(&kind)
            .ok_or_else(|| Error::internal(format!("unknown memory kind `{kind}`")))?,
        name: row.try_get("name").map_err(sqlx_err)?,
        description: row.try_get("description").map_err(sqlx_err)?,
        body: row.try_get("body").map_err(sqlx_err)?,
        source_chat_id: row
            .try_get::<Option<String>, _>("source_chat_id")
            .map_err(sqlx_err)?
            .map(Id),
        created_by: Id(row.try_get("created_by").map_err(sqlx_err)?),
        updated_by: Id(row.try_get("updated_by").map_err(sqlx_err)?),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
        forgotten_at: row
            .try_get::<Option<i64>, _>("forgotten_at_micros")
            .map_err(sqlx_err)?
            .map(TimestampMicros),
    })
}

fn conflict_on_name(error: sqlx::Error) -> Error {
    match sqlx_err(error) {
        Error::Conflict(_) => Error::conflict("a memory with this name already exists"),
        other => other,
    }
}

#[async_trait]
impl MemoryStore for PgMemoryStore {
    async fn list(&self, org_id: &Id, scope: &Scope) -> Result<Vec<Memory>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM agent_memories
             WHERE org_id = $1 AND scope_key = $2 AND forgotten_at_micros IS NULL
             ORDER BY updated_at_micros DESC"
        ))
        .bind(&org_id.0)
        .bind(scope.key())
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(memory_row).collect()
    }

    async fn get(&self, org_id: &Id, id: &Id) -> Result<Memory> {
        let row = sqlx::query(&format!(
            "SELECT {COLS} FROM agent_memories WHERE org_id = $1 AND id = $2"
        ))
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| Error::not_found(format!("memory `{}` not found", id.0)))?;

        memory_row(row)
    }

    async fn find(&self, org_id: &Id, scope: &Scope, name: &str) -> Result<Option<Memory>> {
        sqlx::query(&format!(
            "SELECT {COLS} FROM agent_memories
             WHERE org_id = $1 AND scope_key = $2 AND lower(name) = lower($3)
               AND forgotten_at_micros IS NULL"
        ))
        .bind(&org_id.0)
        .bind(scope.key())
        .bind(name.trim())
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .map(memory_row)
        .transpose()
    }

    async fn remember(
        &self,
        org_id: &Id,
        scope: &Scope,
        draft: &MemoryDraft,
        source_chat_id: Option<&Id>,
        actor: &Id,
    ) -> Result<Memory> {
        check_memory(scope, &draft.name, &draft.description, &draft.body)?;

        if let Some(existing) = self.find(org_id, scope, &draft.name).await? {
            return self.update(org_id, &existing.id, draft, actor).await;
        }

        let count: i64 = sqlx::query(
            "SELECT COUNT(*) AS count FROM agent_memories
             WHERE org_id = $1 AND scope_key = $2 AND forgotten_at_micros IS NULL",
        )
        .bind(&org_id.0)
        .bind(scope.key())
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?
        .try_get("count")
        .map_err(sqlx_err)?;

        if count as usize >= MAX_MEMORIES {
            return Err(Error::invalid(format!(
                "at most {MAX_MEMORIES} memories are kept here; forget some first"
            )));
        }

        let now = TimestampMicros::now();

        let row = sqlx::query(&format!(
            "INSERT INTO agent_memories
                (id, org_id, scope, team_id, owner_id, scope_key, kind, name, description, body,
                 source_chat_id, created_by, updated_by, created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $12, $13, $13)
             RETURNING {COLS}"
        ))
        .bind(Id::new().0)
        .bind(&org_id.0)
        .bind(scope.kind())
        .bind(scope.team_id().map(|id| id.0.as_str()))
        .bind(scope.owner_id().map(|id| id.0.as_str()))
        .bind(scope.key())
        .bind(draft.kind.as_str())
        .bind(draft.name.trim())
        .bind(draft.description.trim())
        .bind(draft.body.trim())
        .bind(source_chat_id.map(|id| id.0.as_str()))
        .bind(&actor.0)
        .bind(now.0)
        .fetch_one(&self.pool)
        .await
        .map_err(conflict_on_name)?;

        memory_row(row)
    }

    async fn update(
        &self,
        org_id: &Id,
        id: &Id,
        draft: &MemoryDraft,
        actor: &Id,
    ) -> Result<Memory> {
        let current = self.get(org_id, id).await?;
        check_memory(&current.scope, &draft.name, &draft.description, &draft.body)?;

        let row = sqlx::query(&format!(
            "UPDATE agent_memories SET kind = $3, name = $4, description = $5, body = $6,
                    updated_by = $7, updated_at_micros = $8
             WHERE org_id = $1 AND id = $2
             RETURNING {COLS}"
        ))
        .bind(&org_id.0)
        .bind(&id.0)
        .bind(draft.kind.as_str())
        .bind(draft.name.trim())
        .bind(draft.description.trim())
        .bind(draft.body.trim())
        .bind(&actor.0)
        .bind(TimestampMicros::now().0)
        .fetch_one(&self.pool)
        .await
        .map_err(conflict_on_name)?;

        memory_row(row)
    }

    async fn forget(&self, org_id: &Id, id: &Id, actor: &Id) -> Result<Memory> {
        let now = TimestampMicros::now();

        let row = sqlx::query(&format!(
            "UPDATE agent_memories
             SET forgotten_at_micros = COALESCE(forgotten_at_micros, $4), updated_by = $3
             WHERE org_id = $1 AND id = $2
             RETURNING {COLS}"
        ))
        .bind(&org_id.0)
        .bind(&id.0)
        .bind(&actor.0)
        .bind(now.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| Error::not_found(format!("memory `{}` not found", id.0)))?;

        memory_row(row)
    }

    async fn restore(&self, org_id: &Id, id: &Id, actor: &Id) -> Result<Memory> {
        let row = sqlx::query(&format!(
            "UPDATE agent_memories
             SET forgotten_at_micros = NULL, updated_by = $3, updated_at_micros = $4
             WHERE org_id = $1 AND id = $2
             RETURNING {COLS}"
        ))
        .bind(&org_id.0)
        .bind(&id.0)
        .bind(&actor.0)
        .bind(TimestampMicros::now().0)
        .fetch_optional(&self.pool)
        .await
        .map_err(conflict_on_name)?
        .ok_or_else(|| Error::not_found(format!("memory `{}` not found", id.0)))?;

        memory_row(row)
    }

    async fn delete(&self, org_id: &Id, id: &Id) -> Result<bool> {
        let deleted = sqlx::query("DELETE FROM agent_memories WHERE org_id = $1 AND id = $2")
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(deleted.rows_affected() > 0)
    }

    async fn settings(&self, org_id: &Id) -> Result<MemorySettings> {
        let row = sqlx::query(
            "SELECT distill_after_chat, updated_by, updated_at_micros FROM agent_memory_settings
             WHERE org_id = $1",
        )
        .bind(&org_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        let Some(row) = row else {
            return Ok(MemorySettings::default());
        };

        Ok(MemorySettings {
            distill_after_chat: row.try_get("distill_after_chat").map_err(sqlx_err)?,
            updated_by: Some(Id(row.try_get("updated_by").map_err(sqlx_err)?)),
            updated_at: Some(TimestampMicros(
                row.try_get("updated_at_micros").map_err(sqlx_err)?,
            )),
        })
    }

    async fn set_settings(
        &self,
        org_id: &Id,
        distill_after_chat: bool,
        actor: &Id,
    ) -> Result<MemorySettings> {
        sqlx::query(
            "INSERT INTO agent_memory_settings (org_id, distill_after_chat, updated_by, updated_at_micros)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (org_id) DO UPDATE SET distill_after_chat = EXCLUDED.distill_after_chat,
                updated_by = EXCLUDED.updated_by, updated_at_micros = EXCLUDED.updated_at_micros",
        )
        .bind(&org_id.0)
        .bind(distill_after_chat)
        .bind(&actor.0)
        .bind(TimestampMicros::now().0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        self.settings(org_id).await
    }
}
