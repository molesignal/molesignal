// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The components that depend on something, and the health worked out for
//! them.

use async_trait::async_trait;
use sqlx::{Row, types::Json};

use super::{
    PgStatusPageRepository,
    codec::{COMPONENT_COLS, row_to_component},
};
use crate::{
    domain::status_page::{
        DependencyHealth, HealthWrite, StatusPageComponent, StatusPageDependencyRepository,
    },
    shared::{Result, ids::Id},
};

#[async_trait]
impl StatusPageDependencyRepository for PgStatusPageRepository {
    async fn list_dependency_organizations(&self) -> Result<Vec<Id>> {
        let rows = sqlx::query(
            "SELECT DISTINCT component.org_id
             FROM status_page_components component
             JOIN status_pages page
               ON page.org_id = component.org_id AND page.id = component.status_page_id
             WHERE component.dependencies <> '[]'::JSONB
               AND component.lifecycle = 'active'
               AND page.lifecycle = 'active'
             ORDER BY component.org_id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(super::sqlx_err)?;

        rows.into_iter()
            .map(|row| {
                row.try_get::<String, _>("org_id")
                    .map(Id)
                    .map_err(super::sqlx_err)
            })
            .collect()
    }

    async fn list_dependent_components(&self, org_id: &Id) -> Result<Vec<StatusPageComponent>> {
        let columns = COMPONENT_COLS
            .split(',')
            .map(|column| format!("component.{}", column.trim()))
            .collect::<Vec<_>>()
            .join(", ");

        let sql = format!(
            "SELECT {columns}
             FROM status_page_components component
             JOIN status_pages page
               ON page.org_id = component.org_id AND page.id = component.status_page_id
             WHERE component.org_id = $1
               AND component.dependencies <> '[]'::JSONB
               AND component.lifecycle = 'active'
               AND page.lifecycle = 'active'
             ORDER BY page.name, component.position, component.name, component.id
             LIMIT 5000"
        );

        sqlx::query(&sql)
            .bind(&org_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .into_iter()
            .map(row_to_component)
            .collect()
    }

    async fn record_dependency_health(
        &self,
        component: &StatusPageComponent,
        after: &DependencyHealth,
    ) -> Result<HealthWrite> {
        let before = component.dependency_health.as_ref();
        let changed = after.changed_from(before);

        // Whatever is written, it is written only over the dependencies and the
        // status this evaluation started from.
        let result = sqlx::query(
            "UPDATE status_page_components
             SET dependency_status = $6, dependency_causes = $7,
                 dependency_changed_at = $8, dependency_evaluated_at = $9
             WHERE org_id = $1 AND id = $2
               AND dependencies = $3
               AND dependency_status IS NOT DISTINCT FROM $4
               AND dependency_changed_at IS NOT DISTINCT FROM $5",
        )
        .bind(&component.org_id.0)
        .bind(&component.id.0)
        .bind(Json(&component.dependencies))
        .bind(before.map(|health| health.status.as_str()))
        .bind(before.map(|health| health.changed_at))
        .bind(after.status.as_str())
        .bind(Json(&after.causes))
        .bind(after.changed_at)
        .bind(after.evaluated_at)
        .execute(&self.pool)
        .await
        .map_err(super::sqlx_err)?;

        Ok(match (result.rows_affected(), changed) {
            (0, _) => HealthWrite::Lost,
            (_, true) => HealthWrite::Changed,
            (_, false) => HealthWrite::Unchanged,
        })
    }
}
