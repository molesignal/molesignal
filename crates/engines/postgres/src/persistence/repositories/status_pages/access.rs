// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::Row;

use super::PgStatusPageRepository;
use crate::{
    domain::status_page::{
        ConsumedStatusPageMagicLink, StatusPageAccessRepository, StatusPageAccessRule,
        StatusPageAccessSession, StatusPageMagicLink, StatusPageMagicLinkExchange,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const RULE_COLS: &str = "id, org_id, status_page_id, kind, value_ciphertext,
    value_nonce, created_at_micros, updated_at_micros";
const SESSION_COLS: &str = "id, org_id, status_page_id, access_rule_id,
    email_ciphertext, email_nonce, session_token_hash, origin_host,
    expires_at_micros, revoked_at_micros, last_seen_at_micros, created_at_micros";
const SESSION_UPDATE_COLS: &str = "session.id AS id, session.org_id AS org_id,
    session.status_page_id AS status_page_id, session.access_rule_id AS access_rule_id,
    session.email_ciphertext AS email_ciphertext, session.email_nonce AS email_nonce,
    session.session_token_hash AS session_token_hash, session.origin_host AS origin_host,
    session.expires_at_micros AS expires_at_micros,
    session.revoked_at_micros AS revoked_at_micros,
    session.last_seen_at_micros AS last_seen_at_micros,
    session.created_at_micros AS created_at_micros";

impl PgStatusPageRepository {
    fn access_hash(&self, org_id: &Id, value: &str) -> String {
        self.cipher
            .org_hmac_sha256(org_id.as_str(), value.as_bytes())
    }

    fn seal_access_value(&self, value: &str) -> Result<(Vec<u8>, Vec<u8>)> {
        self.cipher
            .seal(value.as_bytes())
            .map_err(|_| Error::internal("status-page access value encryption failed"))
    }

    pub(super) fn open_access_value(&self, nonce: &[u8], ciphertext: &[u8]) -> Result<String> {
        let plaintext = self
            .cipher
            .open(nonce, ciphertext)
            .map_err(|_| Error::internal("status-page access value decryption failed"))?;
        String::from_utf8(plaintext)
            .map_err(|_| Error::internal("status-page access value is not UTF-8"))
    }
}

#[async_trait]
impl StatusPageAccessRepository for PgStatusPageRepository {
    async fn create_access_rule(&self, rule: StatusPageAccessRule) -> Result<StatusPageAccessRule> {
        let (nonce, ciphertext) = self.seal_access_value(&rule.value)?;
        let hash = self.access_hash(&rule.org_id, &rule.value);
        let sql = format!(
            "INSERT INTO status_page_access_rules
                (id, org_id, status_page_id, kind, value_ciphertext, value_nonce,
                 value_hash, created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             RETURNING {RULE_COLS}"
        );
        let row = sqlx::query(&sql)
            .bind(&rule.id.0)
            .bind(&rule.org_id.0)
            .bind(&rule.status_page_id.0)
            .bind(rule.kind.as_str())
            .bind(ciphertext)
            .bind(nonce)
            .bind(hash)
            .bind(rule.created_at.0)
            .bind(rule.updated_at.0)
            .fetch_one(&self.pool)
            .await
            .map_err(super::sqlx_err)?;
        self.row_to_access_rule(row)
    }

    async fn list_access_rules(
        &self,
        org_id: &Id,
        page_id: &Id,
    ) -> Result<Vec<StatusPageAccessRule>> {
        let sql = format!(
            "SELECT {RULE_COLS} FROM status_page_access_rules
             WHERE org_id = $1 AND status_page_id = $2
             ORDER BY kind, created_at_micros, id LIMIT 1000"
        );
        sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&page_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .into_iter()
            .map(|row| self.row_to_access_rule(row))
            .collect()
    }

    async fn find_access_rule_for_email(
        &self,
        org_id: &Id,
        page_id: &Id,
        email: &str,
        domain: &str,
    ) -> Result<Option<StatusPageAccessRule>> {
        let email_hash = self.access_hash(org_id, email);
        let domain_hash = self.access_hash(org_id, &format!("@{domain}"));
        let sql = format!(
            "SELECT {RULE_COLS} FROM status_page_access_rules
             WHERE org_id = $1 AND status_page_id = $2
               AND ((kind = 'email' AND value_hash = $3)
                    OR (kind = 'domain' AND value_hash = $4))
             ORDER BY (kind = 'email') DESC LIMIT 1"
        );
        sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&page_id.0)
            .bind(email_hash)
            .bind(domain_hash)
            .fetch_optional(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .map(|row| self.row_to_access_rule(row))
            .transpose()
    }

    async fn delete_access_rule(&self, org_id: &Id, page_id: &Id, rule_id: &Id) -> Result<()> {
        let result = sqlx::query(
            "DELETE FROM status_page_access_rules
             WHERE org_id = $1 AND status_page_id = $2 AND id = $3",
        )
        .bind(&org_id.0)
        .bind(&page_id.0)
        .bind(&rule_id.0)
        .execute(&self.pool)
        .await
        .map_err(super::sqlx_err)?;
        if result.rows_affected() == 0 {
            return Err(Error::not_found("status-page access rule not found"));
        }
        Ok(())
    }

    async fn create_magic_link(&self, link: StatusPageMagicLink) -> Result<bool> {
        let (nonce, ciphertext) = self.seal_access_value(&link.email)?;
        let email_hash = self.access_hash(&link.org_id, &link.email);
        let mut transaction = sqlx::begin(&self.pool).await.map_err(super::sqlx_err)?;
        let lock_key = format!(
            "status-page-magic-link:{}:{}:{}",
            link.org_id, link.status_page_id, email_hash
        );
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(lock_key)
            .execute(&mut *transaction)
            .await
            .map_err(super::sqlx_err)?;
        let suppressed: bool = sqlx::query_scalar(
            "SELECT
                EXISTS (
                    SELECT 1 FROM status_page_magic_links
                    WHERE org_id = $1 AND status_page_id = $2 AND email_hash = $3
                      AND created_at_micros > $4
                )
                OR (
                    SELECT count(*) FROM status_page_magic_links
                    WHERE org_id = $1 AND status_page_id = $2 AND created_at_micros > $5
                ) >= 100",
        )
        .bind(&link.org_id.0)
        .bind(&link.status_page_id.0)
        .bind(&email_hash)
        .bind(link.created_at.0.saturating_sub(60 * 1_000_000))
        .bind(link.created_at.0.saturating_sub(10 * 60 * 1_000_000))
        .fetch_one(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;
        if suppressed {
            transaction.commit().await.map_err(super::sqlx_err)?;
            return Ok(false);
        }
        sqlx::query(
            "INSERT INTO status_page_magic_links
                (id, org_id, status_page_id, access_rule_id, email_ciphertext,
                 email_nonce, email_hash, token_hash, origin_host, expires_at_micros,
                 consumed_at_micros, created_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
        )
        .bind(&link.id.0)
        .bind(&link.org_id.0)
        .bind(&link.status_page_id.0)
        .bind(&link.access_rule_id.0)
        .bind(ciphertext)
        .bind(nonce)
        .bind(email_hash)
        .bind(&link.token_hash)
        .bind(&link.origin_host)
        .bind(link.expires_at.0)
        .bind(link.consumed_at.map(|at| at.0))
        .bind(link.created_at.0)
        .execute(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;
        transaction.commit().await.map_err(super::sqlx_err)?;
        Ok(true)
    }

    async fn consume_magic_link(
        &self,
        exchange: StatusPageMagicLinkExchange,
    ) -> Result<ConsumedStatusPageMagicLink> {
        let StatusPageMagicLinkExchange {
            page_id,
            token_hash,
            origin_host,
            session_id,
            session_token_hash,
            session_expires_at,
            now,
        } = exchange;
        let mut transaction = sqlx::begin(&self.pool).await.map_err(super::sqlx_err)?;
        let row = sqlx::query(
            "SELECT link.id, link.org_id, link.status_page_id, link.access_rule_id,
                    link.email_ciphertext, link.email_nonce, link.token_hash,
                    link.origin_host, link.expires_at_micros,
                    link.consumed_at_micros, link.created_at_micros
             FROM status_page_magic_links link
             JOIN status_pages page
               ON page.org_id = link.org_id AND page.id = link.status_page_id
             JOIN status_page_access_rules rule
               ON rule.org_id = link.org_id AND rule.status_page_id = link.status_page_id
              AND rule.id = link.access_rule_id
             WHERE link.status_page_id = $1 AND link.token_hash = $2
               AND link.origin_host = $3 AND link.consumed_at_micros IS NULL
               AND link.expires_at_micros > $4
               AND page.lifecycle = 'active' AND page.visibility = 'private'
             FOR UPDATE OF link",
        )
        .bind(&page_id.0)
        .bind(&token_hash)
        .bind(&origin_host)
        .bind(now.0)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|error| access_token_error(error, "magic link is invalid or expired"))?;
        let link = self.row_to_magic_link(&row)?;
        sqlx::query(
            "UPDATE status_page_magic_links SET consumed_at_micros = $2
             WHERE id = $1 AND consumed_at_micros IS NULL",
        )
        .bind(&link.id.0)
        .bind(now.0)
        .execute(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;
        let email_ciphertext: Vec<u8> = row.try_get("email_ciphertext").map_err(super::sqlx_err)?;
        let email_nonce: Vec<u8> = row.try_get("email_nonce").map_err(super::sqlx_err)?;
        let email_hash = self.access_hash(&link.org_id, &link.email);
        let sql = format!(
            "INSERT INTO status_page_access_sessions
                (id, org_id, status_page_id, access_rule_id, email_ciphertext,
                 email_nonce, email_hash, session_token_hash, origin_host,
                 expires_at_micros, revoked_at_micros, last_seen_at_micros,
                 created_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, NULL, $11, $11)
             RETURNING {SESSION_COLS}"
        );
        let session_row = sqlx::query(&sql)
            .bind(&session_id.0)
            .bind(&link.org_id.0)
            .bind(&link.status_page_id.0)
            .bind(&link.access_rule_id.0)
            .bind(email_ciphertext)
            .bind(email_nonce)
            .bind(email_hash)
            .bind(&session_token_hash)
            .bind(&origin_host)
            .bind(session_expires_at.0)
            .bind(now.0)
            .fetch_one(&mut *transaction)
            .await
            .map_err(super::sqlx_err)?;
        let session = self.row_to_access_session(session_row)?;
        transaction.commit().await.map_err(super::sqlx_err)?;
        Ok(ConsumedStatusPageMagicLink { link, session })
    }

    async fn find_access_session(
        &self,
        page_id: &Id,
        token_hash: &str,
        origin_host: &str,
        now: TimestampMicros,
    ) -> Result<Option<StatusPageAccessSession>> {
        let sql = format!(
            "UPDATE status_page_access_sessions session SET last_seen_at_micros = $4
             FROM status_pages page, status_page_access_rules rule
             WHERE session.status_page_id = $1 AND session.session_token_hash = $2
               AND session.origin_host = $3 AND session.revoked_at_micros IS NULL
               AND session.expires_at_micros > $4
               AND page.org_id = session.org_id AND page.id = session.status_page_id
               AND page.lifecycle = 'active' AND page.visibility = 'private'
               AND rule.org_id = session.org_id AND rule.status_page_id = session.status_page_id
               AND rule.id = session.access_rule_id
             RETURNING {SESSION_UPDATE_COLS}"
        );
        sqlx::query(&sql)
            .bind(&page_id.0)
            .bind(token_hash)
            .bind(origin_host)
            .bind(now.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .map(|row| self.row_to_access_session(row))
            .transpose()
    }

    async fn list_access_sessions(
        &self,
        org_id: &Id,
        page_id: &Id,
        now: TimestampMicros,
    ) -> Result<Vec<StatusPageAccessSession>> {
        let sql = format!(
            "SELECT {SESSION_COLS} FROM status_page_access_sessions
             WHERE org_id = $1 AND status_page_id = $2
               AND revoked_at_micros IS NULL AND expires_at_micros > $3
             ORDER BY last_seen_at_micros DESC, id DESC LIMIT 1000"
        );
        sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&page_id.0)
            .bind(now.0)
            .fetch_all(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .into_iter()
            .map(|row| self.row_to_access_session(row))
            .collect()
    }

    async fn revoke_access_session(
        &self,
        org_id: &Id,
        page_id: &Id,
        session_id: &Id,
        revoked_at: TimestampMicros,
    ) -> Result<()> {
        let result = sqlx::query(
            "UPDATE status_page_access_sessions SET revoked_at_micros = $4
             WHERE org_id = $1 AND status_page_id = $2 AND id = $3
               AND revoked_at_micros IS NULL",
        )
        .bind(&org_id.0)
        .bind(&page_id.0)
        .bind(&session_id.0)
        .bind(revoked_at.0)
        .execute(&self.pool)
        .await
        .map_err(super::sqlx_err)?;
        if result.rows_affected() == 0 {
            return Err(Error::not_found(
                "active status-page access session not found",
            ));
        }
        Ok(())
    }

    async fn revoke_all_access_sessions(
        &self,
        org_id: &Id,
        page_id: &Id,
        revoked_at: TimestampMicros,
    ) -> Result<u64> {
        sqlx::query(
            "UPDATE status_page_access_sessions SET revoked_at_micros = $3
             WHERE org_id = $1 AND status_page_id = $2 AND revoked_at_micros IS NULL",
        )
        .bind(&org_id.0)
        .bind(&page_id.0)
        .bind(revoked_at.0)
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected())
        .map_err(super::sqlx_err)
    }

    async fn purge_expired_access_artifacts(
        &self,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<u64> {
        let limit = i64::from(limit.min(1000));
        let mut transaction = sqlx::begin(&self.pool).await.map_err(super::sqlx_err)?;
        let sessions = sqlx::query(
            "WITH expired AS (
                SELECT id FROM status_page_access_sessions
                WHERE expires_at_micros <= $1 OR revoked_at_micros IS NOT NULL
                ORDER BY expires_at_micros, id LIMIT $2 FOR UPDATE SKIP LOCKED
             )
             DELETE FROM status_page_access_sessions session
             USING expired WHERE session.id = expired.id",
        )
        .bind(now.0)
        .bind(limit)
        .execute(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?
        .rows_affected();
        let links = sqlx::query(
            "WITH expired AS (
                SELECT id FROM status_page_magic_links
                WHERE expires_at_micros <= $1 OR consumed_at_micros IS NOT NULL
                ORDER BY expires_at_micros, id LIMIT $2 FOR UPDATE SKIP LOCKED
             )
             DELETE FROM status_page_magic_links link
             USING expired WHERE link.id = expired.id",
        )
        .bind(now.0)
        .bind(limit)
        .execute(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?
        .rows_affected();
        transaction.commit().await.map_err(super::sqlx_err)?;
        Ok(sessions.saturating_add(links))
    }
}

fn access_token_error(error: sqlx::Error, message: &'static str) -> Error {
    if matches!(error, sqlx::Error::RowNotFound) {
        Error::invalid(message)
    } else {
        super::sqlx_err(error)
    }
}
