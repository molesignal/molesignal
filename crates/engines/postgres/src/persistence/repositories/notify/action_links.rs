// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! One-time action links on notifications, kept in `one_time_tokens` as kind `notify_action`:
//! the recipient in `user_id`, the alert in `subject`, the action in `purpose`.

use async_trait::async_trait;
use domain::notify::{
    action_links::{ActionGrant, NotifyActionLinkRepository},
    reply::ReplyAction,
};
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use sqlx::{PgPool, Row};

use super::super::sqlx_err;

const KIND: &str = "notify_action";

pub struct PgNotifyActionLinkRepository {
    pool: PgPool,
}

impl PgNotifyActionLinkRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn action_name(action: ReplyAction) -> &'static str {
    match action {
        ReplyAction::Acknowledge => "acknowledge",
        ReplyAction::Resolve => "resolve",
    }
}

fn parse_action(name: &str) -> Result<ReplyAction> {
    match name {
        "acknowledge" => Ok(ReplyAction::Acknowledge),
        "resolve" => Ok(ReplyAction::Resolve),
        other => Err(Error::internal(format!(
            "unknown notify action link purpose: {other}"
        ))),
    }
}

#[async_trait]
impl NotifyActionLinkRepository for PgNotifyActionLinkRepository {
    async fn record(&self, token_hash: &str, grant: &ActionGrant) -> Result<()> {
        sqlx::query(
            "INSERT INTO one_time_tokens
             (token_hash, kind, org_id, user_id, subject, purpose, created_at, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(token_hash)
        .bind(KIND)
        .bind(&grant.organization_id.0)
        .bind(&grant.recipient_user_id.0)
        .bind(&grant.alert_id.0)
        .bind(action_name(grant.action))
        .bind(grant.created_at)
        .bind(grant.expires_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn get(&self, token_hash: &str, now: TimestampMicros) -> Result<Option<ActionGrant>> {
        let row = sqlx::query(
            "SELECT org_id, user_id, subject, purpose, created_at, expires_at, used_at
             FROM one_time_tokens
             WHERE token_hash = $1 AND kind = $2 AND expires_at > $3",
        )
        .bind(token_hash)
        .bind(KIND)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        let Some(row) = row else {
            return Ok(None);
        };

        let text = |column: &str| -> Result<String> {
            row.try_get::<Option<String>, _>(column)
                .map_err(sqlx_err)?
                .ok_or_else(|| Error::internal(format!("notify action link has no {column}")))
        };

        Ok(Some(ActionGrant {
            organization_id: Id::from_string(text("org_id")?),
            recipient_user_id: Id::from_string(text("user_id")?),
            alert_id: Id::from_string(text("subject")?),
            action: parse_action(&text("purpose")?)?,
            created_at: row.try_get("created_at").map_err(sqlx_err)?,
            expires_at: row.try_get("expires_at").map_err(sqlx_err)?,
            used_at: row.try_get("used_at").map_err(sqlx_err)?,
        }))
    }

    async fn mark_used(&self, token_hash: &str, at: TimestampMicros) -> Result<bool> {
        let done = sqlx::query(
            "UPDATE one_time_tokens SET used_at = $3
             WHERE token_hash = $1 AND kind = $2 AND used_at IS NULL AND expires_at > $3",
        )
        .bind(token_hash)
        .bind(KIND)
        .bind(at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(done.rows_affected() == 1)
    }

    async fn purge_expired(&self, before: TimestampMicros) -> Result<u64> {
        let done = sqlx::query("DELETE FROM one_time_tokens WHERE kind = $1 AND expires_at < $2")
            .bind(KIND)
            .bind(before)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(done.rows_affected())
    }
}
