// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::Row;

use super::PgStatusPageRepository;
use crate::{
    domain::status_page::{
        PendingStatusPageSubscription, StatusPageSubscriber, StatusPageSubscriberChannel,
        StatusPageSubscriberPage, StatusPageSubscriberRepository, StatusPageSubscriberStatus,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) const SUBSCRIBER_COLS: &str = "id, org_id, status_page_id, channel,
    target_ciphertext, target_nonce, status, token_hash,
    confirmation_sent_at_micros, confirmed_at_micros,
    unsubscribed_at_micros, created_at_micros, updated_at_micros";
const MAX_SUBSCRIBERS_PER_PAGE: i64 = 10_000;

impl PgStatusPageRepository {
    pub(super) fn target_hash(&self, org_id: &Id, target: &str) -> String {
        self.cipher
            .org_hmac_sha256(org_id.as_str(), target.as_bytes())
    }

    fn seal_target(&self, target: &str) -> Result<(Vec<u8>, Vec<u8>)> {
        self.cipher
            .seal(target.as_bytes())
            .map_err(|_| Error::internal("status-page subscriber target encryption failed"))
    }

    pub(super) fn open_target(&self, nonce: &[u8], ciphertext: &[u8]) -> Result<String> {
        let plaintext = self
            .cipher
            .open(nonce, ciphertext)
            .map_err(|_| Error::internal("status-page subscriber target decryption failed"))?;
        String::from_utf8(plaintext)
            .map_err(|_| Error::internal("status-page subscriber target is not UTF-8"))
    }
}

#[async_trait]
impl StatusPageSubscriberRepository for PgStatusPageRepository {
    async fn upsert_pending_subscriber(
        &self,
        subscriber: StatusPageSubscriber,
        resend_before: TimestampMicros,
    ) -> Result<PendingStatusPageSubscription> {
        let target_hash = self.target_hash(&subscriber.org_id, &subscriber.target);
        let (target_nonce, target_ciphertext) = self.seal_target(&subscriber.target)?;
        let mut transaction = sqlx::begin(&self.pool).await.map_err(super::sqlx_err)?;
        let lock_key = format!(
            "status-page-subscribers:{}:{}",
            subscriber.org_id, subscriber.status_page_id
        );
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(lock_key)
            .execute(&mut *transaction)
            .await
            .map_err(super::sqlx_err)?;
        let select_sql = format!(
            "SELECT {SUBSCRIBER_COLS} FROM status_page_subscribers
             WHERE org_id = $1 AND status_page_id = $2 AND channel = $3
               AND target_hash = $4 FOR UPDATE"
        );
        let existing = sqlx::query(&select_sql)
            .bind(&subscriber.org_id.0)
            .bind(&subscriber.status_page_id.0)
            .bind(subscriber.channel.as_str())
            .bind(&target_hash)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(super::sqlx_err)?
            .map(|row| self.row_to_subscriber(row))
            .transpose()?;
        if let Some(existing) = existing {
            let cooling_down = existing
                .confirmation_sent_at
                .is_some_and(|sent_at| sent_at > resend_before);
            if existing.status == StatusPageSubscriberStatus::Active || cooling_down {
                transaction.commit().await.map_err(super::sqlx_err)?;
                return Ok(PendingStatusPageSubscription {
                    subscriber: existing,
                    should_send_confirmation: false,
                });
            }
            let update_sql = format!(
                "UPDATE status_page_subscribers SET
                    target_ciphertext = $4, target_nonce = $5, target_hash = $6,
                    status = 'pending', token_hash = $7,
                    confirmation_sent_at_micros = $8, confirmed_at_micros = NULL,
                    unsubscribed_at_micros = NULL, updated_at_micros = $8
                 WHERE org_id = $1 AND status_page_id = $2 AND id = $3
                 RETURNING {SUBSCRIBER_COLS}"
            );
            let row = sqlx::query(&update_sql)
                .bind(&existing.org_id.0)
                .bind(&existing.status_page_id.0)
                .bind(&existing.id.0)
                .bind(&target_ciphertext)
                .bind(&target_nonce)
                .bind(&target_hash)
                .bind(&subscriber.token_hash)
                .bind(subscriber.updated_at.0)
                .fetch_one(&mut *transaction)
                .await
                .map_err(super::sqlx_err)?;
            let subscriber = self.row_to_subscriber(row)?;
            transaction.commit().await.map_err(super::sqlx_err)?;
            return Ok(PendingStatusPageSubscription {
                subscriber,
                should_send_confirmation: true,
            });
        }
        let active_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM status_page_subscribers
             WHERE org_id = $1 AND status_page_id = $2 AND status <> 'unsubscribed'",
        )
        .bind(&subscriber.org_id.0)
        .bind(&subscriber.status_page_id.0)
        .fetch_one(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;
        if active_count >= MAX_SUBSCRIBERS_PER_PAGE {
            return Err(Error::resource_exhausted(
                "status page subscriber limit reached",
            ));
        }
        let insert_sql = format!(
            "INSERT INTO status_page_subscribers
                (id, org_id, status_page_id, channel, target_ciphertext, target_nonce,
                 target_hash, status, token_hash, confirmation_sent_at_micros,
                 confirmed_at_micros, unsubscribed_at_micros,
                 created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, NULL, NULL, $11, $12)
             RETURNING {SUBSCRIBER_COLS}"
        );
        let row = sqlx::query(&insert_sql)
            .bind(&subscriber.id.0)
            .bind(&subscriber.org_id.0)
            .bind(&subscriber.status_page_id.0)
            .bind(subscriber.channel.as_str())
            .bind(&target_ciphertext)
            .bind(&target_nonce)
            .bind(&target_hash)
            .bind(subscriber.status.as_str())
            .bind(&subscriber.token_hash)
            .bind(subscriber.confirmation_sent_at.map(|at| at.0))
            .bind(subscriber.created_at.0)
            .bind(subscriber.updated_at.0)
            .fetch_one(&mut *transaction)
            .await
            .map_err(super::sqlx_err)?;
        let subscriber = self.row_to_subscriber(row)?;
        transaction.commit().await.map_err(super::sqlx_err)?;
        Ok(PendingStatusPageSubscription {
            subscriber,
            should_send_confirmation: true,
        })
    }

    async fn confirm_subscriber(
        &self,
        org_id: &Id,
        page_id: &Id,
        token_hash: &str,
        confirmed_at: TimestampMicros,
    ) -> Result<StatusPageSubscriber> {
        let sql = format!(
            "UPDATE status_page_subscribers SET status = 'active',
                 confirmed_at_micros = $4, unsubscribed_at_micros = NULL,
                 updated_at_micros = $4
             WHERE org_id = $1 AND status_page_id = $2 AND token_hash = $3
               AND status IN ('pending', 'active') RETURNING {SUBSCRIBER_COLS}"
        );
        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&page_id.0)
            .bind(token_hash)
            .bind(confirmed_at.0)
            .fetch_one(&self.pool)
            .await
            .map_err(|error| token_lookup_error(error, "subscription confirmation is invalid"))?;
        self.row_to_subscriber(row)
    }

    async fn reset_pending_confirmation(&self, subscriber_id: &Id, token_hash: &str) -> Result<()> {
        sqlx::query(
            "UPDATE status_page_subscribers SET confirmation_sent_at_micros = NULL
             WHERE id = $1 AND token_hash = $2 AND status = 'pending'",
        )
        .bind(&subscriber_id.0)
        .bind(token_hash)
        .execute(&self.pool)
        .await
        .map_err(super::sqlx_err)?;
        Ok(())
    }

    async fn unsubscribe_subscriber(
        &self,
        org_id: &Id,
        page_id: &Id,
        token_hash: &str,
        unsubscribed_at: TimestampMicros,
    ) -> Result<StatusPageSubscriber> {
        update_subscriber_to_unsubscribed(
            self,
            org_id,
            page_id,
            "token_hash",
            token_hash,
            unsubscribed_at,
        )
        .await
    }

    async fn list_subscribers(
        &self,
        org_id: &Id,
        page_id: &Id,
        page: u32,
        per_page: u32,
    ) -> Result<StatusPageSubscriberPage> {
        let per_page = per_page.clamp(1, 100);
        let offset = u64::from(page.saturating_sub(1)).saturating_mul(u64::from(per_page));
        let total: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM status_page_subscribers
             WHERE org_id = $1 AND status_page_id = $2",
        )
        .bind(&org_id.0)
        .bind(&page_id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(super::sqlx_err)?;
        let sql = format!(
            "SELECT {SUBSCRIBER_COLS} FROM status_page_subscribers
             WHERE org_id = $1 AND status_page_id = $2
             ORDER BY updated_at_micros DESC, id DESC LIMIT $3 OFFSET $4"
        );
        let items = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&page_id.0)
            .bind(i64::from(per_page))
            .bind(i64::try_from(offset).unwrap_or(i64::MAX))
            .fetch_all(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .into_iter()
            .map(|row| self.row_to_subscriber(row))
            .collect::<Result<Vec<_>>>()?;
        Ok(StatusPageSubscriberPage {
            items,
            page,
            per_page,
            total: u64::try_from(total.max(0)).unwrap_or(u64::MAX),
        })
    }

    async fn get_subscriber(
        &self,
        org_id: &Id,
        page_id: &Id,
        subscriber_id: &Id,
    ) -> Result<StatusPageSubscriber> {
        let sql = format!(
            "SELECT {SUBSCRIBER_COLS} FROM status_page_subscribers
             WHERE org_id = $1 AND status_page_id = $2 AND id = $3"
        );
        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&page_id.0)
            .bind(&subscriber_id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(super::sqlx_err)?;
        self.row_to_subscriber(row)
    }

    async fn revoke_subscriber(
        &self,
        org_id: &Id,
        page_id: &Id,
        subscriber_id: &Id,
        revoked_at: TimestampMicros,
    ) -> Result<StatusPageSubscriber> {
        update_subscriber_to_unsubscribed(
            self,
            org_id,
            page_id,
            "id",
            subscriber_id.as_str(),
            revoked_at,
        )
        .await
    }

    async fn refresh_pending_confirmation(
        &self,
        org_id: &Id,
        page_id: &Id,
        subscriber_id: &Id,
        token_hash: &str,
        sent_at: TimestampMicros,
    ) -> Result<StatusPageSubscriber> {
        let sql = format!(
            "UPDATE status_page_subscribers SET token_hash = $4,
                 confirmation_sent_at_micros = $5, updated_at_micros = $5
             WHERE org_id = $1 AND status_page_id = $2 AND id = $3 AND status = 'pending'
             RETURNING {SUBSCRIBER_COLS}"
        );
        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&page_id.0)
            .bind(&subscriber_id.0)
            .bind(token_hash)
            .bind(sent_at.0)
            .fetch_one(&self.pool)
            .await
            .map_err(super::sqlx_err)?;
        self.row_to_subscriber(row)
    }
}

