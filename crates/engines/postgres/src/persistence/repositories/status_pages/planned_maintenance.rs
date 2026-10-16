// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What planned maintenance does on its own schedule: which maintenance is
//! due to start, and the reminder that goes out before a planned start.

use async_trait::async_trait;

use super::PgStatusPageRepository;
use crate::{
    domain::status_page::{MaintenanceDueToStart, StatusPagePlannedMaintenanceRepository},
    shared::{Result, ids::Id, time::TimestampMicros},
};

#[async_trait]
impl StatusPagePlannedMaintenanceRepository for PgStatusPageRepository {
    async fn list_maintenance_due_to_start(
        &self,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<Vec<MaintenanceDueToStart>> {
        let due: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT incident.org_id, incident.status_page_id, incident.id
             FROM status_page_incidents incident
             JOIN status_pages page
               ON page.org_id = incident.org_id AND page.id = incident.status_page_id
             WHERE incident.kind = 'maintenance'
               AND incident.publication_state = 'published'
               AND incident.status = 'scheduled'
               AND incident.auto_start
               AND incident.started_at_micros <= $1
               AND page.lifecycle = 'active'
             ORDER BY incident.started_at_micros, incident.id
             LIMIT $2",
        )
        .bind(now.0)
        .bind(i64::from(limit.min(500)))
        .fetch_all(&self.pool)
        .await
        .map_err(super::sqlx_err)?;

        Ok(due
            .into_iter()
            .map(
                |(org_id, status_page_id, incident_id)| MaintenanceDueToStart {
                    org_id: Id(org_id),
                    status_page_id: Id(status_page_id),
                    incident_id: Id(incident_id),
                },
            )
            .collect())
    }

    async fn enqueue_maintenance_reminders(&self, now: TimestampMicros) -> Result<u64> {
        // Marking the planned start as reminded of and queueing the deliveries is one
        // statement, so two nodes that sweep at the same moment queue them once. Subscribers
        // are chosen the way the update triggers choose them, through the same functions, and
        // hear about the maintenance only when it names a component they chose.
        let queued = sqlx::query(
            "WITH due AS (
                 UPDATE status_page_incidents incident
                 SET reminded_start_micros = incident.started_at_micros
                 FROM status_pages page
                 WHERE page.org_id = incident.org_id
                   AND page.id = incident.status_page_id
                   AND page.lifecycle = 'active'
                   AND incident.kind = 'maintenance'
                   AND incident.publication_state = 'published'
                   AND incident.status = 'scheduled'
                   AND incident.reminder_minutes > 0
                   AND incident.reminded_start_micros IS DISTINCT FROM incident.started_at_micros
                   AND incident.started_at_micros > $1
                   AND incident.started_at_micros
                       - incident.reminder_minutes::BIGINT * 60000000 <= $1
                 RETURNING incident.id, incident.org_id, incident.status_page_id,
                           incident.title, incident.started_at_micros,
                           incident.reminder_minutes, page.name AS page_name,
                           page.slug AS page_slug, page.visibility AS page_visibility
             )
             INSERT INTO status_page_notification_deliveries (
                 id, org_id, status_page_id, subscriber_id, event_key, payload,
                 next_attempt_at_micros, created_at_micros, updated_at_micros
             )
             SELECT
                 md5(subscriber.id || ':maintenance_reminder:' || due.id || ':'
                     || due.started_at_micros),
                 due.org_id,
                 due.status_page_id,
                 subscriber.id,
                 'maintenance_reminder:' || due.id || ':' || due.started_at_micros,
                 jsonb_build_object(
                     'type', 'maintenance_reminder',
                     'page_name', due.page_name,
                     'page_slug', due.page_slug,
                     'incident_id', due.id,
                     'title', due.title,
                     'occurred_at', $1::BIGINT,
                     'started_at', due.started_at_micros,
                     'component_names', status_page_event_component_names(
                         due.org_id, due.status_page_id, due.id
                     )
                 ),
                 $1,
                 $1,
                 $1
             FROM due
             JOIN status_page_subscribers subscriber
               ON subscriber.org_id = due.org_id
              AND subscriber.status_page_id = due.status_page_id
             WHERE subscriber.status = 'active'
               AND status_page_subscriber_hears(
                   subscriber.component_scope,
                   subscriber.component_ids,
                   status_page_event_component_ids(due.org_id, due.status_page_id, due.id)
               )
               AND (due.page_visibility = 'public' OR subscriber.channel = 'email')
               AND NOT EXISTS (
                   SELECT 1
                   FROM status_page_incident_updates told
                   WHERE told.org_id = due.org_id
                     AND told.status_page_id = due.status_page_id
                     AND told.incident_id = due.id
                     AND told.created_at_micros >= due.started_at_micros
                         - due.reminder_minutes::BIGINT * 60000000
               )
             ON CONFLICT (subscriber_id, event_key) DO NOTHING",
        )
        .bind(now.0)
        .execute(&self.pool)
        .await
        .map_err(super::sqlx_err)?;

        Ok(queued.rows_affected())
    }
}
