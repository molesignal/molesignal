// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::types::Json;

use super::{
    PgStatusPageRepository,
    codec::{
        COMPONENT_COLS, COMPONENT_STATUS_EVENT_COLS, row_to_component,
        row_to_component_status_event,
    },
    touch_page,
};
use crate::{
    domain::status_page::{
        StatusPageComponent, StatusPageComponentRepository, StatusPageComponentStatusEvent,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[async_trait]
impl StatusPageComponentRepository for PgStatusPageRepository {
    async fn create_component(
        &self,
        component: StatusPageComponent,
        initial_event: StatusPageComponentStatusEvent,
    ) -> Result<StatusPageComponent> {
        let mut transaction = sqlx::begin(&self.pool).await.map_err(super::sqlx_err)?;

        sqlx::query(
            "INSERT INTO status_page_components
                (id, org_id, status_page_id, name, description, status, visibility,
                 lifecycle, position, archived_at, created_at, updated_at,
                 kind, group_id, dependencies, dependency_mode, dependency_notify)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)",
        )
        .bind(&component.id.0)
        .bind(&component.org_id.0)
        .bind(&component.status_page_id.0)
        .bind(&component.name)
        .bind(&component.description)
        .bind(component.status.as_str())
        .bind(component.visibility.as_str())
        .bind(component.lifecycle.as_str())
        .bind(component.position)
        .bind(component.archived_at)
        .bind(component.created_at)
        .bind(component.updated_at)
        .bind(component.kind.as_str())
        .bind(component.group_id.as_ref().map(Id::as_str))
        .bind(Json(&component.dependencies))
        .bind(component.dependency_mode.as_str())
        .bind(component.dependency_notify)
        .execute(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;

        insert_component_status_event(&mut transaction, &initial_event)
            .await
            .map_err(super::sqlx_err)?;

        touch_page(
            &mut transaction,
            &component.org_id,
            &component.status_page_id,
            component.updated_at,
        )
        .await?;

        transaction.commit().await.map_err(super::sqlx_err)?;
        self.get_component(&component.org_id, &component.status_page_id, &component.id)
            .await
    }

    async fn update_component(
        &self,
        component: StatusPageComponent,
        expected_status: crate::domain::status_page::ComponentStatus,
        status_event: Option<StatusPageComponentStatusEvent>,
    ) -> Result<StatusPageComponent> {
        let mut transaction = sqlx::begin(&self.pool).await.map_err(super::sqlx_err)?;

        let updated = sqlx::query(
            // The health worked out from other dependencies is dropped with
            // them; the next evaluation works it out again.
            "UPDATE status_page_components SET
                 name = $4, description = $5, status = $6, visibility = $7,
                 lifecycle = $8, position = $9, archived_at = $10,
                 updated_at = $11, group_id = $13,
                 dependency_status = CASE WHEN dependencies = $14 THEN dependency_status END,
                 dependency_causes = CASE WHEN dependencies = $14
                     THEN dependency_causes ELSE '[]'::JSONB END,
                 dependency_changed_at = CASE WHEN dependencies = $14
                     THEN dependency_changed_at END,
                 dependency_evaluated_at = CASE WHEN dependencies = $14
                     THEN dependency_evaluated_at END,
                 dependencies = $14, dependency_mode = $15, dependency_notify = $16
             WHERE org_id = $1 AND status_page_id = $2 AND id = $3 AND status = $12
             RETURNING id",
        )
        .bind(&component.org_id.0)
        .bind(&component.status_page_id.0)
        .bind(&component.id.0)
        .bind(&component.name)
        .bind(&component.description)
        .bind(component.status.as_str())
        .bind(component.visibility.as_str())
        .bind(component.lifecycle.as_str())
        .bind(component.position)
        .bind(component.archived_at)
        .bind(component.updated_at)
        .bind(expected_status.as_str())
        .bind(component.group_id.as_ref().map(Id::as_str))
        .bind(Json(&component.dependencies))
        .bind(component.dependency_mode.as_str())
        .bind(component.dependency_notify)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;

        if updated.is_none() {
            return Err(Error::conflict(
                "component changed while it was being updated",
            ));
        }

        if let Some(status_event) = status_event {
            let result = sqlx::query(
                "UPDATE status_page_component_status_events
                 SET ended_at = $4
                 WHERE org_id = $1 AND status_page_id = $2 AND component_id = $3
                   AND ended_at IS NULL",
            )
            .bind(&component.org_id.0)
            .bind(&component.status_page_id.0)
            .bind(&component.id.0)
            .bind(status_event.started_at)
            .execute(&mut *transaction)
            .await
            .map_err(super::sqlx_err)?;

            if result.rows_affected() != 1 {
                return Err(Error::conflict(
                    "component status history changed while the component was being updated",
                ));
            }

            insert_component_status_event(&mut transaction, &status_event)
                .await
                .map_err(super::sqlx_err)?;
        }

        touch_page(
            &mut transaction,
            &component.org_id,
            &component.status_page_id,
            component.updated_at,
        )
        .await?;

        transaction.commit().await.map_err(super::sqlx_err)?;
        self.get_component(&component.org_id, &component.status_page_id, &component.id)
            .await
    }

    async fn delete_component(
        &self,
        org_id: &Id,
        page_id: &Id,
        component_id: &Id,
        updated_at: TimestampMicros,
    ) -> Result<()> {
        let mut transaction = sqlx::begin(&self.pool).await.map_err(super::sqlx_err)?;

        let has_history: bool = sqlx::query_scalar(
            "SELECT EXISTS (
                 SELECT 1 FROM status_page_incidents
                 WHERE org_id = $1 AND status_page_id = $2 AND $3 = ANY(component_ids)
             ) OR (
                 SELECT count(*) > 1 FROM status_page_component_status_events
                 WHERE org_id = $1 AND status_page_id = $2 AND component_id = $3
             )",
        )
        .bind(&org_id.0)
        .bind(&page_id.0)
        .bind(&component_id.0)
        .fetch_one(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;

        if has_history {
            return Err(Error::conflict(
                "component has history and must be archived instead of deleted",
            ));
        }

        // The archived components of a group that goes stay, on their own.
        sqlx::query(
            "UPDATE status_page_components SET group_id = NULL
             WHERE org_id = $1 AND status_page_id = $2 AND group_id = $3",
        )
        .bind(&org_id.0)
        .bind(&page_id.0)
        .bind(&component_id.0)
        .execute(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;

        let result = sqlx::query(
            "DELETE FROM status_page_components
             WHERE org_id = $1 AND status_page_id = $2 AND id = $3",
        )
        .bind(&org_id.0)
        .bind(&page_id.0)
        .bind(&component_id.0)
        .execute(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;

        if result.rows_affected() == 0 {
            return Err(Error::not_found("status page component not found"));
        }

        touch_page(&mut transaction, org_id, page_id, updated_at).await?;
        transaction.commit().await.map_err(super::sqlx_err)?;
        Ok(())
    }

    async fn get_component(
        &self,
        org_id: &Id,
        page_id: &Id,
        component_id: &Id,
    ) -> Result<StatusPageComponent> {
        let sql = format!(
            "SELECT {COMPONENT_COLS} FROM status_page_components
             WHERE org_id = $1 AND status_page_id = $2 AND id = $3"
        );

        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&page_id.0)
            .bind(&component_id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(super::sqlx_err)?;

        row_to_component(row)
    }

    async fn list_components(&self, org_id: &Id, page_id: &Id) -> Result<Vec<StatusPageComponent>> {
        let sql = format!(
            "SELECT {COMPONENT_COLS} FROM status_page_components
             WHERE org_id = $1 AND status_page_id = $2
             ORDER BY (lifecycle = 'active') DESC, position, name, id LIMIT 500"
        );

        sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&page_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .into_iter()
            .map(row_to_component)
            .collect()
    }

    async fn list_component_status_events(
        &self,
        org_id: &Id,
        page_id: &Id,
        since: TimestampMicros,
        until: TimestampMicros,
    ) -> Result<Vec<StatusPageComponentStatusEvent>> {
        let sql = format!(
            "SELECT {COMPONENT_STATUS_EVENT_COLS}
             FROM status_page_component_status_events
             WHERE org_id = $1 AND status_page_id = $2
               AND started_at < $4
               AND (ended_at IS NULL OR ended_at > $3)
             ORDER BY started_at DESC, id DESC LIMIT 50000"
        );

        sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&page_id.0)
            .bind(since)
            .bind(until)
            .fetch_all(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .into_iter()
            .map(row_to_component_status_event)
            .collect()
    }
}

async fn insert_component_status_event(
    transaction: &mut sqlx::PgConnection,
    event: &StatusPageComponentStatusEvent,
) -> std::result::Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO status_page_component_status_events
            (id, org_id, status_page_id, component_id, status, started_at,
             ended_at, created_at, notify_subscribers)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(&event.id.0)
    .bind(&event.org_id.0)
    .bind(&event.status_page_id.0)
    .bind(&event.component_id.0)
    .bind(event.status.as_str())
    .bind(event.started_at)
    .bind(event.ended_at)
    .bind(event.created_at)
    .bind(event.notify_subscribers)
    .execute(transaction)
    .await
}