impl PgStatusPageRepository {
    pub(super) fn row_to_subscriber(
        &self,
        row: sqlx::postgres::PgRow,
    ) -> Result<StatusPageSubscriber> {
        let channel: String = row.try_get("channel").map_err(super::sqlx_err)?;
        let status: String = row.try_get("status").map_err(super::sqlx_err)?;
        let ciphertext: Vec<u8> = row.try_get("target_ciphertext").map_err(super::sqlx_err)?;
        let nonce: Vec<u8> = row.try_get("target_nonce").map_err(super::sqlx_err)?;
        Ok(StatusPageSubscriber {
            id: Id(row.try_get("id").map_err(super::sqlx_err)?),
            org_id: Id(row.try_get("org_id").map_err(super::sqlx_err)?),
            status_page_id: Id(row.try_get("status_page_id").map_err(super::sqlx_err)?),
            channel: StatusPageSubscriberChannel::parse(&channel)
                .ok_or_else(|| Error::internal(format!("unknown subscriber channel: {channel}")))?,
            target: self.open_target(&nonce, &ciphertext)?,
            status: StatusPageSubscriberStatus::parse(&status)
                .ok_or_else(|| Error::internal(format!("unknown subscriber status: {status}")))?,
            token_hash: row.try_get("token_hash").map_err(super::sqlx_err)?,
            confirmation_sent_at: optional_timestamp(&row, "confirmation_sent_at_micros")?,
            confirmed_at: optional_timestamp(&row, "confirmed_at_micros")?,
            unsubscribed_at: optional_timestamp(&row, "unsubscribed_at_micros")?,
            created_at: TimestampMicros(row.try_get("created_at_micros").map_err(super::sqlx_err)?),
            updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(super::sqlx_err)?),
        })
    }
}

