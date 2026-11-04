// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `alert_sources` 表 Pg 实装（告警来源），以及接入留下的记录：`alert_intake_requests`（收到的
//! 请求）与 `alert_source_incidents`（外部事件在来源这一侧的状态）。

use std::collections::BTreeMap;

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::{
    domain::alerting::{
        incident::Severity,
        source::{AlertSource, AlertSourceKind, AlertSourceRepository, EmailRules},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

mod intake_requests;
mod mailboxes;
mod source_incidents;

pub use intake_requests::PgIntakeRequestRepository;
pub use mailboxes::PgMailboxRepository;
pub use source_incidents::PgSourceIncidentRepository;

pub struct PgAlertSourceRepository {
    pool: PgPool,
}

impl PgAlertSourceRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, name, kind, enabled, escalation_policy_id, severity, labels,
     token_prefix, token_hash, home_cluster, created_by, created_at, updated_at,
     last_intake_at, email";

fn row_to(row: sqlx::postgres::PgRow) -> Result<AlertSource> {
    let kind: String = row.try_get("kind").map_err(sqlx_err)?;
    let severity: String = row.try_get("severity").map_err(sqlx_err)?;
    let labels: Json<BTreeMap<String, String>> = row.try_get("labels").map_err(sqlx_err)?;
    let email: Json<EmailRules> = row.try_get("email").map_err(sqlx_err)?;

    Ok(AlertSource {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        org_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        name: row.try_get("name").map_err(sqlx_err)?,
        kind: AlertSourceKind::parse(&kind)
            .ok_or_else(|| Error::internal(format!("unknown alert source kind `{kind}`")))?,
        enabled: row.try_get("enabled").map_err(sqlx_err)?,
        escalation_policy_id: row
            .try_get::<Option<String>, _>("escalation_policy_id")
            .map_err(sqlx_err)?
            .map(Id::from_string),
        severity: Severity::from_label(&severity)
            .ok_or_else(|| Error::internal(format!("unknown severity `{severity}`")))?,
        labels: labels.0,
        token_prefix: row.try_get("token_prefix").map_err(sqlx_err)?,
        token_hash: row.try_get("token_hash").map_err(sqlx_err)?,
        home_cluster: row.try_get("home_cluster").map_err(sqlx_err)?,
        created_by: row
            .try_get::<Option<String>, _>("created_by")
            .map_err(sqlx_err)?
            .map(Id::from_string),
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
        updated_at: row
            .try_get::<TimestampMicros, _>("updated_at")
            .map_err(sqlx_err)?,
        last_intake_at: row
            .try_get::<Option<TimestampMicros>, _>("last_intake_at")
            .map_err(sqlx_err)?,
        email: email.0,
    })
}

#[async_trait]
impl AlertSourceRepository for PgAlertSourceRepository {
    async fn create(&self, source: AlertSource) -> Result<AlertSource> {
        sqlx::query(
            "INSERT INTO alert_sources
                (id, org_id, name, kind, enabled, escalation_policy_id, severity, labels,
                 token_prefix, token_hash, home_cluster, created_by, created_at,
                 updated_at, email)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
        )
        .bind(&source.id.0)
        .bind(&source.org_id.0)
        .bind(&source.name)
        .bind(source.kind.as_str())
        .bind(source.enabled)
        .bind(source.escalation_policy_id.as_ref().map(|id| id.0.clone()))
        .bind(source.severity.as_str())
        .bind(Json(&source.labels))
        .bind(&source.token_prefix)
        .bind(&source.token_hash)
        .bind(&source.home_cluster)
        .bind(source.created_by.as_ref().map(|id| id.0.clone()))
        .bind(source.created_at)
        .bind(source.updated_at)
        .bind(Json(&source.email))
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(source)
    }

    async fn update(&self, source: AlertSource) -> Result<AlertSource> {
        sqlx::query(
            "UPDATE alert_sources SET
               name = $2, kind = $3, enabled = $4, escalation_policy_id = $5, severity = $6,
               labels = $7, token_prefix = $8, token_hash = $9, home_cluster = $10,
               updated_at = $11, email = $12
             WHERE id = $1",
        )
        .bind(&source.id.0)
        .bind(&source.name)
        .bind(source.kind.as_str())
        .bind(source.enabled)
        .bind(source.escalation_policy_id.as_ref().map(|id| id.0.clone()))
        .bind(source.severity.as_str())
        .bind(Json(&source.labels))
        .bind(&source.token_prefix)
        .bind(&source.token_hash)
        .bind(&source.home_cluster)
        .bind(source.updated_at)
        .bind(Json(&source.email))
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(source)
    }

    async fn get(&self, id: &Id) -> Result<AlertSource> {
        let row = sqlx::query(&format!("SELECT {COLS} FROM alert_sources WHERE id = $1"))
            .bind(&id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;

        row_to(row)
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<AlertSource>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM alert_sources WHERE org_id = $1 ORDER BY created_at, id"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(row_to).collect()
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM alert_sources WHERE id = $1")
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(())
    }

    async fn find_by_token_prefix(&self, prefix: &str) -> Result<Option<AlertSource>> {
        let row = sqlx::query(&format!(
            "SELECT {COLS} FROM alert_sources WHERE token_prefix = $1"
        ))
        .bind(prefix)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        row.map(row_to).transpose()
    }

    async fn touch_intake(&self, id: &Id, at: TimestampMicros) -> Result<()> {
        sqlx::query(
            "UPDATE alert_sources
                SET last_intake_at = GREATEST(COALESCE(last_intake_at, TIMESTAMPTZ 'epoch'), $2)
              WHERE id = $1",
        )
        .bind(&id.0)
        .bind(at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }
}
