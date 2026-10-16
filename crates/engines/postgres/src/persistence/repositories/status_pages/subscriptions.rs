// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Subscribers of a status page. Targets and tokens are sealed; only their
//! hashes take part in lookups.

mod import;
mod requests;
mod slack_app;

use async_trait::async_trait;
use sqlx::Row;

use super::PgStatusPageRepository;
use crate::{
    domain::status_page::{
        PendingStatusPageSubscription, StatusPageSubscriber, StatusPageSubscriberChannel,
        StatusPageSubscriberImport, StatusPageSubscriberImportOutcome, StatusPageSubscriberOrigin,
        StatusPageSubscriberPage, StatusPageSubscriberRepository, StatusPageSubscriberStatus,
        SubscriberComponentScope, SubscriberComponents,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) const SUBSCRIBER_COLS: &str = "id, org_id, status_page_id, channel,
    target_ciphertext, target_nonce, target_label, status, token_hash,
    token_ciphertext, token_nonce, component_scope, component_ids, origin,
    confirmation_sent_at_micros, confirmed_at_micros,
    unsubscribed_at_micros, created_at_micros, updated_at_micros";

const MAX_SUBSCRIBERS_PER_PAGE: i64 = 10_000;

/// A subscriber of "Add to Slack" that waits for the visitor to come back from
/// Slack is not a subscription anyone can act on yet.
const NOT_WAITING_FOR_SLACK: &str = "NOT (origin = 'slack_app' AND status = 'pending')";

impl PgStatusPageRepository {
    pub(super) fn target_hash(&self, org_id: &Id, target: &str) -> String {
        self.cipher
            .org_hmac_sha256(org_id.as_str(), target.as_bytes())
    }

    fn seal_secret(&self, value: &str) -> Result<(Vec<u8>, Vec<u8>)> {
        self.cipher
            .seal(value.as_bytes())
            .map_err(|_| Error::internal("status-page subscriber encryption failed"))
    }

    pub(super) fn open_secret(&self, nonce: &[u8], ciphertext: &[u8]) -> Result<String> {
        let plaintext = self
            .cipher
            .open(nonce, ciphertext)
            .map_err(|_| Error::internal("status-page subscriber decryption failed"))?;

        String::from_utf8(plaintext)
            .map_err(|_| Error::internal("status-page subscriber secret is not UTF-8"))
    }
}

#[async_trait]
impl StatusPageSubscriberRepository for PgStatusPageRepository {
    async fn upsert_pending_subscriber(
        &self,
        subscriber: StatusPageSubscriber,
        resend_before: TimestampMicros,
    ) -> Result<PendingStatusPageSubscription> {
        requests::upsert_pending(self, subscriber, resend_before).await
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
               AND status IN ('pending', 'active') AND {NOT_WAITING_FOR_SLACK}
             RETURNING {SUBSCRIBER_COLS}"
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
             WHERE id = $1 AND token_hash = $2 AND status IN ('pending', 'active')",
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

        let count_sql = format!(
            "SELECT count(*) FROM status_page_subscribers
             WHERE org_id = $1 AND status_page_id = $2 AND {NOT_WAITING_FOR_SLACK}"
        );

        let total: i64 = sqlx::query_scalar(&count_sql)
            .bind(&org_id.0)
            .bind(&page_id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(super::sqlx_err)?;

        let sql = format!(
            "SELECT {SUBSCRIBER_COLS} FROM status_page_subscribers
             WHERE org_id = $1 AND status_page_id = $2 AND {NOT_WAITING_FOR_SLACK}
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
        token: &str,
        token_hash: &str,
        sent_at: TimestampMicros,
    ) -> Result<StatusPageSubscriber> {
        let (token_nonce, token_ciphertext) = self.seal_secret(token)?;

        let sql = format!(
            "UPDATE status_page_subscribers SET token_hash = $4,
                 token_ciphertext = $5, token_nonce = $6,
                 confirmation_sent_at_micros = $7, updated_at_micros = $7
             WHERE org_id = $1 AND status_page_id = $2 AND id = $3 AND status = 'pending'
               AND {NOT_WAITING_FOR_SLACK}
             RETURNING {SUBSCRIBER_COLS}"
        );

        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&page_id.0)
            .bind(&subscriber_id.0)
            .bind(token_hash)
            .bind(&token_ciphertext)
            .bind(&token_nonce)
            .bind(sent_at.0)
            .fetch_one(&self.pool)
            .await
            .map_err(super::sqlx_err)?;

        self.row_to_subscriber(row)
    }

    async fn get_subscriber_by_token(
        &self,
        org_id: &Id,
        page_id: &Id,
        token_hash: &str,
    ) -> Result<StatusPageSubscriber> {
        let sql = format!(
            "SELECT {SUBSCRIBER_COLS} FROM status_page_subscribers
             WHERE org_id = $1 AND status_page_id = $2 AND token_hash = $3
               AND status IN ('pending', 'active') AND {NOT_WAITING_FOR_SLACK}"
        );

        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&page_id.0)
            .bind(token_hash)
            .fetch_one(&self.pool)
            .await
            .map_err(|error| token_lookup_error(error, "subscription link is invalid"))?;

        self.row_to_subscriber(row)
    }

    async fn update_subscriber_components(
        &self,
        org_id: &Id,
        page_id: &Id,
        token_hash: &str,
        components: &SubscriberComponents,
        updated_at: TimestampMicros,
    ) -> Result<StatusPageSubscriber> {
        let sql = format!(
            "UPDATE status_page_subscribers SET component_scope = $4,
                 component_ids = $5, updated_at_micros = $6
             WHERE org_id = $1 AND status_page_id = $2 AND token_hash = $3
               AND status IN ('pending', 'active') AND {NOT_WAITING_FOR_SLACK}
             RETURNING {SUBSCRIBER_COLS}"
        );

        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&page_id.0)
            .bind(token_hash)
            .bind(components.scope.as_str())
            .bind(component_id_strings(components))
            .bind(updated_at.0)
            .fetch_one(&self.pool)
            .await
            .map_err(|error| token_lookup_error(error, "subscription link is invalid"))?;

        self.row_to_subscriber(row)
    }

    async fn import_subscribers(
        &self,
        org_id: &Id,
        page_id: &Id,
        import: &StatusPageSubscriberImport,
    ) -> Result<StatusPageSubscriberImportOutcome> {
        import::import(self, org_id, page_id, import).await
    }
}