async fn update_subscriber_to_unsubscribed(
    repository: &PgStatusPageRepository,
    org_id: &Id,
    page_id: &Id,
    key: &str,
    value: &str,
    unsubscribed_at: TimestampMicros,
) -> Result<StatusPageSubscriber> {
    let mut transaction = sqlx::begin(&repository.pool)
        .await
        .map_err(super::sqlx_err)?;
    let sql = format!(
        "UPDATE status_page_subscribers SET status = 'unsubscribed',
             unsubscribed_at_micros = $4, updated_at_micros = $4
         WHERE org_id = $1 AND status_page_id = $2 AND {key} = $3
           AND status IN ('pending', 'active', 'unsubscribed')
         RETURNING {SUBSCRIBER_COLS}"
    );
    let row = sqlx::query(&sql)
        .bind(&org_id.0)
        .bind(&page_id.0)
        .bind(value)
        .bind(unsubscribed_at.0)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|error| token_lookup_error(error, "subscription not found"))?;
    let subscriber = repository.row_to_subscriber(row)?;
    sqlx::query(
        "UPDATE status_page_notification_deliveries SET status = 'failed',
             claimed_at_micros = NULL, last_error = 'subscriber unsubscribed',
             updated_at_micros = $4
         WHERE org_id = $1 AND status_page_id = $2 AND subscriber_id = $3
           AND status IN ('pending', 'processing')",
    )
    .bind(&subscriber.org_id.0)
    .bind(&subscriber.status_page_id.0)
    .bind(&subscriber.id.0)
    .bind(unsubscribed_at.0)
    .execute(&mut *transaction)
    .await
    .map_err(super::sqlx_err)?;
    transaction.commit().await.map_err(super::sqlx_err)?;
    Ok(subscriber)
}

