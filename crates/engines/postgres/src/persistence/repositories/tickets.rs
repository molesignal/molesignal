// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 工单连接器与事件关联的工单。连接器的秘密（Jira 的 API Token / PAT 与 Webhook 密钥、飞书项目的
//! 插件密钥）用根密钥加密后单独存；列表里只说哪些已经设了。

use std::collections::BTreeMap;

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::{
    domain::alerting::ticket::{
        IncidentTicket, TicketAutoRule, TicketComments, TicketConnector, TicketRepository,
        TicketSystem,
    },
    infra::cipher::CipherRootKey,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct PgTicketRepository {
    pool: PgPool,
    cipher: CipherRootKey,
}

impl PgTicketRepository {
    pub fn new(pool: PgPool, cipher: CipherRootKey) -> Self {
        Self { pool, cipher }
    }
}

const CONNECTOR_COLS: &str = "id, org_id, name, system, settings, secret_nonce, secret_ciphertext,
     comments, transition_on_resolve, resolve_incident_when_done, auto_rule, on_major_incident,
     enabled, created_at, updated_at";

const TICKET_COLS: &str =
    "id, org_id, incident_id, connector_id, system, external_id, external_key,
     url, status, done, incident_status, created_by, created_at, synced_at,
     last_error, action_item_id";

impl PgTicketRepository {
    fn row_to_connector(
        &self,
        row: &sqlx::postgres::PgRow,
        reveal: bool,
    ) -> Result<TicketConnector> {
        let nonce: Option<Vec<u8>> = row.try_get("secret_nonce").map_err(sqlx_err)?;
        let ciphertext: Option<Vec<u8>> = row.try_get("secret_ciphertext").map_err(sqlx_err)?;

        let secrets: BTreeMap<String, String> = match (nonce, ciphertext) {
            (Some(nonce), Some(ciphertext)) => {
                let plaintext = self
                    .cipher
                    .open(&nonce, &ciphertext)
                    .map_err(|_| Error::internal("ticket connector secrets decrypt failed"))?;

                serde_json::from_slice(&plaintext)
                    .map_err(|_| Error::internal("ticket connector secrets are not valid JSON"))?
            }
            _ => BTreeMap::new(),
        };

        let system: String = row.try_get("system").map_err(sqlx_err)?;
        let Json(settings): Json<serde_json::Value> = row.try_get("settings").map_err(sqlx_err)?;
        let Json(comments): Json<TicketComments> = row.try_get("comments").map_err(sqlx_err)?;
        let Json(auto): Json<TicketAutoRule> = row.try_get("auto_rule").map_err(sqlx_err)?;

        Ok(TicketConnector {
            id: Id(row.try_get("id").map_err(sqlx_err)?),
            org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
            name: row.try_get("name").map_err(sqlx_err)?,
            system: TicketSystem::parse(&system)?,
            settings,
            secrets_set: secrets
                .iter()
                .filter(|(_, value)| !value.is_empty())
                .map(|(key, _)| key.clone())
                .collect(),
            secrets: if reveal { secrets } else { BTreeMap::new() },
            comments,
            transition_on_resolve: row.try_get("transition_on_resolve").map_err(sqlx_err)?,
            resolve_incident_when_done: row
                .try_get("resolve_incident_when_done")
                .map_err(sqlx_err)?,
            auto,
            on_major_incident: row.try_get("on_major_incident").map_err(sqlx_err)?,
            enabled: row.try_get("enabled").map_err(sqlx_err)?,
            created_at: row
                .try_get::<TimestampMicros, _>("created_at")
                .map_err(sqlx_err)?,
            updated_at: row
                .try_get::<TimestampMicros, _>("updated_at")
                .map_err(sqlx_err)?,
        })
    }
}

fn row_to_ticket(row: &sqlx::postgres::PgRow) -> Result<IncidentTicket> {
    let system: String = row.try_get("system").map_err(sqlx_err)?;

    Ok(IncidentTicket {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        incident_id: Id(row.try_get("incident_id").map_err(sqlx_err)?),
        connector_id: Id(row.try_get("connector_id").map_err(sqlx_err)?),
        system: TicketSystem::parse(&system)?,
        external_id: row.try_get("external_id").map_err(sqlx_err)?,
        external_key: row.try_get("external_key").map_err(sqlx_err)?,
        url: row.try_get("url").map_err(sqlx_err)?,
        status: row.try_get("status").map_err(sqlx_err)?,
        done: row.try_get("done").map_err(sqlx_err)?,
        incident_status: row.try_get("incident_status").map_err(sqlx_err)?,
        created_by: row
            .try_get::<Option<String>, _>("created_by")
            .map_err(sqlx_err)?
            .map(Id),
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
        synced_at: row
            .try_get::<TimestampMicros, _>("synced_at")
            .map_err(sqlx_err)?,
        last_error: row.try_get("last_error").map_err(sqlx_err)?,
        action_item_id: row
            .try_get::<Option<String>, _>("action_item_id")
            .map_err(sqlx_err)?
            .map(Id),
    })
}

#[async_trait]
impl TicketRepository for PgTicketRepository {
    async fn list_connectors(&self, org_id: &Id) -> Result<Vec<TicketConnector>> {
        let rows = sqlx::query(&format!(
            "SELECT {CONNECTOR_COLS} FROM ticket_connectors WHERE org_id = $1 ORDER BY name"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.iter()
            .map(|row| self.row_to_connector(row, false))
            .collect()
    }

    async fn get_connector(&self, org_id: &Id, id: &Id) -> Result<Option<TicketConnector>> {
        sqlx::query(&format!(
            "SELECT {CONNECTOR_COLS} FROM ticket_connectors WHERE org_id = $1 AND id = $2"
        ))
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .map(|row| self.row_to_connector(&row, true))
        .transpose()
    }

    async fn find_connector(&self, id: &Id) -> Result<Option<TicketConnector>> {
        sqlx::query(&format!(
            "SELECT {CONNECTOR_COLS} FROM ticket_connectors WHERE id = $1"
        ))
        .bind(&id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .map(|row| self.row_to_connector(&row, true))
        .transpose()
    }

    async fn save_connector(&self, connector: TicketConnector) -> Result<TicketConnector> {
        let secrets = connector
            .secrets
            .iter()
            .filter(|(_, value)| !value.trim().is_empty())
            .collect::<BTreeMap<_, _>>();

        let sealed = if secrets.is_empty() {
            None
        } else {
            let bytes = serde_json::to_vec(&secrets)
                .map_err(|error| Error::internal(format!("ticket connector secrets: {error}")))?;

            Some(
                self.cipher
                    .seal(&bytes)
                    .map_err(|error| Error::internal(format!("ticket connector seal: {error}")))?,
            )
        };

        sqlx::query(
            "INSERT INTO ticket_connectors
                (id, org_id, name, system, settings, secret_nonce, secret_ciphertext, comments,
                 transition_on_resolve, resolve_incident_when_done, auto_rule, on_major_incident,
                 enabled, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
             ON CONFLICT (id) DO UPDATE
             SET name = EXCLUDED.name, settings = EXCLUDED.settings,
                 secret_nonce = EXCLUDED.secret_nonce,
                 secret_ciphertext = EXCLUDED.secret_ciphertext,
                 comments = EXCLUDED.comments,
                 transition_on_resolve = EXCLUDED.transition_on_resolve,
                 resolve_incident_when_done = EXCLUDED.resolve_incident_when_done,
                 auto_rule = EXCLUDED.auto_rule, on_major_incident = EXCLUDED.on_major_incident,
                 enabled = EXCLUDED.enabled, updated_at = EXCLUDED.updated_at
             WHERE ticket_connectors.org_id = EXCLUDED.org_id",
        )
        .bind(&connector.id.0)
        .bind(&connector.org_id.0)
        .bind(&connector.name)
        .bind(connector.system.as_str())
        .bind(Json(&connector.settings))
        .bind(sealed.as_ref().map(|(nonce, _)| nonce.clone()))
        .bind(sealed.as_ref().map(|(_, ciphertext)| ciphertext.clone()))
        .bind(Json(&connector.comments))
        .bind(connector.transition_on_resolve)
        .bind(connector.resolve_incident_when_done)
        .bind(Json(&connector.auto))
        .bind(connector.on_major_incident)
        .bind(connector.enabled)
        .bind(connector.created_at)
        .bind(connector.updated_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        self.get_connector(&connector.org_id, &connector.id)
            .await?
            .ok_or_else(|| Error::not_found("ticket connector not found"))
    }

    async fn delete_connector(&self, org_id: &Id, id: &Id) -> Result<bool> {
        let deleted = sqlx::query("DELETE FROM ticket_connectors WHERE org_id = $1 AND id = $2")
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?
            .rows_affected();

        Ok(deleted > 0)
    }

    async fn create_ticket(&self, ticket: IncidentTicket) -> Result<IncidentTicket> {
        sqlx::query(
            "INSERT INTO incident_tickets
                (id, org_id, incident_id, connector_id, system, external_id, external_key, url,
                 status, done, incident_status, created_by, created_at, synced_at,
                 last_error, action_item_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)",
        )
        .bind(&ticket.id.0)
        .bind(&ticket.org_id.0)
        .bind(&ticket.incident_id.0)
        .bind(&ticket.connector_id.0)
        .bind(ticket.system.as_str())
        .bind(&ticket.external_id)
        .bind(&ticket.external_key)
        .bind(&ticket.url)
        .bind(&ticket.status)
        .bind(ticket.done)
        .bind(&ticket.incident_status)
        .bind(ticket.created_by.as_ref().map(|id| &id.0))
        .bind(ticket.created_at)
        .bind(ticket.synced_at)
        .bind(&ticket.last_error)
        .bind(ticket.action_item_id.as_ref().map(|id| &id.0))
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(ticket)
    }

    async fn update_ticket(&self, ticket: IncidentTicket) -> Result<IncidentTicket> {
        sqlx::query(
            "UPDATE incident_tickets
                SET status = $3, done = $4, incident_status = $5, synced_at = $6,
                    last_error = $7, url = $8
              WHERE id = $1 AND org_id = $2",
        )
        .bind(&ticket.id.0)
        .bind(&ticket.org_id.0)
        .bind(&ticket.status)
        .bind(ticket.done)
        .bind(&ticket.incident_status)
        .bind(ticket.synced_at)
        .bind(&ticket.last_error)
        .bind(&ticket.url)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(ticket)
    }

    async fn list_for_incident(
        &self,
        org_id: &Id,
        incident_id: &Id,
    ) -> Result<Vec<IncidentTicket>> {
        let rows = sqlx::query(&format!(
            "SELECT {TICKET_COLS} FROM incident_tickets
              WHERE org_id = $1 AND incident_id = $2
              ORDER BY created_at"
        ))
        .bind(&org_id.0)
        .bind(&incident_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.iter().map(row_to_ticket).collect()
    }

    async fn due_for_sync(
        &self,
        before: TimestampMicros,
        limit: i64,
    ) -> Result<Vec<IncidentTicket>> {
        let rows = sqlx::query(&format!(
            "SELECT {TICKET_COLS} FROM incident_tickets
              WHERE NOT done AND synced_at <= $1
              ORDER BY synced_at
              LIMIT $2"
        ))
        .bind(before)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.iter().map(row_to_ticket).collect()
    }

    async fn find_by_external(
        &self,
        connector_id: &Id,
        external_key: &str,
    ) -> Result<Option<IncidentTicket>> {
        sqlx::query(&format!(
            "SELECT {TICKET_COLS} FROM incident_tickets
              WHERE connector_id = $1 AND external_key = $2"
        ))
        .bind(&connector_id.0)
        .bind(external_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .as_ref()
        .map(row_to_ticket)
        .transpose()
    }
}
