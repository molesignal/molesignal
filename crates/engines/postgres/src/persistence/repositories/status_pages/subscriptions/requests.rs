// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A visitor asks a page to keep them informed.

use super::{
    super::PgStatusPageRepository, MAX_SUBSCRIBERS_PER_PAGE, SUBSCRIBER_COLS, component_id_strings,
    count_subscribed, lock_page_subscribers,
};
use crate::{
    domain::status_page::{
        PendingStatusPageSubscription, StatusPageSubscriber, StatusPageSubscriberStatus,
        SubscriptionRequestAction,
    },
    shared::{Error, Result, time::TimestampMicros},
};

pub(super) async fn upsert_pending(
    repository: &PgStatusPageRepository,
    subscriber: StatusPageSubscriber,
    resend_before: TimestampMicros,
) -> Result<PendingStatusPageSubscription> {
    let target_hash = repository.target_hash(&subscriber.org_id, &subscriber.target);
    let (target_nonce, target_ciphertext) = repository.seal_secret(&subscriber.target)?;
    let (token_nonce, token_ciphertext) = repository.seal_secret(&subscriber.token)?;
    let component_ids = component_id_strings(&subscriber.components);
    let mut transaction = sqlx::begin(&repository.pool)
        .await
        .map_err(super::super::sqlx_err)?;

    lock_page_subscribers(
        &mut transaction,
        &subscriber.org_id,
        &subscriber.status_page_id,
    )
    .await?;

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
        .map_err(super::super::sqlx_err)?
        .map(|row| repository.row_to_subscriber(row))
        .transpose()?;

    if let Some(existing) = existing {
        let cooling_down = existing
            .confirmation_sent_at
            .is_some_and(|sent_at| sent_at > resend_before);

        if cooling_down {
            transaction.commit().await.map_err(super::super::sqlx_err)?;

            return Ok(PendingStatusPageSubscription {
                subscriber: existing,
                action: SubscriptionRequestAction::Nothing,
            });
        }

        // An active subscriber keeps its token and components: it is sent the
        // link that changes them, which only the subscriber can open.
        if existing.status == StatusPageSubscriberStatus::Active {
            let sql = format!(
                "UPDATE status_page_subscribers SET confirmation_sent_at_micros = $4
                 WHERE org_id = $1 AND status_page_id = $2 AND id = $3
                 RETURNING {SUBSCRIBER_COLS}"
            );

            let row = sqlx::query(&sql)
                .bind(&existing.org_id.0)
                .bind(&existing.status_page_id.0)
                .bind(&existing.id.0)
                .bind(subscriber.updated_at.0)
                .fetch_one(&mut *transaction)
                .await
                .map_err(super::super::sqlx_err)?;

            let subscriber = repository.row_to_subscriber(row)?;
            transaction.commit().await.map_err(super::super::sqlx_err)?;

            return Ok(PendingStatusPageSubscription {
                subscriber,
                action: SubscriptionRequestAction::Manage,
            });
        }

        let update_sql = format!(
            "UPDATE status_page_subscribers SET
                target_ciphertext = $4, target_nonce = $5, target_hash = $6,
                status = 'pending', token_hash = $7, token_ciphertext = $8, token_nonce = $9,
                component_scope = $10, component_ids = $11, origin = $12,
                confirmation_sent_at_micros = $13, confirmed_at_micros = NULL,
                unsubscribed_at_micros = NULL, updated_at_micros = $13
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
            .bind(&token_ciphertext)
            .bind(&token_nonce)
            .bind(subscriber.components.scope.as_str())
            .bind(&component_ids)
            .bind(subscriber.origin.as_str())
            .bind(subscriber.updated_at.0)
            .fetch_one(&mut *transaction)
            .await
            .map_err(super::super::sqlx_err)?;

        let subscriber = repository.row_to_subscriber(row)?;
        transaction.commit().await.map_err(super::super::sqlx_err)?;

        return Ok(PendingStatusPageSubscription {
            subscriber,
            action: SubscriptionRequestAction::Confirm,
        });
    }

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

    let subscriber = repository.row_to_subscriber(row)?;
    transaction.commit().await.map_err(super::super::sqlx_err)?;

    Ok(PendingStatusPageSubscription {
        subscriber,
        action: SubscriptionRequestAction::Confirm,
    })
}

/// Inserts a subscriber with its sealed target and token, `(nonce,
/// ciphertext)` each.
pub(super) async fn insert_subscriber(
    transaction: &mut sqlx::PgConnection,
    subscriber: &StatusPageSubscriber,
    target_hash: &str,
    target: (&[u8], &[u8]),
    token: (&[u8], &[u8]),
) -> Result<sqlx::postgres::PgRow> {
    let insert_sql = format!(
        "INSERT INTO status_page_subscribers
            (id, org_id, status_page_id, channel, target_ciphertext, target_nonce,
             target_hash, target_label, status, token_hash, token_ciphertext, token_nonce,
             component_scope, component_ids, origin, confirmation_sent_at_micros,
             confirmed_at_micros, unsubscribed_at_micros, created_at_micros, updated_at_micros)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16,
                 $17, NULL, $18, $19)
         RETURNING {SUBSCRIBER_COLS}"
    );

    sqlx::query(&insert_sql)
        .bind(&subscriber.id.0)
        .bind(&subscriber.org_id.0)
        .bind(&subscriber.status_page_id.0)
        .bind(subscriber.channel.as_str())
        .bind(target.1)
        .bind(target.0)
        .bind(target_hash)
        .bind(&subscriber.target_label)
        .bind(subscriber.status.as_str())
        .bind(&subscriber.token_hash)
        .bind(token.1)
        .bind(token.0)
        .bind(subscriber.components.scope.as_str())
        .bind(component_id_strings(&subscriber.components))
        .bind(subscriber.origin.as_str())
        .bind(subscriber.confirmation_sent_at.map(|at| at.0))
        .bind(subscriber.confirmed_at.map(|at| at.0))
        .bind(subscriber.created_at.0)
        .bind(subscriber.updated_at.0)
        .fetch_one(transaction)
        .await
        .map_err(super::super::sqlx_err)
}