pub(super) fn optional_timestamp(
    row: &sqlx::postgres::PgRow,
    column: &str,
) -> Result<Option<TimestampMicros>> {
    row.try_get::<Option<i64>, _>(column)
        .map(|value| value.map(TimestampMicros))
        .map_err(super::sqlx_err)
}

fn token_lookup_error(error: sqlx::Error, message: &'static str) -> Error {
    if matches!(error, sqlx::Error::RowNotFound) {
        Error::invalid(message)
    } else {
        super::sqlx_err(error)
    }
}

#[cfg(test)]
mod tests {
    use base64::Engine as _;

    use super::*;
    use crate::infra::cipher::CipherRootKey;

    #[tokio::test]
    async fn subscriber_targets_are_encrypted_and_hashed_per_organization() {
        let key = base64::engine::general_purpose::STANDARD.encode([11u8; 32]);
        let repository = PgStatusPageRepository::new(
            sqlx::PgPool::connect_lazy("postgres://unused").unwrap(),
            CipherRootKey::from_base64(&key).unwrap(),
        );
        let target = "https://hooks.example/secret-token";
        let (nonce, ciphertext) = repository.seal_target(target).unwrap();
        assert_eq!(repository.open_target(&nonce, &ciphertext).unwrap(), target);
        assert!(
            !ciphertext
                .windows(target.len())
                .any(|part| part == target.as_bytes())
        );
        assert_ne!(
            repository.target_hash(&Id("org-a".into()), target),
            repository.target_hash(&Id("org-b".into()), target),
        );
    }
}
