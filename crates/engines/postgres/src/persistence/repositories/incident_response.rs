// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 重大事件（处置、任务、组织设置）与内部复盘（复盘、改进项）。组织设置在 `org_settings` 的
//! `incident.response` 一节，会议的密钥用根密钥加密，存在那一行的 `secret_*`。

use std::collections::BTreeMap;

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::{org_settings::sections::INCIDENT_RESPONSE, sqlx_err};
use crate::{
    domain::alerting::{
        postmortem::{
            ActionItem, ActionItemStatus, Postmortem, PostmortemRepository, PostmortemStatus,
            PostmortemSummary,
        },
        response::{
            IncidentResponse, IncidentResponseRepository, IncidentTask, ResponseRole,
            ResponseSettings, WarRoom,
        },
    },
    infra::cipher::CipherRootKey,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct PgIncidentResponseRepository {
    pool: PgPool,
    cipher: CipherRootKey,
}

impl PgIncidentResponseRepository {
    pub fn new(pool: PgPool, cipher: CipherRootKey) -> Self {
        Self { pool, cipher }
    }
}

fn opt_id(row: &sqlx::postgres::PgRow, column: &str) -> Result<Option<Id>> {
    Ok(row
        .try_get::<Option<String>, _>(column)
        .map_err(sqlx_err)?
        .map(Id))
}

fn opt_at(row: &sqlx::postgres::PgRow, column: &str) -> Result<Option<TimestampMicros>> {
    row.try_get(column).map_err(sqlx_err)
}

fn at(row: &sqlx::postgres::PgRow, column: &str) -> Result<TimestampMicros> {
    row.try_get(column).map_err(sqlx_err)
}

fn row_to_task(row: &sqlx::postgres::PgRow) -> Result<IncidentTask> {
    Ok(IncidentTask {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        incident_id: Id(row.try_get("incident_id").map_err(sqlx_err)?),
        title: row.try_get("title").map_err(sqlx_err)?,
        assignee_id: opt_id(row, "assignee_id")?,
        done: row.try_get("done").map_err(sqlx_err)?,
        done_by: opt_id(row, "done_by")?,
        done_at: opt_at(row, "done_at")?,
        position: row.try_get("position").map_err(sqlx_err)?,
        created_by: opt_id(row, "created_by")?,
        created_at: at(row, "created_at")?,
    })
}

#[async_trait]
impl IncidentResponseRepository for PgIncidentResponseRepository {
    async fn get(&self, org_id: &Id, incident_id: &Id) -> Result<Option<IncidentResponse>> {
        let Some(row) = sqlx::query(
            "SELECT incident_id, org_id, declared_at, declared_by, roles, war_room,
                    meeting_url, updated_at
               FROM incident_responses WHERE org_id = $1 AND incident_id = $2",
        )
        .bind(&org_id.0)
        .bind(&incident_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        else {
            return Ok(None);
        };

        let Json(roles): Json<BTreeMap<ResponseRole, Id>> =
            row.try_get("roles").map_err(sqlx_err)?;
        let war_room: Option<Json<WarRoom>> = row.try_get("war_room").map_err(sqlx_err)?;

        Ok(Some(IncidentResponse {
            incident_id: Id(row.try_get("incident_id").map_err(sqlx_err)?),
            org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
            declared_at: at(&row, "declared_at")?,
            declared_by: opt_id(&row, "declared_by")?,
            roles,
            war_room: war_room.map(|Json(room)| room),
            meeting_url: row.try_get("meeting_url").map_err(sqlx_err)?,
            updated_at: at(&row, "updated_at")?,
        }))
    }

    async fn save(&self, response: IncidentResponse) -> Result<IncidentResponse> {
        sqlx::query(
            "INSERT INTO incident_responses
                (incident_id, org_id, declared_at, declared_by, roles, war_room,
                 meeting_url, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (incident_id) DO UPDATE
             SET roles = EXCLUDED.roles, war_room = EXCLUDED.war_room,
                 meeting_url = EXCLUDED.meeting_url, updated_at = EXCLUDED.updated_at
             WHERE incident_responses.org_id = EXCLUDED.org_id",
        )
        .bind(&response.incident_id.0)
        .bind(&response.org_id.0)
        .bind(response.declared_at)
        .bind(response.declared_by.as_ref().map(|id| &id.0))
        .bind(Json(&response.roles))
        .bind(response.war_room.as_ref().map(Json))
        .bind(&response.meeting_url)
        .bind(response.updated_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(response)
    }

    async fn list_tasks(&self, org_id: &Id, incident_id: &Id) -> Result<Vec<IncidentTask>> {
        let rows = sqlx::query(
            "SELECT id, org_id, incident_id, title, assignee_id, done, done_by, done_at,
                    position, created_by, created_at
               FROM incident_tasks WHERE org_id = $1 AND incident_id = $2
              ORDER BY position, created_at",
        )
        .bind(&org_id.0)
        .bind(&incident_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.iter().map(row_to_task).collect()
    }

    async fn save_task(&self, task: IncidentTask) -> Result<IncidentTask> {
        sqlx::query(
            "INSERT INTO incident_tasks
                (id, org_id, incident_id, title, assignee_id, done, done_by, done_at,
                 position, created_by, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
             ON CONFLICT (id) DO UPDATE
             SET title = EXCLUDED.title, assignee_id = EXCLUDED.assignee_id, done = EXCLUDED.done,
                 done_by = EXCLUDED.done_by, done_at = EXCLUDED.done_at,
                 position = EXCLUDED.position
             WHERE incident_tasks.org_id = EXCLUDED.org_id",
        )
        .bind(&task.id.0)
        .bind(&task.org_id.0)
        .bind(&task.incident_id.0)
        .bind(&task.title)
        .bind(task.assignee_id.as_ref().map(|id| &id.0))
        .bind(task.done)
        .bind(task.done_by.as_ref().map(|id| &id.0))
        .bind(task.done_at)
        .bind(task.position)
        .bind(task.created_by.as_ref().map(|id| &id.0))
        .bind(task.created_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(task)
    }

    async fn delete_task(&self, org_id: &Id, incident_id: &Id, id: &Id) -> Result<bool> {
        let deleted = sqlx::query(
            "DELETE FROM incident_tasks WHERE org_id = $1 AND incident_id = $2 AND id = $3",
        )
        .bind(&org_id.0)
        .bind(&incident_id.0)
        .bind(&id.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?
        .rows_affected();

        Ok(deleted > 0)
    }

    async fn settings(&self, org_id: &Id) -> Result<ResponseSettings> {
        let row = sqlx::query(
            "SELECT value, secret_nonce, secret_ciphertext
             FROM org_settings WHERE org_id = $1 AND section = $2",
        )
        .bind(&org_id.0)
        .bind(INCIDENT_RESPONSE)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        let Some(row) = row else {
            return Ok(ResponseSettings::default());
        };

        let Json(mut settings): Json<ResponseSettings> = row.try_get("value").map_err(sqlx_err)?;
        let nonce: Option<Vec<u8>> = row.try_get("secret_nonce").map_err(sqlx_err)?;
        let ciphertext: Option<Vec<u8>> = row.try_get("secret_ciphertext").map_err(sqlx_err)?;

        if let (Some(nonce), Some(ciphertext)) = (nonce, ciphertext) {
            let plaintext = self
                .cipher
                .open(&nonce, &ciphertext)
                .map_err(|_| Error::internal("incident response secrets decrypt failed"))?;

            settings.secrets = serde_json::from_slice(&plaintext)
                .map_err(|_| Error::internal("incident response secrets are not valid JSON"))?;
        }

        settings.secrets_set = settings.secrets.keys().cloned().collect();
        Ok(settings)
    }

    async fn save_settings(
        &self,
        org_id: &Id,
        settings: ResponseSettings,
        by: &Id,
    ) -> Result<ResponseSettings> {
        let mut settings = settings;

        let sealed = if settings.secrets.is_empty() {
            None
        } else {
            let bytes = serde_json::to_vec(&settings.secrets)
                .map_err(|error| Error::internal(format!("incident response secrets: {error}")))?;

            Some(
                self.cipher
                    .seal(&bytes)
                    .map_err(|error| Error::internal(format!("incident response seal: {error}")))?,
            )
        };

        settings.secrets_set = settings.secrets.keys().cloned().collect();

        sqlx::query(
            "INSERT INTO org_settings
                (org_id, section, value, secret_nonce, secret_ciphertext, updated_by, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (org_id, section) DO UPDATE
             SET value = EXCLUDED.value, secret_nonce = EXCLUDED.secret_nonce,
                 secret_ciphertext = EXCLUDED.secret_ciphertext, updated_by = EXCLUDED.updated_by,
                 updated_at = EXCLUDED.updated_at",
        )
        .bind(&org_id.0)
        .bind(INCIDENT_RESPONSE)
        .bind(Json(&settings))
        .bind(sealed.as_ref().map(|(nonce, _)| nonce.clone()))
        .bind(sealed.as_ref().map(|(_, ciphertext)| ciphertext.clone()))
        .bind(&by.0)
        .bind(TimestampMicros::now())
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(settings)
    }
}

const POSTMORTEM_COLS: &str =
    "p.id, p.org_id, p.incident_id, p.title, p.status, p.body, p.owner_id,
     p.reviewer_ids, p.created_by, p.created_at, p.updated_at, p.published_at";

const ACTION_COLS: &str = "id, org_id, postmortem_id, incident_id, title, owner_id, due_at,
     status, ticket_id, ticket_key, ticket_url, created_at, updated_at";

fn row_to_postmortem(row: &sqlx::postgres::PgRow) -> Result<Postmortem> {
    let status: String = row.try_get("status").map_err(sqlx_err)?;
    let Json(reviewer_ids): Json<Vec<Id>> = row.try_get("reviewer_ids").map_err(sqlx_err)?;

    Ok(Postmortem {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        incident_id: Id(row.try_get("incident_id").map_err(sqlx_err)?),
        title: row.try_get("title").map_err(sqlx_err)?,
        status: PostmortemStatus::parse(&status),
        body: row.try_get("body").map_err(sqlx_err)?,
        owner_id: opt_id(row, "owner_id")?,
        reviewer_ids,
        created_by: opt_id(row, "created_by")?,
        created_at: at(row, "created_at")?,
        updated_at: at(row, "updated_at")?,
        published_at: opt_at(row, "published_at")?,
    })
}

fn row_to_action(row: &sqlx::postgres::PgRow) -> Result<ActionItem> {
    let status: String = row.try_get("status").map_err(sqlx_err)?;

    Ok(ActionItem {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        postmortem_id: Id(row.try_get("postmortem_id").map_err(sqlx_err)?),
        incident_id: Id(row.try_get("incident_id").map_err(sqlx_err)?),
        title: row.try_get("title").map_err(sqlx_err)?,
        owner_id: opt_id(row, "owner_id")?,
        due_at: opt_at(row, "due_at")?,
        status: if status == "done" {
            ActionItemStatus::Done
        } else {
            ActionItemStatus::Open
        },
        ticket_id: opt_id(row, "ticket_id")?,
        ticket_key: row.try_get("ticket_key").map_err(sqlx_err)?,
        ticket_url: row.try_get("ticket_url").map_err(sqlx_err)?,
        created_at: at(row, "created_at")?,
        updated_at: at(row, "updated_at")?,
    })
}

#[async_trait]
impl PostmortemRepository for PgIncidentResponseRepository {
    async fn list(&self, org_id: &Id, now: TimestampMicros) -> Result<Vec<PostmortemSummary>> {
        let rows = sqlx::query(&format!(
            "SELECT {POSTMORTEM_COLS},
                    COALESCE(i.summary, '') AS incident_summary,
                    COUNT(a.id) FILTER (WHERE a.status = 'open') AS open_actions,
                    COUNT(a.id) FILTER (WHERE a.status = 'open' AND a.due_at < $2)
                        AS overdue_actions
               FROM incident_postmortems p
               LEFT JOIN incidents i ON i.id = p.incident_id
               LEFT JOIN postmortem_action_items a ON a.postmortem_id = p.id
              WHERE p.org_id = $1
              GROUP BY p.id, i.summary
              ORDER BY p.created_at DESC
              LIMIT 200"
        ))
        .bind(&org_id.0)
        .bind(now)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.iter()
            .map(|row| {
                Ok(PostmortemSummary {
                    postmortem: row_to_postmortem(row)?,
                    incident_summary: row.try_get("incident_summary").map_err(sqlx_err)?,
                    open_actions: row.try_get("open_actions").map_err(sqlx_err)?,
                    overdue_actions: row.try_get("overdue_actions").map_err(sqlx_err)?,
                })
            })
            .collect()
    }

    async fn get(&self, org_id: &Id, id: &Id) -> Result<Option<Postmortem>> {
        sqlx::query(&format!(
            "SELECT {POSTMORTEM_COLS} FROM incident_postmortems p WHERE p.org_id = $1 AND p.id = $2"
        ))
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .as_ref()
        .map(row_to_postmortem)
        .transpose()
    }

    async fn for_incident(&self, org_id: &Id, incident_id: &Id) -> Result<Option<Postmortem>> {
        sqlx::query(&format!(
            "SELECT {POSTMORTEM_COLS} FROM incident_postmortems p
              WHERE p.org_id = $1 AND p.incident_id = $2"
        ))
        .bind(&org_id.0)
        .bind(&incident_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .as_ref()
        .map(row_to_postmortem)
        .transpose()
    }

    async fn save(&self, postmortem: Postmortem) -> Result<Postmortem> {
        sqlx::query(
            "INSERT INTO incident_postmortems
                (id, org_id, incident_id, title, status, body, owner_id, reviewer_ids, created_by,
                 created_at, updated_at, published_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
             ON CONFLICT (id) DO UPDATE
             SET title = EXCLUDED.title, status = EXCLUDED.status, body = EXCLUDED.body,
                 owner_id = EXCLUDED.owner_id, reviewer_ids = EXCLUDED.reviewer_ids,
                 updated_at = EXCLUDED.updated_at,
                 published_at = EXCLUDED.published_at
             WHERE incident_postmortems.org_id = EXCLUDED.org_id",
        )
        .bind(&postmortem.id.0)
        .bind(&postmortem.org_id.0)
        .bind(&postmortem.incident_id.0)
        .bind(&postmortem.title)
        .bind(postmortem.status.as_str())
        .bind(&postmortem.body)
        .bind(postmortem.owner_id.as_ref().map(|id| &id.0))
        .bind(Json(&postmortem.reviewer_ids))
        .bind(postmortem.created_by.as_ref().map(|id| &id.0))
        .bind(postmortem.created_at)
        .bind(postmortem.updated_at)
        .bind(postmortem.published_at)
        .execute(&self.pool)
        .await
        .map_err(|error| match sqlx_err(error) {
            Error::Conflict(_) => Error::conflict("this incident already has a postmortem"),
            other => other,
        })?;

        Ok(postmortem)
    }

    async fn list_actions(&self, org_id: &Id, postmortem_id: &Id) -> Result<Vec<ActionItem>> {
        let rows = sqlx::query(&format!(
            "SELECT {ACTION_COLS} FROM postmortem_action_items
              WHERE org_id = $1 AND postmortem_id = $2
              ORDER BY created_at"
        ))
        .bind(&org_id.0)
        .bind(&postmortem_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.iter().map(row_to_action).collect()
    }

    async fn get_action(&self, org_id: &Id, id: &Id) -> Result<Option<ActionItem>> {
        sqlx::query(&format!(
            "SELECT {ACTION_COLS} FROM postmortem_action_items WHERE org_id = $1 AND id = $2"
        ))
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .as_ref()
        .map(row_to_action)
        .transpose()
    }

    async fn save_action(&self, item: ActionItem) -> Result<ActionItem> {
        sqlx::query(
            "INSERT INTO postmortem_action_items
                (id, org_id, postmortem_id, incident_id, title, owner_id, due_at, status,
                 ticket_id, ticket_key, ticket_url, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
             ON CONFLICT (id) DO UPDATE
             SET title = EXCLUDED.title, owner_id = EXCLUDED.owner_id,
                 due_at = EXCLUDED.due_at, status = EXCLUDED.status,
                 ticket_id = EXCLUDED.ticket_id, ticket_key = EXCLUDED.ticket_key,
                 ticket_url = EXCLUDED.ticket_url, updated_at = EXCLUDED.updated_at
             WHERE postmortem_action_items.org_id = EXCLUDED.org_id",
        )
        .bind(&item.id.0)
        .bind(&item.org_id.0)
        .bind(&item.postmortem_id.0)
        .bind(&item.incident_id.0)
        .bind(&item.title)
        .bind(item.owner_id.as_ref().map(|id| &id.0))
        .bind(item.due_at)
        .bind(match item.status {
            ActionItemStatus::Open => "open",
            ActionItemStatus::Done => "done",
        })
        .bind(item.ticket_id.as_ref().map(|id| &id.0))
        .bind(&item.ticket_key)
        .bind(&item.ticket_url)
        .bind(item.created_at)
        .bind(item.updated_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(item)
    }

    async fn delete_action(&self, org_id: &Id, postmortem_id: &Id, id: &Id) -> Result<bool> {
        let deleted = sqlx::query(
            "DELETE FROM postmortem_action_items WHERE org_id = $1 AND postmortem_id = $2 AND id = $3",
        )
        .bind(&org_id.0)
        .bind(&postmortem_id.0)
        .bind(&id.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?
        .rows_affected();

        Ok(deleted > 0)
    }
}
