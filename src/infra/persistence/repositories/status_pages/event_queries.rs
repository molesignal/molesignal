// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashMap;

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use super::{
    PgStatusPageRepository,
    codec::{INCIDENT_COLS, row_to_incident, row_to_update},
};
use crate::{
    domain::status_page::{
        StatusPageEventList, StatusPageEventView, StatusPageHistoryPage, StatusPageHistoryQuery,
        StatusPageIncident, StatusPageIncidentKind, StatusPageIncidentQueryRepository,
        StatusPageIncidentUpdate,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[async_trait]
impl StatusPageIncidentQueryRepository for PgStatusPageRepository {
    async fn get_status_incident(
        &self,
        org_id: &Id,
        page_id: &Id,
        incident_id: &Id,
    ) -> Result<StatusPageIncident> {
        fetch_incident(&self.pool, org_id, page_id, incident_id).await
    }

    async fn list_status_incidents(
        &self,
        org_id: &Id,
        page_id: &Id,
        since: TimestampMicros,
    ) -> Result<Vec<StatusPageIncident>> {
        let sql = format!(
            "SELECT {INCIDENT_COLS} FROM status_page_incidents
             WHERE org_id = $1 AND status_page_id = $2
               AND publication_state = 'published'
               AND (started_at_micros >= $3 OR ended_at_micros >= $3
                    OR ended_at_micros IS NULL)
             ORDER BY (ended_at_micros IS NULL) DESC, started_at_micros DESC, id DESC
             LIMIT 200"
        );
        let incidents = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&page_id.0)
            .bind(since.0)
            .fetch_all(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .into_iter()
            .map(row_to_incident)
            .collect::<Result<Vec<_>>>()?;
        hydrate_incidents(&self.pool, org_id, page_id, incidents).await
    }

    async fn list_status_events(
        &self,
        org_id: &Id,
        page_id: &Id,
        kind: StatusPageIncidentKind,
        view: StatusPageEventView,
        limit: u32,
    ) -> Result<StatusPageEventList> {
        validate_view_kind(kind, view)?;
        let predicate = view_predicate(view);
        let count_sql = format!(
            "SELECT count(*) FROM status_page_incidents
             WHERE org_id = $1 AND status_page_id = $2 AND kind = $3 AND {predicate}"
        );
        let total: i64 = sqlx::query_scalar(&count_sql)
            .bind(&org_id.0)
            .bind(&page_id.0)
            .bind(kind.as_str())
            .fetch_one(&self.pool)
            .await
            .map_err(super::sqlx_err)?;
        let order = if matches!(view, StatusPageEventView::Upcoming) {
            "started_at_micros, id"
        } else if matches!(
            view,
            StatusPageEventView::Resolved | StatusPageEventView::Completed
        ) {
            "ended_at_micros DESC, id DESC"
        } else {
            "updated_at_micros DESC, id DESC"
        };
        let sql = format!(
            "SELECT {INCIDENT_COLS} FROM status_page_incidents
             WHERE org_id = $1 AND status_page_id = $2 AND kind = $3 AND {predicate}
             ORDER BY {order} LIMIT $4"
        );
        let incidents = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&page_id.0)
            .bind(kind.as_str())
            .bind(i64::from(limit.min(200)))
            .fetch_all(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .into_iter()
            .map(row_to_incident)
            .collect::<Result<Vec<_>>>()?;
        Ok(StatusPageEventList {
            kind,
            view,
            items: hydrate_incidents(&self.pool, org_id, page_id, incidents).await?,
            total: u64::try_from(total.max(0)).unwrap_or(u64::MAX),
        })
    }

    async fn query_status_history(
        &self,
        org_id: &Id,
        page_id: &Id,
        query: &StatusPageHistoryQuery,
    ) -> Result<StatusPageHistoryPage> {
        let search = query.search.as_deref().map(escape_like);
        let status = query.status.map(|value| value.as_str());
        let kind = query.kind.map(|value| value.as_str());
        let filter = "event.org_id = $1 AND event.status_page_id = $2
            AND event.publication_state = 'published' AND event.ended_at_micros IS NOT NULL
            AND ($3::VARCHAR IS NULL OR event.kind = $3)
            AND ($4::VARCHAR IS NULL OR event.status = $4)
            AND ($5::VARCHAR IS NULL OR EXISTS (
                SELECT 1 FROM status_page_incident_components link
                WHERE link.org_id = event.org_id
                  AND link.status_page_id = event.status_page_id
                  AND link.incident_id = event.id AND link.component_id = $5
            ))
            AND ($6::BIGINT IS NULL OR event.ended_at_micros >= $6)
            AND ($7::BIGINT IS NULL OR event.ended_at_micros <= $7)
            AND ($8::TEXT IS NULL OR event.title ILIKE ('%' || $8 || '%') ESCAPE '\\'
                 OR EXISTS (
                    SELECT 1 FROM status_page_incident_updates update_row
                    WHERE update_row.org_id = event.org_id
                      AND update_row.status_page_id = event.status_page_id
                      AND update_row.incident_id = event.id
                      AND update_row.message ILIKE ('%' || $8 || '%') ESCAPE '\\'
                 ))";
        let count_sql = format!("SELECT count(*) FROM status_page_incidents event WHERE {filter}");
        let total: i64 = sqlx::query_scalar(&count_sql)
            .bind(&org_id.0)
            .bind(&page_id.0)
            .bind(kind)
            .bind(status)
            .bind(query.component_id.as_ref().map(Id::as_str))
            .bind(query.from.map(|value| value.0))
            .bind(query.to.map(|value| value.0))
            .bind(search.as_deref())
            .fetch_one(&self.pool)
            .await
            .map_err(super::sqlx_err)?;
        let offset =
            u64::from(query.page.saturating_sub(1)).saturating_mul(u64::from(query.per_page));
        let sql = format!(
            "SELECT {INCIDENT_COLS} FROM status_page_incidents event
             WHERE {filter}
             ORDER BY event.ended_at_micros DESC, event.id DESC
             LIMIT $9 OFFSET $10"
        );
        let rows = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&page_id.0)
            .bind(kind)
            .bind(status)
            .bind(query.component_id.as_ref().map(Id::as_str))
            .bind(query.from.map(|value| value.0))
            .bind(query.to.map(|value| value.0))
            .bind(search.as_deref())
            .bind(i64::from(query.per_page))
            .bind(i64::try_from(offset).unwrap_or(i64::MAX))
            .fetch_all(&self.pool)
            .await
            .map_err(super::sqlx_err)?;
        let incidents = rows
            .into_iter()
            .map(row_to_incident)
            .collect::<Result<Vec<_>>>()?;
        Ok(StatusPageHistoryPage {
            items: hydrate_incidents(&self.pool, org_id, page_id, incidents).await?,
            page: query.page,
            per_page: query.per_page,
            total: u64::try_from(total.max(0)).unwrap_or(u64::MAX),
        })
    }
}

pub(super) async fn fetch_incident(
    pool: &PgPool,
    org_id: &Id,
    page_id: &Id,
    incident_id: &Id,
) -> Result<StatusPageIncident> {
    let sql = format!(
        "SELECT {INCIDENT_COLS} FROM status_page_incidents
         WHERE org_id = $1 AND status_page_id = $2 AND id = $3"
    );
    let incident = sqlx::query(&sql)
        .bind(&org_id.0)
        .bind(&page_id.0)
        .bind(&incident_id.0)
        .fetch_optional(pool)
        .await
        .map_err(super::sqlx_err)?
        .map(row_to_incident)
        .transpose()?
        .ok_or_else(|| Error::not_found("status-page event not found"))?;
    hydrate_incidents(pool, org_id, page_id, vec![incident])
        .await?
        .pop()
        .ok_or_else(|| Error::not_found("status-page event not found"))
}

async fn hydrate_incidents(
    pool: &PgPool,
    org_id: &Id,
    page_id: &Id,
    mut incidents: Vec<StatusPageIncident>,
) -> Result<Vec<StatusPageIncident>> {
    if incidents.is_empty() {
        return Ok(incidents);
    }
    let selected_ids: Vec<String> = incidents.iter().map(|event| event.id.0.clone()).collect();
    let component_rows = sqlx::query(
        "SELECT incident_id, component_id FROM status_page_incident_components
         WHERE org_id = $1 AND status_page_id = $2 AND incident_id = ANY($3::TEXT[])
         ORDER BY incident_id, component_id",
    )
    .bind(&org_id.0)
    .bind(&page_id.0)
    .bind(&selected_ids)
    .fetch_all(pool)
    .await
    .map_err(super::sqlx_err)?;
    let mut component_ids: HashMap<String, Vec<Id>> = HashMap::new();
    for row in component_rows {
        let owner: String = row.try_get("incident_id").map_err(super::sqlx_err)?;
        let component: String = row.try_get("component_id").map_err(super::sqlx_err)?;
        component_ids.entry(owner).or_default().push(Id(component));
    }
    let update_rows = sqlx::query(
        "SELECT id, org_id, status_page_id, incident_id, status, message, created_at_micros
         FROM (
             SELECT update_row.*,
                    ROW_NUMBER() OVER (
                        PARTITION BY update_row.incident_id
                        ORDER BY update_row.created_at_micros DESC, update_row.id DESC
                    ) AS update_rank
             FROM status_page_incident_updates update_row
             WHERE update_row.org_id = $1 AND update_row.status_page_id = $2
               AND update_row.incident_id = ANY($3::TEXT[])
         ) ranked
         WHERE update_rank <= 200
         ORDER BY created_at_micros DESC, id DESC",
    )
    .bind(&org_id.0)
    .bind(&page_id.0)
    .bind(&selected_ids)
    .fetch_all(pool)
    .await
    .map_err(super::sqlx_err)?;
    let mut updates: HashMap<String, Vec<StatusPageIncidentUpdate>> = HashMap::new();
    for row in update_rows {
        let update = row_to_update(row)?;
        updates
            .entry(update.incident_id.0.clone())
            .or_default()
            .push(update);
    }
    for incident in &mut incidents {
        incident.component_ids = component_ids
            .remove(incident.id.as_str())
            .unwrap_or_default();
        incident.updates = updates.remove(incident.id.as_str()).unwrap_or_default();
    }
    Ok(incidents)
}

fn validate_view_kind(kind: StatusPageIncidentKind, view: StatusPageEventView) -> Result<()> {
    let valid = match kind {
        StatusPageIncidentKind::Incident => matches!(
            view,
            StatusPageEventView::Current
                | StatusPageEventView::Draft
                | StatusPageEventView::Resolved
        ),
        StatusPageIncidentKind::Maintenance => matches!(
            view,
            StatusPageEventView::Upcoming
                | StatusPageEventView::InProgress
                | StatusPageEventView::Completed
                | StatusPageEventView::Draft
        ),
    };
    valid
        .then_some(())
        .ok_or_else(|| Error::invalid("event view is invalid for this event kind"))
}

const fn view_predicate(view: StatusPageEventView) -> &'static str {
    match view {
        StatusPageEventView::Current => "publication_state = 'published' AND status <> 'resolved'",
        StatusPageEventView::Draft => "publication_state = 'draft'",
        StatusPageEventView::Resolved => "publication_state = 'published' AND status = 'resolved'",
        StatusPageEventView::Upcoming => "publication_state = 'published' AND status = 'scheduled'",
        StatusPageEventView::InProgress => {
            "publication_state = 'published' AND status = 'in_progress'"
        }
        StatusPageEventView::Completed => {
            "publication_state = 'published' AND status IN ('completed', 'cancelled')"
        }
    }
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}
