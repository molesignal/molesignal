// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The postmortem of an incident, kept on the incident row.

use async_trait::async_trait;

use super::{PgStatusPageRepository, event_queries::fetch_incident, touch_page};
use crate::{
    domain::status_page::{PostmortemWrite, StatusPageIncident, StatusPagePostmortemRepository},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[async_trait]
impl StatusPagePostmortemRepository for PgStatusPageRepository {
    async fn save_postmortem(
        &self,
        org_id: &Id,
        page_id: &Id,
        incident_id: &Id,
        body: &str,
        write: PostmortemWrite,
        actor_id: Option<&Id>,
        now: TimestampMicros,
    ) -> Result<StatusPageIncident> {
        let mut transaction = sqlx::begin(&self.pool).await.map_err(super::sqlx_err)?;

        let state: Option<Option<String>> = sqlx::query_scalar(
            "SELECT postmortem_state FROM status_page_incidents
             WHERE org_id = $1 AND status_page_id = $2 AND id = $3
             FOR UPDATE",
        )
        .bind(&org_id.0)
        .bind(&page_id.0)
        .bind(&incident_id.0)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;

        let state = state.ok_or_else(|| Error::not_found("status-page event not found"))?;
        let was_published = state.as_deref() == Some("published");

        if write == PostmortemWrite::DraftOnly && was_published {
            return Err(Error::conflict(
                "the postmortem of this incident is published; revise it on the status page",
            ));
        }

        match write {
            PostmortemWrite::Draft | PostmortemWrite::DraftOnly => {
                sqlx::query(
                    "UPDATE status_page_incidents SET
                         postmortem_body = $4, postmortem_state = 'draft',
                         postmortem_published_at_micros = NULL,
                         postmortem_revised_at_micros = NULL,
                         postmortem_updated_at_micros = $5, postmortem_updated_by = $6,
                         updated_at_micros = CASE WHEN $7 THEN $5 ELSE updated_at_micros END
                     WHERE org_id = $1 AND status_page_id = $2 AND id = $3",
                )
                .bind(&org_id.0)
                .bind(&page_id.0)
                .bind(&incident_id.0)
                .bind(body)
                .bind(now.0)
                .bind(actor_id.map(Id::as_str))
                .bind(was_published)
                .execute(&mut *transaction)
                .await
                .map_err(super::sqlx_err)?;
            }
            PostmortemWrite::Publish { notify_subscribers } => {
                sqlx::query(
                    "UPDATE status_page_incidents SET
                         postmortem_body = $4, postmortem_state = 'published',
                         postmortem_published_at_micros =
                             CASE WHEN $7 THEN postmortem_published_at_micros ELSE $5 END,
                         postmortem_revised_at_micros = CASE WHEN $7 THEN $5 ELSE NULL END,
                         postmortem_updated_at_micros = $5, postmortem_updated_by = $6,
                         updated_at_micros = $5
                     WHERE org_id = $1 AND status_page_id = $2 AND id = $3",
                )
                .bind(&org_id.0)
                .bind(&page_id.0)
                .bind(&incident_id.0)
                .bind(body)
                .bind(now.0)
                .bind(actor_id.map(Id::as_str))
                .bind(was_published)
                .execute(&mut *transaction)
                .await
                .map_err(super::sqlx_err)?;

                if notify_subscribers {
                    queue_postmortem_deliveries(
                        &mut transaction,
                        org_id,
                        page_id,
                        incident_id,
                        was_published,
                        now,
                    )
                    .await?;
                }
            }
        }

        let public_change = was_published || matches!(write, PostmortemWrite::Publish { .. });

        if public_change {
            touch_page(&mut transaction, org_id, page_id, now).await?;
        }

        transaction.commit().await.map_err(super::sqlx_err)?;
        fetch_incident(&self.pool, org_id, page_id, incident_id).await
    }

    async fn delete_postmortem_draft(
        &self,
        org_id: &Id,
        page_id: &Id,
        incident_id: &Id,
    ) -> Result<StatusPageIncident> {
        let deleted = sqlx::query(
            "UPDATE status_page_incidents SET
                 postmortem_body = NULL, postmortem_state = NULL,
                 postmortem_published_at_micros = NULL, postmortem_revised_at_micros = NULL,
                 postmortem_updated_at_micros = NULL, postmortem_updated_by = NULL
             WHERE org_id = $1 AND status_page_id = $2 AND id = $3
               AND postmortem_state = 'draft'",
        )
        .bind(&org_id.0)
        .bind(&page_id.0)
        .bind(&incident_id.0)
        .execute(&self.pool)
        .await
        .map_err(super::sqlx_err)?;

        let incident = fetch_incident(&self.pool, org_id, page_id, incident_id).await?;

        if deleted.rows_affected() == 0 && incident.postmortem.is_some() {
            return Err(Error::conflict(
                "take the postmortem back to a draft before deleting it",
            ));
        }

        Ok(incident)
    }
}

/// Tells the subscribers who hear about the incident that its postmortem is
/// published, choosing them and naming components the way the update
/// triggers do. The text is read when the message is written, so the queue
/// does not hold a copy for every subscriber.
async fn queue_postmortem_deliveries(
    transaction: &mut sqlx::PgConnection,
    org_id: &Id,
    page_id: &Id,
    incident_id: &Id,
    revision: bool,
    now: TimestampMicros,
) -> Result<()> {
    sqlx::query(
        "WITH named AS (
             SELECT status_page_event_component_ids($1, $2, $3) AS component_ids
         )
         INSERT INTO status_page_notification_deliveries (
             id, org_id, status_page_id, subscriber_id, event_key, payload,
             next_attempt_at_micros, created_at_micros, updated_at_micros
         )
         SELECT
             md5(subscriber.id || ':postmortem:' || incident.id || ':' || $4::BIGINT),
             incident.org_id,
             incident.status_page_id,
             subscriber.id,
             'postmortem:' || incident.id || ':' || $4::BIGINT,
             jsonb_build_object(
                 'type', 'postmortem_published',
                 'page_name', page.name,
                 'page_slug', page.slug,
                 'incident_id', incident.id,
                 'title', incident.title,
                 'impact', incident.impact,
                 'revision', $5::BOOLEAN,
                 'occurred_at', $4::BIGINT,
                 'started_at', incident.started_at_micros,
                 'ended_at', incident.ended_at_micros,
                 'component_names', status_page_event_component_names(
                     incident.org_id, incident.status_page_id, incident.id
                 )
             ),
             $4, $4, $4
         FROM status_page_incidents incident
         CROSS JOIN named
         JOIN status_pages page
           ON page.org_id = incident.org_id AND page.id = incident.status_page_id
         JOIN status_page_subscribers subscriber
           ON subscriber.org_id = incident.org_id
          AND subscriber.status_page_id = incident.status_page_id
         WHERE incident.org_id = $1 AND incident.status_page_id = $2 AND incident.id = $3
           AND subscriber.status = 'active'
           AND status_page_subscriber_hears(
               subscriber.component_scope, subscriber.component_ids, named.component_ids
           )
           AND page.lifecycle = 'active'
           AND (page.visibility = 'public' OR subscriber.channel = 'email')
         ON CONFLICT (subscriber_id, event_key) DO NOTHING",
    )
    .bind(&org_id.0)
    .bind(&page_id.0)
    .bind(&incident_id.0)
    .bind(now.0)
    .bind(revision)
    .execute(transaction)
    .await
    .map_err(super::sqlx_err)?;

    Ok(())
}
