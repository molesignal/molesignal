// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `agent_automations` 与 `agent_automation_runs`。
//!
//! 作用域存成三列（`scope`、`team_id`、`owner_id`）；名称在组织内唯一。列表与详情带上最近一次
//! 运行。API 令牌只存哈希与前缀，读规则时只有前缀。运行记录的 SQL 在 [`runs`]。

mod runs;

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde_json::Value;
use sqlx::{PgPool, Row, postgres::PgRow, types::Json};

use super::super::sqlx_err;
use crate::{
    agent::{
        automations::{
            Automation, AutomationRepository, AutomationRun, AutomationRunStatus,
            LastAutomationRun, RunEnding,
        },
        scope::Scope,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct PgAutomationRepository {
    pool: PgPool,
}

impl PgAutomationRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// 规则连同最近一次运行。
const SELECT: &str = "SELECT a.*, r.id AS last_run_id, r.created_at AS last_run_at, \
    r.status AS last_run_status \
    FROM agent_automations a \
    LEFT JOIN LATERAL ( \
        SELECT id, created_at, status FROM agent_automation_runs \
        WHERE automation_id = a.id ORDER BY created_at DESC LIMIT 1 \
    ) r ON TRUE";

fn json<T: DeserializeOwned>(row: &PgRow, column: &str) -> Result<T> {
    let value: Json<Value> = row.try_get(column).map_err(sqlx_err)?;

    serde_json::from_value(value.0)
        .map_err(|error| Error::internal(format!("stored automation `{column}`: {error}")))
}

fn automation_row(row: PgRow) -> Result<Automation> {
    let kind: String = row.try_get("scope").map_err(sqlx_err)?;
    let team_id: Option<String> = row.try_get("team_id").map_err(sqlx_err)?;
    let owner_id: Option<String> = row.try_get("owner_id").map_err(sqlx_err)?;
    let micros =
        |column: &str| -> Result<Option<TimestampMicros>> { row.try_get(column).map_err(sqlx_err) };

    let last_run = match row
        .try_get::<Option<String>, _>("last_run_id")
        .map_err(sqlx_err)?
    {
        Some(id) => {
            let status: String = row.try_get("last_run_status").map_err(sqlx_err)?;

            Some(LastAutomationRun {
                id: Id(id),
                at: row
                    .try_get::<TimestampMicros, _>("last_run_at")
                    .map_err(sqlx_err)?,
                status: AutomationRunStatus::parse(&status).ok_or_else(|| {
                    Error::internal(format!("unknown automation run status `{status}`"))
                })?,
            })
        }
        None => None,
    };

    Ok(Automation {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        scope: Scope::from_parts(&kind, team_id.as_deref(), owner_id.as_deref())?,
        name: row.try_get("name").map_err(sqlx_err)?,
        description: row.try_get("description").map_err(sqlx_err)?,
        prompt: row.try_get("prompt").map_err(sqlx_err)?,
        enabled: row.try_get("enabled").map_err(sqlx_err)?,
        trigger: json(&row, "trigger")?,
        outputs: json(&row, "outputs")?,
        run_as_user_id: Id(row.try_get("run_as_user_id").map_err(sqlx_err)?),
        language: row.try_get("language").map_err(sqlx_err)?,
        timeout_secs: row
            .try_get::<i32, _>("timeout_secs")
            .map_err(sqlx_err)?
            .try_into()
            .unwrap_or(crate::agent::automations::DEFAULT_TIMEOUT_SECS),
        template: row.try_get("template").map_err(sqlx_err)?,
        api_token_prefix: row.try_get("api_token_prefix").map_err(sqlx_err)?,
        next_run_at: micros("next_run_at")?,
        incident_cursor: micros("incident_cursor")?,
        last_run,
        created_by: Id(row.try_get("created_by").map_err(sqlx_err)?),
        updated_by: Id(row.try_get("updated_by").map_err(sqlx_err)?),
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
        updated_at: row
            .try_get::<TimestampMicros, _>("updated_at")
            .map_err(sqlx_err)?,
    })
}

/// 唯一约束冲突说成名称已被占用。
fn name_conflict(error: sqlx::Error, name: &str) -> Error {
    match &error {
        sqlx::Error::Database(database) if database.code().as_deref() == Some("23505") => {
            Error::conflict(format!("an automation named `{name}` already exists"))
        }
        _ => sqlx_err(error),
    }
}

fn not_found(id: &Id) -> Error {
    Error::not_found(format!("automation `{}` not found", id.0))
}

impl PgAutomationRepository {
    async fn fetch_one(&self, org_id: &Id, id: &Id) -> Result<Option<Automation>> {
        sqlx::query(&format!("{SELECT} WHERE a.org_id = $1 AND a.id = $2"))
            .bind(&org_id.0)
            .bind(&id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?
            .map(automation_row)
            .transpose()
    }

    async fn fetch_many(&self, filter: &str, bind: Option<&str>) -> Result<Vec<Automation>> {
        let sql = format!("{SELECT} {filter}");
        let query = sqlx::query(&sql);
        let query = match bind {
            Some(value) => query.bind(value.to_string()),
            None => query,
        };

        query
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?
            .into_iter()
            .map(automation_row)
            .collect()
    }
}

#[async_trait]
impl AutomationRepository for PgAutomationRepository {
    async fn list(&self, org_id: &Id) -> Result<Vec<Automation>> {
        self.fetch_many(
            "WHERE a.org_id = $1 ORDER BY lower(a.name)",
            Some(&org_id.0),
        )
        .await
    }

    async fn get(&self, org_id: &Id, id: &Id) -> Result<Automation> {
        self.fetch_one(org_id, id)
            .await?
            .ok_or_else(|| not_found(id))
    }

    async fn get_with_token(&self, id: &Id) -> Result<Option<(Automation, Option<String>)>> {
        let Some(row) = sqlx::query(&format!("{SELECT} WHERE a.id = $1"))
            .bind(&id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?
        else {
            return Ok(None);
        };

        let hash: Option<String> = row.try_get("api_token_hash").map_err(sqlx_err)?;

        Ok(Some((automation_row(row)?, hash)))
    }

    async fn count(&self, org_id: &Id) -> Result<usize> {
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM agent_automations WHERE org_id = $1")
                .bind(&org_id.0)
                .fetch_one(&self.pool)
                .await
                .map_err(sqlx_err)?;

        Ok(usize::try_from(count).unwrap_or(usize::MAX))
    }

    async fn create(&self, automation: Automation) -> Result<Automation> {
        sqlx::query(
            "INSERT INTO agent_automations
             (id, org_id, scope, team_id, owner_id, name, description, prompt, enabled, trigger,
              outputs, run_as_user_id, language, timeout_secs, template, next_run_at,
              incident_cursor, created_by, updated_by, created_at,
              updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17,
                     $18, $19, $20, $21)",
        )
        .bind(&automation.id.0)
        .bind(&automation.org_id.0)
        .bind(automation.scope.kind())
        .bind(automation.scope.team_id().map(|id| id.0.as_str()))
        .bind(automation.scope.owner_id().map(|id| id.0.as_str()))
        .bind(&automation.name)
        .bind(&automation.description)
        .bind(&automation.prompt)
        .bind(automation.enabled)
        .bind(Json(&automation.trigger))
        .bind(Json(&automation.outputs))
        .bind(&automation.run_as_user_id.0)
        .bind(&automation.language)
        .bind(i32::try_from(automation.timeout_secs).unwrap_or(i32::MAX))
        .bind(automation.template.as_deref())
        .bind(automation.next_run_at)
        .bind(automation.incident_cursor)
        .bind(&automation.created_by.0)
        .bind(&automation.updated_by.0)
        .bind(automation.created_at)
        .bind(automation.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|error| name_conflict(error, &automation.name))?;

        self.get(&automation.org_id, &automation.id).await
    }

    async fn update(&self, automation: Automation) -> Result<Automation> {
        let result = sqlx::query(
            "UPDATE agent_automations SET
               name = $3, description = $4, prompt = $5, enabled = $6, trigger = $7,
               outputs = $8, run_as_user_id = $9, language = $10, timeout_secs = $11,
               next_run_at = $12, incident_cursor = $13, updated_by = $14,
               updated_at = $15
             WHERE org_id = $1 AND id = $2",
        )
        .bind(&automation.org_id.0)
        .bind(&automation.id.0)
        .bind(&automation.name)
        .bind(&automation.description)
        .bind(&automation.prompt)
        .bind(automation.enabled)
        .bind(Json(&automation.trigger))
        .bind(Json(&automation.outputs))
        .bind(&automation.run_as_user_id.0)
        .bind(&automation.language)
        .bind(i32::try_from(automation.timeout_secs).unwrap_or(i32::MAX))
        .bind(automation.next_run_at)
        .bind(automation.incident_cursor)
        .bind(&automation.updated_by.0)
        .bind(automation.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|error| name_conflict(error, &automation.name))?;

        if result.rows_affected() == 0 {
            return Err(not_found(&automation.id));
        }

        self.get(&automation.org_id, &automation.id).await
    }

    async fn delete(&self, org_id: &Id, id: &Id) -> Result<bool> {
        let result = sqlx::query("DELETE FROM agent_automations WHERE org_id = $1 AND id = $2")
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(result.rows_affected() > 0)
    }

    async fn set_enabled(&self, org_id: &Id, id: &Id, enabled: bool) -> Result<Automation> {
        let result = sqlx::query(
            "UPDATE agent_automations SET enabled = $3, updated_at = $4
             WHERE org_id = $1 AND id = $2",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .bind(enabled)
        .bind(TimestampMicros::now())
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        if result.rows_affected() == 0 {
            return Err(not_found(id));
        }

        self.get(org_id, id).await
    }

    async fn set_api_token(
        &self,
        org_id: &Id,
        id: &Id,
        token: Option<(String, String)>,
    ) -> Result<()> {
        let (hash, prefix) = token.unzip();

        let result = sqlx::query(
            "UPDATE agent_automations SET api_token_hash = $3, api_token_prefix = $4
             WHERE org_id = $1 AND id = $2",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .bind(hash)
        .bind(prefix)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        if result.rows_affected() == 0 {
            return Err(not_found(id));
        }

        Ok(())
    }

    async fn claim_due(
        &self,
        now: TimestampMicros,
        hold_until: TimestampMicros,
        limit: i64,
    ) -> Result<Vec<Automation>> {
        let ids: Vec<String> = sqlx::query_scalar(
            "UPDATE agent_automations a SET next_run_at = $2
             FROM (
               SELECT id FROM agent_automations
               WHERE enabled AND next_run_at IS NOT NULL AND next_run_at <= $1
               ORDER BY next_run_at LIMIT $3
               FOR UPDATE SKIP LOCKED
             ) due
             WHERE a.id = due.id
             RETURNING a.id",
        )
        .bind(now)
        .bind(hold_until)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        if ids.is_empty() {
            return Ok(Vec::new());
        }

        sqlx::query(&format!("{SELECT} WHERE a.id = ANY($1)"))
            .bind(&ids)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?
            .into_iter()
            .map(automation_row)
            .collect()
    }

    async fn set_next_run(&self, id: &Id, next: Option<TimestampMicros>) -> Result<()> {
        sqlx::query("UPDATE agent_automations SET next_run_at = $2 WHERE id = $1")
            .bind(&id.0)
            .bind(next)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(())
    }

    async fn incident_rules(&self) -> Result<Vec<Automation>> {
        self.fetch_many(
            "WHERE a.enabled AND a.trigger->>'kind' IN ('incident', 'finding')
             ORDER BY a.org_id, a.id",
            None,
        )
        .await
    }

    async fn advance_incident_cursor(
        &self,
        id: &Id,
        from: Option<TimestampMicros>,
        to: TimestampMicros,
    ) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE agent_automations SET incident_cursor = $3
             WHERE id = $1 AND incident_cursor IS NOT DISTINCT FROM $2",
        )
        .bind(&id.0)
        .bind(from)
        .bind(to)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(result.rows_affected() > 0)
    }

    async fn writes_rca(&self, org_id: &Id) -> Result<bool> {
        sqlx::query_scalar(
            "SELECT EXISTS (
               SELECT 1 FROM agent_automations
               WHERE org_id = $1 AND enabled AND trigger->>'kind' = 'incident'
                 AND COALESCE((outputs->>'writeRca')::BOOLEAN, FALSE)
             )",
        )
        .bind(&org_id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)
    }

    async fn create_run(&self, run: AutomationRun) -> Result<AutomationRun> {
        runs::create(&self.pool, run).await
    }

    async fn attach_run(&self, id: &Id, chat_id: &Id, run_id: &Id) -> Result<()> {
        runs::attach(&self.pool, id, chat_id, run_id).await
    }

    async fn finish_run(&self, id: &Id, ending: &RunEnding) -> Result<Option<AutomationRun>> {
        runs::finish(&self.pool, id, ending).await
    }

    async fn get_run(&self, org_id: &Id, id: &Id) -> Result<AutomationRun> {
        runs::get(&self.pool, org_id, id).await
    }

    async fn run_for_agent_run(&self, run_id: &Id) -> Result<Option<AutomationRun>> {
        runs::for_agent_run(&self.pool, run_id).await
    }

    async fn list_runs(
        &self,
        org_id: &Id,
        automation_id: &Id,
        limit: i64,
    ) -> Result<Vec<AutomationRun>> {
        runs::list(&self.pool, org_id, automation_id, limit).await
    }

    async fn running_runs(&self, limit: i64) -> Result<Vec<AutomationRun>> {
        runs::running(&self.pool, limit).await
    }

    async fn analyzed(
        &self,
        automation_id: &Id,
        incident_id: &Id,
        group_key: &str,
        since: TimestampMicros,
    ) -> Result<bool> {
        runs::analyzed(&self.pool, automation_id, incident_id, group_key, since).await
    }

    async fn purge_runs(&self, keep: i64) -> Result<u64> {
        runs::purge(&self.pool, keep).await
    }
}
