// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The business service a component stands for, kept on the component row.

use async_trait::async_trait;

use super::{
    PgStatusPageRepository,
    codec::{COMPONENT_COLS, row_to_component},
    touch_page,
};
use crate::{
    domain::status_page::{
        ComponentServiceLink, StatusPageComponent, StatusPageServiceLinkRepository,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[async_trait]
impl StatusPageServiceLinkRepository for PgStatusPageRepository {
    async fn set_component_service_link(
        &self,
        org_id: &Id,
        page_id: &Id,
        component_id: &Id,
        link: Option<&ComponentServiceLink>,
        updated_at: TimestampMicros,
    ) -> Result<StatusPageComponent> {
        let mut transaction = sqlx::begin(&self.pool).await.map_err(super::sqlx_err)?;

        let sql = format!(
            "UPDATE status_page_components
             SET business_service_id = $4, business_service_mode = $5,
                 business_service_notify = $6, updated_at_micros = $7
             WHERE org_id = $1 AND status_page_id = $2 AND id = $3
             RETURNING {COMPONENT_COLS}"
        );

        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&page_id.0)
            .bind(&component_id.0)
            .bind(link.map(|link| link.business_service_id.as_str()))
            .bind(link.map(|link| link.mode.as_str()))
            .bind(link.is_none_or(|link| link.notify_subscribers))
            .bind(updated_at.0)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(super::sqlx_err)?
            .ok_or_else(|| Error::not_found("status page component not found"))?;

        touch_page(&mut transaction, org_id, page_id, updated_at).await?;
        transaction.commit().await.map_err(super::sqlx_err)?;
        row_to_component(row)
    }

    async fn list_service_linked_components(
        &self,
        org_id: &Id,
        business_service_id: Option<&Id>,
    ) -> Result<Vec<StatusPageComponent>> {
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
               AND component.business_service_id IS NOT NULL
               AND ($2::VARCHAR IS NULL OR component.business_service_id = $2)
               AND page.lifecycle = 'active'
             ORDER BY page.name, component.position, component.name, component.id
             LIMIT 5000"
        );

        sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(business_service_id.map(Id::as_str))
            .fetch_all(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .into_iter()
            .map(row_to_component)
            .collect()
    }
}
