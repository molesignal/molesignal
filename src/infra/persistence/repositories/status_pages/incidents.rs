// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;

use super::{PgStatusPageRepository, event_queries::fetch_incident, touch_page};
use crate::{
    domain::status_page::{
        PublicIncidentStatus, StatusPageIncident, StatusPageIncidentUpdate,
        StatusPageIncidentWriteRepository, StatusPagePublicationState,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[async_trait]
impl StatusPageIncidentWriteRepository for PgStatusPageRepository {
    async fn create_status_incident(
        &self,
        incident: StatusPageIncident,
    ) -> Result<StatusPageIncident> {
        match incident.publication_state {
            StatusPagePublicationState::Draft if !incident.updates.is_empty() => {
                return Err(Error::invalid(
                    "a draft event cannot have published updates",
                ));
            }
            StatusPagePublicationState::Published if incident.updates.len() != 1 => {
                return Err(Error::invalid(
                    "a published status-page event requires exactly one initial update",
                ));
            }
            _ => {}
        }
        let mut transaction = sqlx::begin(&self.pool).await.map_err(super::sqlx_err)?;
        insert_incident(&mut transaction, &incident).await?;
        replace_component_links(&mut transaction, &incident).await?;
        if let Some(initial_update) = incident.updates.first() {
            insert_update(&mut transaction, initial_update)
                .await
                .map_err(super::sqlx_err)?;
            touch_page(
                &mut transaction,
                &incident.org_id,
                &incident.status_page_id,
                incident.updated_at,
            )
            .await?;
        }
        transaction.commit().await.map_err(super::sqlx_err)?;
        fetch_incident(
            &self.pool,
            &incident.org_id,
            &incident.status_page_id,
            &incident.id,
        )
        .await
    }

    async fn append_status_update(
        &self,
        update: StatusPageIncidentUpdate,
        expected_status: PublicIncidentStatus,
        ended_at: Option<TimestampMicros>,
        updated_at: TimestampMicros,
    ) -> Result<StatusPageIncident> {
        let mut transaction = sqlx::begin(&self.pool).await.map_err(super::sqlx_err)?;
        let result = sqlx::query(
            "UPDATE status_page_incidents
             SET status = $5, ended_at_micros = $6, updated_at_micros = $7
             WHERE org_id = $1 AND status_page_id = $2 AND id = $3 AND status = $4
               AND publication_state = 'published'",
        )
        .bind(&update.org_id.0)
        .bind(&update.status_page_id.0)
        .bind(&update.incident_id.0)
        .bind(expected_status.as_str())
        .bind(update.status.as_str())
        .bind(ended_at.map(|at| at.0))
        .bind(updated_at.0)
        .execute(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;
        if result.rows_affected() == 0 {
            return Err(Error::conflict(
                "status-page event changed while the update was being published",
            ));
        }
        insert_update(&mut transaction, &update)
            .await
            .map_err(super::sqlx_err)?;
        touch_page(
            &mut transaction,
            &update.org_id,
            &update.status_page_id,
            updated_at,
        )
        .await?;
        transaction.commit().await.map_err(super::sqlx_err)?;
        fetch_incident(
            &self.pool,
            &update.org_id,
            &update.status_page_id,
            &update.incident_id,
        )
        .await
    }

    async fn update_status_incident_draft(
        &self,
        incident: StatusPageIncident,
    ) -> Result<StatusPageIncident> {
        let mut transaction = sqlx::begin(&self.pool).await.map_err(super::sqlx_err)?;
        let result = sqlx::query(
            "UPDATE status_page_incidents SET
                 source_incident_id = $4, kind = $5, title = $6, impact = $7,
                 status = $8, draft_message = $9, started_at_micros = $10,
                 updated_at_micros = $11
             WHERE org_id = $1 AND status_page_id = $2 AND id = $3
               AND publication_state = 'draft'",
        )
        .bind(&incident.org_id.0)
        .bind(&incident.status_page_id.0)
        .bind(&incident.id.0)
        .bind(incident.source_incident_id.as_ref().map(Id::as_str))
        .bind(incident.kind.as_str())
        .bind(&incident.title)
        .bind(incident.impact.as_str())
        .bind(incident.status.as_str())
        .bind(&incident.draft_message)
        .bind(incident.started_at.0)
        .bind(incident.updated_at.0)
        .execute(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;
        if result.rows_affected() == 0 {
            return Err(Error::conflict("draft event is no longer editable"));
        }
        replace_component_links(&mut transaction, &incident).await?;
        transaction.commit().await.map_err(super::sqlx_err)?;
        fetch_incident(
            &self.pool,
            &incident.org_id,
            &incident.status_page_id,
            &incident.id,
        )
        .await
    }

    async fn publish_status_incident(
        &self,
        incident: StatusPageIncident,
        initial_update: StatusPageIncidentUpdate,
    ) -> Result<StatusPageIncident> {
        let published_at = incident
            .published_at
            .ok_or_else(|| Error::invalid("published_at is required"))?;
        let mut transaction = sqlx::begin(&self.pool).await.map_err(super::sqlx_err)?;
        let result = sqlx::query(
            "UPDATE status_page_incidents SET
                 source_incident_id = $4, kind = $5, title = $6, impact = $7,
                 status = $8, publication_state = 'published', started_at_micros = $9,
                 published_at_micros = $10, draft_message = NULL, updated_at_micros = $10
             WHERE org_id = $1 AND status_page_id = $2 AND id = $3
               AND publication_state = 'draft'",
        )
        .bind(&incident.org_id.0)
        .bind(&incident.status_page_id.0)
        .bind(&incident.id.0)
        .bind(incident.source_incident_id.as_ref().map(Id::as_str))
        .bind(incident.kind.as_str())
        .bind(&incident.title)
        .bind(incident.impact.as_str())
        .bind(incident.status.as_str())
        .bind(incident.started_at.0)
        .bind(published_at.0)
        .execute(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;
        if result.rows_affected() == 0 {
            return Err(Error::conflict("draft event is no longer publishable"));
        }
        replace_component_links(&mut transaction, &incident).await?;
        insert_update(&mut transaction, &initial_update)
            .await
            .map_err(super::sqlx_err)?;
        touch_page(
            &mut transaction,
            &incident.org_id,
            &incident.status_page_id,
            published_at,
        )
        .await?;
        transaction.commit().await.map_err(super::sqlx_err)?;
        fetch_incident(
            &self.pool,
            &incident.org_id,
            &incident.status_page_id,
            &incident.id,
        )
        .await
    }

    async fn update_scheduled_maintenance(
        &self,
        incident: StatusPageIncident,
        update: StatusPageIncidentUpdate,
    ) -> Result<StatusPageIncident> {
        let mut transaction = sqlx::begin(&self.pool).await.map_err(super::sqlx_err)?;
        let result = sqlx::query(
            "UPDATE status_page_incidents SET
                 title = $4, started_at_micros = $5, updated_at_micros = $6
             WHERE org_id = $1 AND status_page_id = $2 AND id = $3
               AND kind = 'maintenance' AND publication_state = 'published'
               AND status = 'scheduled'",
        )
        .bind(&incident.org_id.0)
        .bind(&incident.status_page_id.0)
        .bind(&incident.id.0)
        .bind(&incident.title)
        .bind(incident.started_at.0)
        .bind(incident.updated_at.0)
        .execute(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;
        if result.rows_affected() != 1 {
            return Err(Error::conflict(
                "scheduled maintenance is no longer editable",
            ));
        }
        replace_component_links(&mut transaction, &incident).await?;
        insert_update(&mut transaction, &update)
            .await
            .map_err(super::sqlx_err)?;
        touch_page(
            &mut transaction,
            &incident.org_id,
            &incident.status_page_id,
            incident.updated_at,
        )
        .await?;
        transaction.commit().await.map_err(super::sqlx_err)?;
        fetch_incident(
            &self.pool,
            &incident.org_id,
            &incident.status_page_id,
            &incident.id,
        )
        .await
    }
}

async fn insert_incident(
    transaction: &mut sqlx::PgConnection,
    incident: &StatusPageIncident,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO status_page_incidents
            (id, org_id, status_page_id, source_incident_id, kind, title, impact,
             status, publication_state, draft_message, started_at_micros, ended_at_micros,
             published_at_micros, created_at_micros, updated_at_micros)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
    )
    .bind(&incident.id.0)
    .bind(&incident.org_id.0)
    .bind(&incident.status_page_id.0)
    .bind(incident.source_incident_id.as_ref().map(Id::as_str))
    .bind(incident.kind.as_str())
    .bind(&incident.title)
    .bind(incident.impact.as_str())
    .bind(incident.status.as_str())
    .bind(incident.publication_state.as_str())
    .bind(&incident.draft_message)
    .bind(incident.started_at.0)
    .bind(incident.ended_at.map(|at| at.0))
    .bind(incident.published_at.map(|at| at.0))
    .bind(incident.created_at.0)
    .bind(incident.updated_at.0)
    .execute(transaction)
    .await
    .map_err(super::sqlx_err)?;
    Ok(())
}

async fn replace_component_links(
    transaction: &mut sqlx::PgConnection,
    incident: &StatusPageIncident,
) -> Result<()> {
    sqlx::query(
        "DELETE FROM status_page_incident_components
         WHERE org_id = $1 AND status_page_id = $2 AND incident_id = $3",
    )
    .bind(&incident.org_id.0)
    .bind(&incident.status_page_id.0)
    .bind(&incident.id.0)
    .execute(&mut *transaction)
    .await
    .map_err(super::sqlx_err)?;
    for component_id in &incident.component_ids {
        sqlx::query(
            "INSERT INTO status_page_incident_components
                (org_id, status_page_id, incident_id, component_id)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(&incident.org_id.0)
        .bind(&incident.status_page_id.0)
        .bind(&incident.id.0)
        .bind(&component_id.0)
        .execute(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;
    }
    Ok(())
}

pub(super) async fn insert_update(
    transaction: &mut sqlx::PgConnection,
    update: &StatusPageIncidentUpdate,
) -> std::result::Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO status_page_incident_updates
            (id, org_id, status_page_id, incident_id, status, message, created_at_micros)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(&update.id.0)
    .bind(&update.org_id.0)
    .bind(&update.status_page_id.0)
    .bind(&update.incident_id.0)
    .bind(update.status.as_str())
    .bind(&update.message)
    .bind(update.created_at.0)
    .execute(transaction)
    .await
}