impl PgStatusPageRepository {
    pub(super) fn row_to_subscriber(
        &self,
        row: sqlx::postgres::PgRow,
    ) -> Result<StatusPageSubscriber> {
        let channel: String = row.try_get("channel").map_err(super::sqlx_err)?;
        let status: String = row.try_get("status").map_err(super::sqlx_err)?;
        let origin: String = row.try_get("origin").map_err(super::sqlx_err)?;
        let ciphertext: Vec<u8> = row.try_get("target_ciphertext").map_err(super::sqlx_err)?;
        let nonce: Vec<u8> = row.try_get("target_nonce").map_err(super::sqlx_err)?;

        let token_ciphertext: Vec<u8> = row.try_get("token_ciphertext").map_err(super::sqlx_err)?;

        let token_nonce: Vec<u8> = row.try_get("token_nonce").map_err(super::sqlx_err)?;

        Ok(StatusPageSubscriber {
            id: Id(row.try_get("id").map_err(super::sqlx_err)?),
            org_id: Id(row.try_get("org_id").map_err(super::sqlx_err)?),
            status_page_id: Id(row.try_get("status_page_id").map_err(super::sqlx_err)?),
            channel: StatusPageSubscriberChannel::parse(&channel)
                .ok_or_else(|| Error::internal(format!("unknown subscriber channel: {channel}")))?,
            target: self.open_secret(&nonce, &ciphertext)?,
            target_label: row.try_get("target_label").map_err(super::sqlx_err)?,
            status: StatusPageSubscriberStatus::parse(&status)
                .ok_or_else(|| Error::internal(format!("unknown subscriber status: {status}")))?,
            token_hash: row.try_get("token_hash").map_err(super::sqlx_err)?,
            token: self.open_secret(&token_nonce, &token_ciphertext)?,
            components: row_to_components(&row)?,
            origin: StatusPageSubscriberOrigin::parse(&origin)
                .ok_or_else(|| Error::internal(format!("unknown subscriber origin: {origin}")))?,
            confirmation_sent_at: optional_timestamp(&row, "confirmation_sent_at_micros")?,
            confirmed_at: optional_timestamp(&row, "confirmed_at_micros")?,
            unsubscribed_at: optional_timestamp(&row, "unsubscribed_at_micros")?,
            created_at: TimestampMicros(row.try_get("created_at_micros").map_err(super::sqlx_err)?),
            updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(super::sqlx_err)?),
        })
    }
}

/// What a subscriber hears about, from its `component_scope` and
/// `component_ids` columns.
pub(super) fn row_to_components(row: &sqlx::postgres::PgRow) -> Result<SubscriberComponents> {
    let scope: String = row.try_get("component_scope").map_err(super::sqlx_err)?;
    let component_ids: Vec<String> = row.try_get("component_ids").map_err(super::sqlx_err)?;

    Ok(SubscriberComponents {
        scope: SubscriberComponentScope::parse(&scope)
            .ok_or_else(|| Error::internal(format!("unknown subscriber scope: {scope}")))?,
        component_ids: component_ids.into_iter().map(Id).collect(),
    })
}

pub(super) fn component_id_strings(components: &SubscriberComponents) -> Vec<String> {
    match components.scope {
        SubscriberComponentScope::All => Vec::new(),
        SubscriberComponentScope::Selected => components
            .component_ids
            .iter()
            .map(|id| id.0.clone())
            .collect(),
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
           AND status IN ('pending', 'active', 'unsubscribed') AND {NOT_WAITING_FOR_SLACK}
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

/// Serializes subscription changes on one page, so the limit on its
/// subscribers holds under concurrent requests and imports.
async fn lock_page_subscribers(
    transaction: &mut sqlx::PgConnection,
    org_id: &Id,
    page_id: &Id,
) -> Result<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("status-page-subscribers:{org_id}:{page_id}"))
        .execute(transaction)
        .await
        .map_err(super::sqlx_err)?;

    Ok(())
}

/// How many subscribers of the page have not unsubscribed.
async fn count_subscribed(
    transaction: &mut sqlx::PgConnection,
    org_id: &Id,
    page_id: &Id,
) -> Result<i64> {
    sqlx::query_scalar(
        "SELECT count(*) FROM status_page_subscribers
         WHERE org_id = $1 AND status_page_id = $2 AND status <> 'unsubscribed'",
    )
    .bind(&org_id.0)
    .bind(&page_id.0)
    .fetch_one(transaction)
    .await
    .map_err(super::sqlx_err)
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
        let (nonce, ciphertext) = repository.seal_secret(target).unwrap();
        assert_eq!(repository.open_secret(&nonce, &ciphertext).unwrap(), target);
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

    #[test]
    fn a_whole_page_subscriber_stores_no_component_ids() {
        let chosen = SubscriberComponents::chosen(vec![Id("api".into())]);
        assert_eq!(component_id_strings(&chosen), ["api"]);
        assert!(component_id_strings(&SubscriberComponents::all()).is_empty());
    }
}
