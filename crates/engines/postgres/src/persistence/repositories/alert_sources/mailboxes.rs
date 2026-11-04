// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `alert_source_mailboxes` 表：邮件来源读的 IMAP 邮箱。密码用根密钥加密。

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::super::sqlx_err;
use crate::{
    domain::alerting::source::{MailCursor, Mailbox, MailboxRepository, MailboxSettings},
    infra::cipher::CipherRootKey,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct PgMailboxRepository {
    pool: PgPool,
    cipher: CipherRootKey,
}

impl PgMailboxRepository {
    pub fn new(pool: PgPool, cipher: CipherRootKey) -> Self {
        Self { pool, cipher }
    }

    fn row_to(&self, row: sqlx::postgres::PgRow) -> Result<Mailbox> {
        let settings: Json<MailboxSettings> = row.try_get("settings").map_err(sqlx_err)?;
        let nonce: Option<Vec<u8>> = row.try_get("secret_nonce").map_err(sqlx_err)?;
        let ciphertext: Option<Vec<u8>> = row.try_get("secret_ciphertext").map_err(sqlx_err)?;

        let secret = match (nonce, ciphertext) {
            (Some(nonce), Some(ciphertext)) => {
                let bytes = self
                    .cipher
                    .open(&nonce, &ciphertext)
                    .map_err(|error| Error::internal(format!("mailbox secret: {error}")))?;

                String::from_utf8(bytes)
                    .map_err(|error| Error::internal(format!("mailbox secret: {error}")))?
            }
            _ => String::new(),
        };

        let uid_validity: Option<i64> = row.try_get("uid_validity").map_err(sqlx_err)?;
        let last_uid: Option<i64> = row.try_get("last_uid").map_err(sqlx_err)?;

        let cursor = uid_validity
            .zip(last_uid)
            .map(|(validity, uid)| MailCursor {
                uid_validity: u32::try_from(validity).unwrap_or_default(),
                last_uid: u32::try_from(uid).unwrap_or_default(),
            });

        Ok(Mailbox {
            source_id: Id::from_string(row.try_get::<String, _>("source_id").map_err(sqlx_err)?),
            org_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
            settings: settings.0,
            secret,
            cursor,
            last_polled_at: row
                .try_get::<Option<TimestampMicros>, _>("last_polled_at")
                .map_err(sqlx_err)?,
            last_success_at: row
                .try_get::<Option<TimestampMicros>, _>("last_success_at")
                .map_err(sqlx_err)?,
            last_error: row.try_get("last_error").map_err(sqlx_err)?,
            updated_at: row
                .try_get::<TimestampMicros, _>("updated_at")
                .map_err(sqlx_err)?,
        })
    }
}

const COLS: &str = "source_id, org_id, settings, secret_nonce, secret_ciphertext, uid_validity,
     last_uid, last_polled_at, last_success_at, last_error, updated_at";

#[async_trait]
impl MailboxRepository for PgMailboxRepository {
    async fn get(&self, source_id: &Id) -> Result<Option<Mailbox>> {
        let row = sqlx::query(&format!(
            "SELECT {COLS} FROM alert_source_mailboxes WHERE source_id = $1"
        ))
        .bind(&source_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        row.map(|row| self.row_to(row)).transpose()
    }

    async fn list_all(&self) -> Result<Vec<Mailbox>> {
        let rows = sqlx::query(&format!("SELECT {COLS} FROM alert_source_mailboxes"))
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;

        rows.into_iter().map(|row| self.row_to(row)).collect()
    }

    async fn save(&self, mailbox: Mailbox) -> Result<Mailbox> {
        let (nonce, ciphertext) = self
            .cipher
            .seal(mailbox.secret.as_bytes())
            .map_err(|error| Error::internal(format!("mailbox secret: {error}")))?;

        sqlx::query(
            "INSERT INTO alert_source_mailboxes
                (source_id, org_id, settings, secret_nonce, secret_ciphertext, uid_validity,
                 last_uid, last_polled_at, last_success_at, last_error,
                 updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
             ON CONFLICT (source_id) DO UPDATE
             SET settings = EXCLUDED.settings,
                 secret_nonce = EXCLUDED.secret_nonce,
                 secret_ciphertext = EXCLUDED.secret_ciphertext,
                 uid_validity = EXCLUDED.uid_validity,
                 last_uid = EXCLUDED.last_uid,
                 last_polled_at = EXCLUDED.last_polled_at,
                 last_success_at = EXCLUDED.last_success_at,
                 last_error = EXCLUDED.last_error,
                 updated_at = EXCLUDED.updated_at",
        )
        .bind(&mailbox.source_id.0)
        .bind(&mailbox.org_id.0)
        .bind(Json(&mailbox.settings))
        .bind(nonce)
        .bind(ciphertext)
        .bind(mailbox.cursor.map(|cursor| i64::from(cursor.uid_validity)))
        .bind(mailbox.cursor.map(|cursor| i64::from(cursor.last_uid)))
        .bind(mailbox.last_polled_at)
        .bind(mailbox.last_success_at)
        .bind(&mailbox.last_error)
        .bind(mailbox.updated_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(mailbox)
    }

    async fn delete(&self, source_id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM alert_source_mailboxes WHERE source_id = $1")
            .bind(&source_id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(())
    }

    async fn advance(&self, source_id: &Id, cursor: MailCursor) -> Result<()> {
        sqlx::query(
            "UPDATE alert_source_mailboxes SET uid_validity = $2, last_uid = $3
             WHERE source_id = $1",
        )
        .bind(&source_id.0)
        .bind(i64::from(cursor.uid_validity))
        .bind(i64::from(cursor.last_uid))
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn record_poll(
        &self,
        source_id: &Id,
        at: TimestampMicros,
        error: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE alert_source_mailboxes
             SET last_polled_at = $2,
                 last_success_at = CASE WHEN $3::TEXT IS NULL THEN $2
                                               ELSE last_success_at END,
                 last_error = COALESCE($3::TEXT, '')
             WHERE source_id = $1",
        )
        .bind(&source_id.0)
        .bind(at)
        .bind(error)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }
}
