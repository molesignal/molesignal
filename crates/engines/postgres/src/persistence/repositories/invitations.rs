// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

use super::sqlx_err;
use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invitation {
    pub id: Id,
    pub org_id: Id,
    pub email: String,
    pub role_id: Id,
    pub inviter_id: Id,
    pub status: String,
    pub sent_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[async_trait]
pub trait InvitationRepository: Send + Sync {
    async fn create(&self, invitation: Invitation) -> Result<Invitation>;
    async fn get(&self, org_id: &Id, id: &Id) -> Result<Invitation>;
    async fn list(&self, org_id: &Id) -> Result<Vec<Invitation>>;
    async fn update_status(
        &self,
        org_id: &Id,
        id: &Id,
        status: &str,
        sent_at: Option<TimestampMicros>,
        updated_at: TimestampMicros,
    ) -> Result<Invitation>;
}

pub struct PgInvitationRepository {
    pool: PgPool,
}

impl PgInvitationRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str =
    "id, org_id, email, role_id, inviter_id, status, sent_at_micros, updated_at_micros";

fn row_to(row: sqlx::postgres::PgRow) -> Result<Invitation> {
    Ok(Invitation {
        id: Id(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        email: row.try_get("email").map_err(sqlx_err)?,
        role_id: Id(row.try_get::<String, _>("role_id").map_err(sqlx_err)?),
        inviter_id: Id(row.try_get::<String, _>("inviter_id").map_err(sqlx_err)?),
        status: row.try_get("status").map_err(sqlx_err)?,
        sent_at: TimestampMicros(row.try_get("sent_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

#[async_trait]
impl InvitationRepository for PgInvitationRepository {
    async fn create(&self, invitation: Invitation) -> Result<Invitation> {
        sqlx::query(
            "INSERT INTO invitations
                (id, org_id, email, role_id, inviter_id, status, sent_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(&invitation.id.0)
        .bind(&invitation.org_id.0)
        .bind(&invitation.email)
        .bind(&invitation.role_id.0)
        .bind(&invitation.inviter_id.0)
        .bind(&invitation.status)
        .bind(invitation.sent_at.0)
        .bind(invitation.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(invitation)
    }

    async fn get(&self, org_id: &Id, id: &Id) -> Result<Invitation> {
        let row = sqlx::query(&format!(
            "SELECT {COLS} FROM invitations WHERE org_id = $1 AND id = $2"
        ))
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;
        row_to(row)
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<Invitation>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM invitations WHERE org_id = $1 ORDER BY sent_at_micros DESC"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(row_to).collect()
    }

    async fn update_status(
        &self,
        org_id: &Id,
        id: &Id,
        status: &str,
        sent_at: Option<TimestampMicros>,
        updated_at: TimestampMicros,
    ) -> Result<Invitation> {
        let row = sqlx::query(&format!(
            "UPDATE invitations
                SET status = $3,
                    sent_at_micros = COALESCE($4, sent_at_micros),
                    updated_at_micros = $5
              WHERE org_id = $1 AND id = $2
              RETURNING {COLS}"
        ))
        .bind(&org_id.0)
        .bind(&id.0)
        .bind(status)
        .bind(sent_at.map(|t| t.0))
        .bind(updated_at.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;
        row_to(row)
    }
}
