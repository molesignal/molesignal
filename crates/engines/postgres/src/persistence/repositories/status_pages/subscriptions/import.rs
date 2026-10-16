// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Email subscribers brought over from another status page.

use std::collections::HashMap;

use super::{
    super::PgStatusPageRepository, MAX_SUBSCRIBERS_PER_PAGE, component_id_strings,
    count_subscribed, lock_page_subscribers, requests::insert_subscriber,
};
use crate::{
    domain::status_page::{
        ImportedStatusPageSubscriber, StatusPageSubscriber, StatusPageSubscriberChannel,
        StatusPageSubscriberImport, StatusPageSubscriberImportMode,
        StatusPageSubscriberImportOutcome, StatusPageSubscriberOrigin, StatusPageSubscriberStatus,
    },
    shared::{Error, Result, ids::Id},
};

/// An address the import subscribes, and the row it already has, if any.
struct Planned<'a> {
    subscriber: &'a ImportedStatusPageSubscriber,
    target_hash: String,
    pending_id: Option<String>,
}

pub(super) async fn import(
    repository: &PgStatusPageRepository,
    org_id: &Id,
    page_id: &Id,
    import: &StatusPageSubscriberImport,
) -> Result<StatusPageSubscriberImportOutcome> {
    let hashes: Vec<String> = import
        .subscribers
        .iter()
        .map(|subscriber| repository.target_hash(org_id, &subscriber.target))
        .collect();

    let mut transaction = sqlx::begin(&repository.pool)
        .await
        .map_err(super::super::sqlx_err)?;
    lock_page_subscribers(&mut transaction, org_id, page_id).await?;

    let existing: HashMap<String, (String, String)> = sqlx::query_as::<(String, String, String)>(
        "SELECT target_hash, id, status FROM status_page_subscribers
         WHERE org_id = $1 AND status_page_id = $2 AND channel = 'email'
           AND target_hash = ANY($3) FOR UPDATE",
    )
    .bind(&org_id.0)
    .bind(&page_id.0)
    .bind(&hashes)
    .fetch_all(&mut *transaction)
    .await
    .map_err(super::super::sqlx_err)?
    .into_iter()
    .map(|(hash, id, status)| (hash, (id, status)))
    .collect();

    let mut outcome = StatusPageSubscriberImportOutcome::default();
    let mut planned = Vec::new();

    for (subscriber, target_hash) in import.subscribers.iter().zip(hashes) {
        let pending_id = match existing.get(&target_hash) {
            None => None,
            Some((_, status)) if status == StatusPageSubscriberStatus::Active.as_str() => {
                outcome.already_subscribed += 1;
                continue;
            }
            Some((_, status)) if status == StatusPageSubscriberStatus::Unsubscribed.as_str() => {
                outcome.skipped_unsubscribed += 1;
                continue;
            }
            Some((id, _)) => Some(id.clone()),
        };

        outcome.imported += 1;

        planned.push(Planned {
            subscriber,
            target_hash,
            pending_id,
        });
    }

    if import.mode == StatusPageSubscriberImportMode::Confirm {
        outcome.confirmations_queued = outcome.imported;
    }

    let added = planned
        .iter()
        .filter(|plan| plan.pending_id.is_none())
        .count();
    let subscribed = count_subscribed(&mut transaction, org_id, page_id).await?;

    if subscribed.saturating_add(i64::try_from(added).unwrap_or(i64::MAX))
        > MAX_SUBSCRIBERS_PER_PAGE
    {
        return Err(Error::resource_exhausted(
            "the import would take the page past 10,000 subscribers",
        ));
    }

    if import.dry_run {
        return Ok(outcome);
    }

    let mut subscribed_ids = Vec::with_capacity(planned.len());

    for plan in &planned {
        let id = match &plan.pending_id {
            Some(id) => {
                update_pending(repository, &mut transaction, plan, id, import).await?;
                id.clone()
            }
            None => insert_new(repository, &mut transaction, org_id, page_id, plan, import).await?,
        };

        subscribed_ids.push(id);
    }

    if import.mode == StatusPageSubscriberImportMode::Confirm {
        queue_confirmations(&mut transaction, org_id, page_id, &subscribed_ids, import).await?;
    }

    transaction.commit().await.map_err(super::super::sqlx_err)?;
    Ok(outcome)
}

