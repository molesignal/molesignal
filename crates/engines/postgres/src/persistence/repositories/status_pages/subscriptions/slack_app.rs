// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! "Add to Slack": the subscriber waits, pending with a placeholder target and
//! the OAuth state as its token, until the visitor comes back from Slack with
//! the incoming webhook Slack created for the channel they chose.

use async_trait::async_trait;

use super::{
    super::PgStatusPageRepository, MAX_SUBSCRIBERS_PER_PAGE, SUBSCRIBER_COLS, count_subscribed,
    lock_page_subscribers, requests::insert_subscriber,
};
use crate::{
    domain::status_page::{SlackAppCompletion, StatusPageSlackAppRepository, StatusPageSubscriber},
    shared::{Error, Result, time::TimestampMicros},
};

const WAITING: &str = "origin = 'slack_app' AND status = 'pending'";

#[async_trait]
impl StatusPageSlackAppRepository for PgStatusPageRepository {
    async fn begin_slack_app_subscription(
        &self,
        subscriber: StatusPageSubscriber,
    ) -> Result<StatusPageSubscriber> {
        let target_hash = self.target_hash(&subscriber.org_id, &subscriber.target);
        let (target_nonce, target_ciphertext) = self.seal_secret(&subscriber.target)?;
        let (token_nonce, token_ciphertext) = self.seal_secret(&subscriber.token)?;
        let mut transaction = sqlx::begin(&self.pool)
            .await
            .map_err(super::super::sqlx_err)?;

        lock_page_subscribers(
            &mut transaction,
            &subscriber.org_id,
            &subscriber.status_page_id,
        )
        .await?;

        let subscribed = count_subscribed(
            &mut transaction,
            &subscriber.org_id,
            &subscriber.status_page_id,
        )
        .await?;

        if subscribed >= MAX_SUBSCRIBERS_PER_PAGE {
            return Err(Error::resource_exhausted(
                "status page subscriber limit reached",
            ));
        }

        let row = insert_subscriber(
            &mut transaction,
            &subscriber,
            &target_hash,
            (&target_nonce, &target_ciphertext),
            (&token_nonce, &token_ciphertext),
        )
        .await?;

        let subscriber = self.row_to_subscriber(row)?;
        transaction.commit().await.map_err(super::super::sqlx_err)?;
        Ok(subscriber)
    }

    async fn waiting_slack_app_subscription(
        &self,
        state_hash: &str,
        begun_after: TimestampMicros,
    ) -> Result<Option<StatusPageSubscriber>> {
        let sql = format!(
            "SELECT {SUBSCRIBER_COLS} FROM status_page_subscribers
             WHERE token_hash = $1 AND {WAITING} AND created_at_micros >= $2"
        );

        sqlx::query(&sql)
            .bind(state_hash)
            .bind(begun_after.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(super::super::sqlx_err)?
            .map(|row| self.row_to_subscriber(row))
            .transpose()
    }

    async fn complete_slack_app_subscription(
        &self,
        waiting: &StatusPageSubscriber,
        completion: SlackAppCompletion,
    ) -> Result<StatusPageSubscriber> {
        let target_hash = self.target_hash(&waiting.org_id, &completion.webhook_url);
        let (target_nonce, target_ciphertext) = self.seal_secret(&completion.webhook_url)?;
        let (token_nonce, token_ciphertext) = self.seal_secret(&completion.token)?;

        let sql = format!(
            "UPDATE status_page_subscribers SET
                 target_ciphertext = $4, target_nonce = $5, target_hash = $6,
                 target_label = $7, token_hash = $8, token_ciphertext = $9, token_nonce = $10,
                 status = 'active', confirmed_at_micros = $11, unsubscribed_at_micros = NULL,
                 updated_at_micros = $11
             WHERE org_id = $1 AND status_page_id = $2 AND id = $3 AND {WAITING}
             RETURNING {SUBSCRIBER_COLS}"
        );

        let row = sqlx::query(&sql)
            .bind(&waiting.org_id.0)
            .bind(&waiting.status_page_id.0)
            .bind(&waiting.id.0)
            .bind(&target_ciphertext)
            .bind(&target_nonce)
            .bind(&target_hash)
            .bind(&completion.label)
            .bind(&completion.token_hash)
            .bind(&token_ciphertext)
            .bind(&token_nonce)
            .bind(completion.completed_at.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(super::super::sqlx_err)?
            .ok_or_else(|| Error::conflict("the Slack request was already completed"))?;

        self.row_to_subscriber(row)
    }

    async fn cancel_slack_app_subscription(&self, waiting: &StatusPageSubscriber) -> Result<()> {
        let sql = format!(
            "DELETE FROM status_page_subscribers
             WHERE org_id = $1 AND status_page_id = $2 AND id = $3 AND {WAITING}"
        );

        sqlx::query(&sql)
            .bind(&waiting.org_id.0)
            .bind(&waiting.status_page_id.0)
            .bind(&waiting.id.0)
            .execute(&self.pool)
            .await
            .map_err(super::super::sqlx_err)?;

        Ok(())
    }

    async fn purge_abandoned_slack_app_subscriptions(
        &self,
        begun_before: TimestampMicros,
    ) -> Result<u64> {
        let sql = format!(
            "DELETE FROM status_page_subscribers WHERE {WAITING} AND created_at_micros < $1"
        );

        let purged = sqlx::query(&sql)
            .bind(begun_before.0)
            .execute(&self.pool)
            .await
            .map_err(super::super::sqlx_err)?;

        Ok(purged.rows_affected())
    }
}
