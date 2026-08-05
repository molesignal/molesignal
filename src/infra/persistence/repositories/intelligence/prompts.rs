// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `intelligence_prompt_templates` 表 Pg 实装。
//!
//! builtin 行（scope=builtin, org_id NULL）由迁移 seed，**不可变**。Admin 建
//! org-scoped override、有 prompt-write 权限的用户建 user-scoped override；每个
//! override 经 `parent_id` + `builtin_key` + 递增 `version` 追溯到 builtin 源。
//!
//! 运行时 prompt 解析（[`AgentPromptRepository::resolve`]）顺序：
//!   user default → org default → builtin default（按 purpose）。
//! 显式 `prompt_template_id` 由调用方先走 [`AgentPromptRepository::get`]。

use async_trait::async_trait;
use serde::Serialize;
use serde_json::{Map, Value};
use sqlx::{PgPool, Row, types::Json};

use super::super::sqlx_err;
use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Serialize)]
pub struct AgentPromptTemplate {
    pub id: Id,
    /// builtin scope 为 None（全局共享）；org/user override 带 org_id。
    pub org_id: Option<String>,
    pub user_id: Option<String>,
    pub scope: String,
    pub builtin_key: Option<String>,
    pub purpose: String,
    pub name: String,
    pub body: String,
    pub variables_schema: Value,
    pub is_default: bool,
    pub enabled: bool,
    pub version: i32,
    pub parent_id: Option<String>,
    pub created_by: Option<String>,
    pub updated_by: Option<String>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[async_trait]
pub trait AgentPromptRepository: Send + Sync {
    /// 列出 builtin（全局）+ 本 org override + 本 user override。
    async fn list(&self, org_id: &Id, user_id: &Id) -> Result<Vec<AgentPromptTemplate>>;
    /// 按 id 取（不限 scope；调用方负责可见性校验）。
    async fn get(&self, id: &Id) -> Result<AgentPromptTemplate>;
    /// 按 builtin_key 取 builtin 行（restore-from-builtin / builtin 默认解析）。
    async fn get_builtin(&self, builtin_key: &str) -> Result<AgentPromptTemplate>;
    async fn create(&self, t: AgentPromptTemplate) -> Result<AgentPromptTemplate>;
    /// 更新 override（递增 version）；builtin scope 拒绝。
    async fn update(&self, t: AgentPromptTemplate) -> Result<AgentPromptTemplate>;
    /// 设默认：同 (scope, owner, purpose) 内清掉其它默认，置本行 is_default=TRUE。
    async fn set_default(&self, id: &Id) -> Result<()>;
    /// 删除 override；builtin scope 拒绝。
    async fn delete(&self, id: &Id) -> Result<()>;
    /// 解析 purpose 的活跃 prompt：user default → org default → builtin default。
    async fn resolve(
        &self,
        org_id: &Id,
        user_id: &Id,
        purpose: &str,
    ) -> Result<AgentPromptTemplate>;
}

pub struct PgAgentPromptRepository {
    pool: PgPool,
}

impl PgAgentPromptRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, user_id, scope, builtin_key, purpose, name, body, \
    variables_schema, is_default, enabled, version, parent_id, created_by, updated_by, \
    created_at_micros, updated_at_micros";

fn row_to(r: sqlx::postgres::PgRow) -> Result<AgentPromptTemplate> {
    let schema: Json<Value> = r.try_get("variables_schema").map_err(sqlx_err)?;
    Ok(AgentPromptTemplate {
        id: Id(r.try_get::<String, _>("id").map_err(sqlx_err)?),
        org_id: r.try_get("org_id").map_err(sqlx_err)?,
        user_id: r.try_get("user_id").map_err(sqlx_err)?,
        scope: r.try_get("scope").map_err(sqlx_err)?,
        builtin_key: r.try_get("builtin_key").map_err(sqlx_err)?,
        purpose: r.try_get("purpose").map_err(sqlx_err)?,
        name: r.try_get("name").map_err(sqlx_err)?,
        body: r.try_get("body").map_err(sqlx_err)?,
        variables_schema: schema.0,
        is_default: r.try_get("is_default").map_err(sqlx_err)?,
        enabled: r.try_get("enabled").map_err(sqlx_err)?,
        version: r.try_get("version").map_err(sqlx_err)?,
        parent_id: r.try_get("parent_id").map_err(sqlx_err)?,
        created_by: r.try_get("created_by").map_err(sqlx_err)?,
        updated_by: r.try_get("updated_by").map_err(sqlx_err)?,
        created_at: TimestampMicros(r.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(r.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

#[async_trait]
impl AgentPromptRepository for PgAgentPromptRepository {
    async fn list(&self, org_id: &Id, user_id: &Id) -> Result<Vec<AgentPromptTemplate>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM intelligence_prompt_templates
             WHERE scope = 'builtin'
                OR (scope = 'org' AND org_id = $1)
                OR (scope = 'user' AND org_id = $1 AND user_id = $2)
             ORDER BY scope, purpose, updated_at_micros DESC"
        ))
        .bind(&org_id.0)
        .bind(&user_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(row_to).collect()
    }

    async fn get(&self, id: &Id) -> Result<AgentPromptTemplate> {
        let row = sqlx::query(&format!(
            "SELECT {COLS} FROM intelligence_prompt_templates WHERE id = $1"
        ))
        .bind(&id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| Error::not_found(format!("ai prompt `{}` not found", id.0)))?;
        row_to(row)
    }

    async fn get_builtin(&self, builtin_key: &str) -> Result<AgentPromptTemplate> {
        let row = sqlx::query(&format!(
            "SELECT {COLS} FROM intelligence_prompt_templates
             WHERE scope = 'builtin' AND builtin_key = $1"
        ))
        .bind(builtin_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| Error::not_found(format!("builtin prompt `{builtin_key}` not found")))?;
        row_to(row)
    }

    async fn create(&self, t: AgentPromptTemplate) -> Result<AgentPromptTemplate> {
        if t.scope == "builtin" {
            return Err(Error::invalid("cannot create builtin prompt via API"));
        }
        sqlx::query(
            "INSERT INTO intelligence_prompt_templates
                (id, org_id, user_id, scope, builtin_key, purpose, name, body, variables_schema,
                 is_default, enabled, version, parent_id, created_by, updated_by,
                 created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $16)",
        )
        .bind(&t.id.0)
        .bind(&t.org_id)
        .bind(&t.user_id)
        .bind(&t.scope)
        .bind(&t.builtin_key)
        .bind(&t.purpose)
        .bind(&t.name)
        .bind(&t.body)
        .bind(Json(&t.variables_schema))
        .bind(t.is_default)
        .bind(t.enabled)
        .bind(t.version)
        .bind(&t.parent_id)
        .bind(&t.created_by)
        .bind(&t.updated_by)
        .bind(t.created_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        self.get(&t.id).await
    }

    async fn update(&self, t: AgentPromptTemplate) -> Result<AgentPromptTemplate> {
        let existing = self.get(&t.id).await?;
        if existing.scope == "builtin" {
            return Err(Error::invalid("builtin prompts are immutable"));
        }
        let now = TimestampMicros::now();
        sqlx::query(
            "UPDATE intelligence_prompt_templates SET
                name = $2, body = $3, variables_schema = $4, enabled = $5,
                version = version + 1, updated_by = $6, updated_at_micros = $7
             WHERE id = $1",
        )
        .bind(&t.id.0)
        .bind(&t.name)
        .bind(&t.body)
        .bind(Json(&t.variables_schema))
        .bind(t.enabled)
        .bind(&t.updated_by)
        .bind(now.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        self.get(&t.id).await
    }

    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "intelligence_prompt_templates")
    )]
    async fn set_default(&self, id: &Id) -> Result<()> {
        let t = self.get(id).await?;
        if t.scope == "builtin" {
            return Err(Error::invalid("builtin defaults are managed by the system"));
        }
        let now = TimestampMicros::now();
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        // 清掉同 scope + owner + purpose 的其它默认。
        sqlx::query(
            "UPDATE intelligence_prompt_templates SET is_default = FALSE, updated_at_micros = $5
             WHERE scope = $1 AND purpose = $2
               AND org_id IS NOT DISTINCT FROM $3 AND user_id IS NOT DISTINCT FROM $4",
        )
        .bind(&t.scope)
        .bind(&t.purpose)
        .bind(&t.org_id)
        .bind(&t.user_id)
        .bind(now.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
        sqlx::query(
            "UPDATE intelligence_prompt_templates SET is_default = TRUE, enabled = TRUE, updated_at_micros = $2
             WHERE id = $1",
        )
        .bind(&id.0)
        .bind(now.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
        tx.commit().await.map_err(sqlx_err)?;
        Ok(())
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let t = self.get(id).await?;
        if t.scope == "builtin" {
            return Err(Error::invalid("builtin prompts cannot be deleted"));
        }
        sqlx::query("DELETE FROM intelligence_prompt_templates WHERE id = $1")
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(())
    }

    async fn resolve(
        &self,
        org_id: &Id,
        user_id: &Id,
        purpose: &str,
    ) -> Result<AgentPromptTemplate> {
        // user default → org default → builtin default。
        let row = sqlx::query(&format!(
            "SELECT {COLS} FROM intelligence_prompt_templates
             WHERE purpose = $1 AND enabled = TRUE
               AND (
                   (scope = 'user' AND org_id = $2 AND user_id = $3 AND is_default = TRUE)
                OR (scope = 'org'  AND org_id = $2 AND is_default = TRUE)
                OR (scope = 'builtin' AND is_default = TRUE)
               )
             ORDER BY CASE scope WHEN 'user' THEN 0 WHEN 'org' THEN 1 ELSE 2 END
             LIMIT 1"
        ))
        .bind(purpose)
        .bind(&org_id.0)
        .bind(&user_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| Error::not_found(format!("no prompt resolved for purpose `{purpose}`")))?;
        row_to(row)
    }
}

// ---------------------------------------------------------------------------
// Prompt 变量校验 + 渲染（task 2.4 / 3.3）
// ---------------------------------------------------------------------------

/// 抽取 body 中 `{{ var }}` 引用的变量名（去重，保序）。
pub fn referenced_variables(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = body.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'{'
            && bytes[i + 1] == b'{'
            && let Some(end) = body[i + 2..].find("}}")
        {
            let raw = &body[i + 2..i + 2 + end];
            let name = raw.trim().to_string();
            if !name.is_empty() && !out.contains(&name) {
                out.push(name);
            }
            i = i + 2 + end + 2;
            continue;
        }
        i += 1;
    }
    out
}

/// `variables_schema` 允许的变量名集合（取 `properties` 的 key）。
pub fn allowed_variables(variables_schema: &Value) -> Vec<String> {
    variables_schema
        .get("properties")
        .and_then(|p| p.as_object())
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
}

/// 校验 body 只引用 `variables_schema` 允许的变量；否则返 `InvalidArgument`。
pub fn validate_template_variables(body: &str, variables_schema: &Value) -> Result<()> {
    let allowed = allowed_variables(variables_schema);
    for var in referenced_variables(body) {
        if !allowed.contains(&var) {
            return Err(Error::invalid(format!(
                "prompt references unknown variable `{{{{{var}}}}}` not in variables_schema"
            )));
        }
    }
    Ok(())
}

/// 渲染：把 `{{ var }}` 替换为 `vars` 中的值；未提供的变量替换为空串。
pub fn render_prompt(body: &str, vars: &Map<String, Value>) -> String {
    let mut out = String::with_capacity(body.len());
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < body.len() {
        if i + 1 < bytes.len()
            && bytes[i] == b'{'
            && bytes[i + 1] == b'{'
            && let Some(end) = body[i + 2..].find("}}")
        {
            let name = body[i + 2..i + 2 + end].trim();
            let val = vars
                .get(name)
                .map(|v| match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .unwrap_or_default();
            out.push_str(&val);
            i = i + 2 + end + 2;
            continue;
        }
        // 推进一个 UTF-8 char，避免在多字节边界切割。
        let ch_len = body[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        out.push_str(&body[i..i + ch_len]);
        i += ch_len;
    }
    out
}

/// 渲染后 prompt 的 SHA-256（hex），用于可追溯持久化与审计。
pub fn prompt_hash(rendered: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(rendered.as_bytes());
    let digest = h.finalize();
    let mut s = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
    }
    s
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn referenced_variables_extracts_and_dedups() {
        let vars = referenced_variables("hi {{ a }} and {{b}} and {{ a }}");
        assert_eq!(vars, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn validate_rejects_unknown_variable() {
        let schema = json!({"type":"object","properties":{"time_range":{}}});
        assert!(validate_template_variables("look at {{time_range}}", &schema).is_ok());
        assert!(validate_template_variables("look at {{secret}}", &schema).is_err());
    }

    #[test]
    fn render_substitutes_known_and_blanks_unknown() {
        let mut vars = Map::new();
        vars.insert("name".into(), json!("ops"));
        let out = render_prompt("hello {{ name }} / {{ missing }}!", &vars);
        assert_eq!(out, "hello ops / !");
    }

    #[test]
    fn prompt_hash_is_stable_hex() {
        let h = prompt_hash("abc");
        assert_eq!(
            h,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