async fn insert_new(
    repository: &PgStatusPageRepository,
    transaction: &mut sqlx::PgConnection,
    org_id: &Id,
    page_id: &Id,
    plan: &Planned<'_>,
    import: &StatusPageSubscriberImport,
) -> Result<String> {
    let active = import.mode == StatusPageSubscriberImportMode::Active;
    let at = import.imported_at;
    let (target_nonce, target_ciphertext) = repository.seal_secret(&plan.subscriber.target)?;
    let (token_nonce, token_ciphertext) = repository.seal_secret(&plan.subscriber.token)?;

    let subscriber = StatusPageSubscriber {
        id: Id::new(),
        org_id: org_id.clone(),
        status_page_id: page_id.clone(),
        channel: StatusPageSubscriberChannel::Email,
        target: plan.subscriber.target.clone(),
        target_label: None,
        status: if active {
            StatusPageSubscriberStatus::Active
        } else {
            StatusPageSubscriberStatus::Pending
        },
        token_hash: plan.subscriber.token_hash.clone(),
        token: plan.subscriber.token.clone(),
        components: plan.subscriber.components.clone(),
        origin: StatusPageSubscriberOrigin::Import,
        confirmation_sent_at: (!active).then_some(at),
        confirmed_at: active.then_some(at),
        unsubscribed_at: None,
        created_at: at,
        updated_at: at,
    };

    insert_subscriber(
        transaction,
        &subscriber,
        &plan.target_hash,
        (&target_nonce, &target_ciphertext),
        (&token_nonce, &token_ciphertext),
    )
    .await?;

    Ok(subscriber.id.0)
}

/// A pending subscriber the list names: activated, or asked again to confirm
/// with a new token.
async fn update_pending(
    repository: &PgStatusPageRepository,
    transaction: &mut sqlx::PgConnection,
    plan: &Planned<'_>,
    id: &str,
    import: &StatusPageSubscriberImport,
) -> Result<()> {
    let (token_nonce, token_ciphertext) = repository.seal_secret(&plan.subscriber.token)?;
    let active = import.mode == StatusPageSubscriberImportMode::Active;

    sqlx::query(
        "UPDATE status_page_subscribers SET
             status = CASE WHEN $2 THEN 'active' ELSE 'pending' END,
             token_hash = $3, token_ciphertext = $4, token_nonce = $5,
             component_scope = $6, component_ids = $7, origin = 'import',
             confirmed_at_micros = CASE WHEN $2 THEN $8 ELSE NULL END,
             confirmation_sent_at_micros = CASE WHEN $2 THEN confirmation_sent_at_micros ELSE $8 END,
             unsubscribed_at_micros = NULL, updated_at_micros = $8
         WHERE id = $1 AND status = 'pending'",
    )
    .bind(id)
    .bind(active)
    .bind(&plan.subscriber.token_hash)
    .bind(&token_ciphertext)
    .bind(&token_nonce)
    .bind(plan.subscriber.components.scope.as_str())
    .bind(component_id_strings(&plan.subscriber.components))
    .bind(import.imported_at.0)
    .execute(transaction)
    .await
    .map_err(super::super::sqlx_err)?;

    Ok(())
}

/// Queues a confirmation for every address the import left pending. The
/// links are written when the message is sent, from the sealed token.
async fn queue_confirmations(
    transaction: &mut sqlx::PgConnection,
    org_id: &Id,
    page_id: &Id,
    subscriber_ids: &[String],
    import: &StatusPageSubscriberImport,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO status_page_notification_deliveries (
             id, org_id, status_page_id, subscriber_id, event_key, payload,
             next_attempt_at_micros, created_at_micros, updated_at_micros
         )
         SELECT
             md5(subscriber.id || ':subscription_confirmation:' || $3::BIGINT),
             subscriber.org_id,
             subscriber.status_page_id,
             subscriber.id,
             'subscription_confirmation:' || $3::BIGINT,
             jsonb_build_object(
                 'type', 'status_page_subscription_confirmation',
                 'page_name', page.name,
                 'page_slug', page.slug,
                 'occurred_at', $3::BIGINT
             ),
             $3, $3, $3
         FROM status_page_subscribers subscriber
         JOIN status_pages page
           ON page.org_id = subscriber.org_id AND page.id = subscriber.status_page_id
         WHERE subscriber.org_id = $1 AND subscriber.status_page_id = $2
           AND subscriber.id = ANY($4) AND subscriber.status = 'pending'
         ON CONFLICT (subscriber_id, event_key) DO NOTHING",
    )
    .bind(&org_id.0)
    .bind(&page_id.0)
    .bind(import.imported_at.0)
    .bind(subscriber_ids)
    .execute(transaction)
    .await
    .map_err(super::super::sqlx_err)?;

    Ok(())
}
