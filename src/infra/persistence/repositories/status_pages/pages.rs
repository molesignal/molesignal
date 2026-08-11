// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;

use super::{
    PgStatusPageRepository,
    codec::{PAGE_COLS, row_to_page},
};
use crate::{
    domain::status_page::{StatusPage, StatusPagePageRepository, StatusPageVisibility},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const PAGE_JOIN_COLS: &str = "page.id AS id, page.org_id AS org_id, page.name AS name,
    page.slug AS slug, page.logo_url AS logo_url, page.brand_color AS brand_color,
    page.custom_domain AS custom_domain, page.timezone AS timezone,
    page.language AS language, page.languages AS languages, page.history_days AS history_days,
    page.visibility AS visibility, page.delivery_retention_days AS delivery_retention_days,
    page.private_session_days AS private_session_days, page.lifecycle AS lifecycle,
    page.archived_at_micros AS archived_at_micros,
    page.purge_after_micros AS purge_after_micros,
    page.created_at_micros AS created_at_micros,
    page.updated_at_micros AS updated_at_micros";

#[async_trait]
impl StatusPagePageRepository for PgStatusPageRepository {
    async fn create_page(&self, page: StatusPage) -> Result<StatusPage> {
        let sql = format!(
            "INSERT INTO status_pages
                (id, org_id, name, slug, logo_url, brand_color, custom_domain, timezone,
                 language, languages, history_days, delivery_retention_days,
                 private_session_days, visibility, lifecycle, archived_at_micros,
                 purge_after_micros, created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
                     $14, $15, $16, $17, $18, $19)
             RETURNING {PAGE_COLS}"
        );
        let row = sqlx::query(&sql)
            .bind(&page.id.0)
            .bind(&page.org_id.0)
            .bind(&page.name)
            .bind(&page.slug)
            .bind(&page.logo_url)
            .bind(&page.brand_color)
            .bind(&page.custom_domain)
            .bind(&page.timezone)
            .bind(&page.language)
            .bind(&page.languages)
            .bind(page.history_days)
            .bind(page.delivery_retention_days)
            .bind(page.private_session_days)
            .bind(page.visibility.as_str())
            .bind(page.lifecycle.as_str())
            .bind(page.archived_at.map(|at| at.0))
            .bind(page.purge_after.map(|at| at.0))
            .bind(page.created_at.0)
            .bind(page.updated_at.0)
            .fetch_one(&self.pool)
            .await
            .map_err(super::sqlx_err)?;
        row_to_page(row)
    }

    async fn update_page(&self, page: StatusPage) -> Result<StatusPage> {
        let mut transaction = sqlx::begin(&self.pool).await.map_err(super::sqlx_err)?;
        let previous_visibility: String = sqlx::query_scalar(
            "SELECT visibility FROM status_pages
             WHERE org_id = $1 AND id = $2 AND lifecycle = 'active' FOR UPDATE",
        )
        .bind(&page.org_id.0)
        .bind(&page.id.0)
        .fetch_one(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;
        let sql = format!(
            "UPDATE status_pages SET
                 name = $3, slug = $4, logo_url = $5, brand_color = $6,
                 custom_domain = $7, timezone = $8, language = $9, languages = $10,
                 history_days = $11, delivery_retention_days = $12,
                 private_session_days = $13, visibility = $14, updated_at_micros = $15
             WHERE org_id = $1 AND id = $2 AND lifecycle = 'active'
             RETURNING {PAGE_COLS}"
        );
        let row = sqlx::query(&sql)
            .bind(&page.org_id.0)
            .bind(&page.id.0)
            .bind(&page.name)
            .bind(&page.slug)
            .bind(&page.logo_url)
            .bind(&page.brand_color)
            .bind(&page.custom_domain)
            .bind(&page.timezone)
            .bind(&page.language)
            .bind(&page.languages)
            .bind(page.history_days)
            .bind(page.delivery_retention_days)
            .bind(page.private_session_days)
            .bind(page.visibility.as_str())
            .bind(page.updated_at.0)
            .fetch_one(&mut *transaction)
            .await
            .map_err(super::sqlx_err)?;
        let updated = row_to_page(row)?;
        if previous_visibility != updated.visibility.as_str() {
            sqlx::query(
                "DELETE FROM status_page_magic_links
                 WHERE org_id = $1 AND status_page_id = $2",
            )
            .bind(&updated.org_id.0)
            .bind(&updated.id.0)
            .execute(&mut *transaction)
            .await
            .map_err(super::sqlx_err)?;
            sqlx::query(
                "UPDATE status_page_access_sessions SET revoked_at_micros = $3
                 WHERE org_id = $1 AND status_page_id = $2
                   AND revoked_at_micros IS NULL",
            )
            .bind(&updated.org_id.0)
            .bind(&updated.id.0)
            .bind(updated.updated_at.0)
            .execute(&mut *transaction)
            .await
            .map_err(super::sqlx_err)?;
        }
        if updated.visibility == StatusPageVisibility::Private {
            sqlx::query(
                "UPDATE status_page_subscribers
                 SET status = 'unsubscribed', unsubscribed_at_micros = $3,
                     updated_at_micros = $3
                 WHERE org_id = $1 AND status_page_id = $2 AND channel = 'webhook'
                   AND status <> 'unsubscribed'",
            )
            .bind(&updated.org_id.0)
            .bind(&updated.id.0)
            .bind(updated.updated_at.0)
            .execute(&mut *transaction)
            .await
            .map_err(super::sqlx_err)?;
            sqlx::query(
                "UPDATE status_page_notification_deliveries delivery
                 SET status = 'failed', claimed_at_micros = NULL,
                     last_error = 'webhook disabled for private status page',
                     updated_at_micros = $3
                 FROM status_page_subscribers subscriber
                 WHERE delivery.org_id = $1 AND delivery.status_page_id = $2
                   AND delivery.subscriber_id = subscriber.id
                   AND subscriber.org_id = delivery.org_id
                   AND subscriber.status_page_id = delivery.status_page_id
                   AND subscriber.channel = 'webhook'
                   AND delivery.status IN ('pending', 'processing')",
            )
            .bind(&updated.org_id.0)
            .bind(&updated.id.0)
            .bind(updated.updated_at.0)
            .execute(&mut *transaction)
            .await
            .map_err(super::sqlx_err)?;
        }
        transaction.commit().await.map_err(super::sqlx_err)?;
        Ok(updated)
    }

    async fn update_page_logo(
        &self,
        org_id: &Id,
        page_id: &Id,
        logo_url: Option<String>,
        updated_at: TimestampMicros,
    ) -> Result<StatusPage> {
        let sql = format!(
            "UPDATE status_pages SET logo_url = $3, updated_at_micros = $4
             WHERE org_id = $1 AND id = $2 AND lifecycle = 'active'
             RETURNING {PAGE_COLS}"
        );
        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&page_id.0)
            .bind(logo_url)
            .bind(updated_at.0)
            .fetch_one(&self.pool)
            .await
            .map_err(super::sqlx_err)?;
        row_to_page(row)
    }

    async fn delete_page(&self, org_id: &Id, page_id: &Id) -> Result<()> {
        let mut transaction = sqlx::begin(&self.pool).await.map_err(super::sqlx_err)?;
        delete_page_and_domain(&mut transaction, org_id, page_id, true).await?;
        transaction.commit().await.map_err(super::sqlx_err)
    }

    async fn archive_page(
        &self,
        org_id: &Id,
        page_id: &Id,
        archived_at: TimestampMicros,
        purge_after: TimestampMicros,
    ) -> Result<StatusPage> {
        let mut transaction = sqlx::begin(&self.pool).await.map_err(super::sqlx_err)?;
        let sql = format!(
            "UPDATE status_pages SET lifecycle = 'archived', archived_at_micros = $3,
                 purge_after_micros = $4, updated_at_micros = $3
             WHERE org_id = $1 AND id = $2 AND lifecycle = 'active'
             RETURNING {PAGE_COLS}"
        );
        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&page_id.0)
            .bind(archived_at.0)
            .bind(purge_after.0)
            .fetch_one(&mut *transaction)
            .await
            .map_err(super::sqlx_err)?;
        sqlx::query(
            "UPDATE status_page_notification_deliveries
             SET status = 'failed', claimed_at_micros = NULL,
                 last_error = 'status page archived', updated_at_micros = $3
             WHERE org_id = $1 AND status_page_id = $2
               AND status IN ('pending', 'processing')",
        )
        .bind(&org_id.0)
        .bind(&page_id.0)
        .bind(archived_at.0)
        .execute(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;
        let page = row_to_page(row)?;
        transaction.commit().await.map_err(super::sqlx_err)?;
        Ok(page)
    }

    async fn restore_page(
        &self,
        org_id: &Id,
        page_id: &Id,
        updated_at: TimestampMicros,
    ) -> Result<StatusPage> {
        let sql = format!(
            "UPDATE status_pages SET lifecycle = 'active', archived_at_micros = NULL,
                 purge_after_micros = NULL, updated_at_micros = $3
             WHERE org_id = $1 AND id = $2 AND lifecycle = 'archived'
               AND purge_after_micros > $3
             RETURNING {PAGE_COLS}"
        );
        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&page_id.0)
            .bind(updated_at.0)
            .fetch_one(&self.pool)
            .await
            .map_err(super::sqlx_err)?;
        row_to_page(row)
    }

    async fn purge_archived_pages(
        &self,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<Vec<StatusPage>> {
        let mut transaction = sqlx::begin(&self.pool).await.map_err(super::sqlx_err)?;
        let sql = format!(
            "SELECT {PAGE_COLS} FROM status_pages
             WHERE lifecycle = 'archived' AND purge_after_micros <= $1
             ORDER BY purge_after_micros, id
             LIMIT $2 FOR UPDATE SKIP LOCKED"
        );
        let pages = sqlx::query(&sql)
            .bind(now.0)
            .bind(i64::from(limit.min(100)))
            .fetch_all(&mut *transaction)
            .await
            .map_err(super::sqlx_err)?
            .into_iter()
            .map(row_to_page)
            .collect::<Result<Vec<_>>>()?;
        for page in &pages {
            delete_page_and_domain(&mut transaction, &page.org_id, &page.id, false).await?;
        }
        transaction.commit().await.map_err(super::sqlx_err)?;
        Ok(pages)
    }

    async fn get_page(&self, org_id: &Id, page_id: &Id) -> Result<StatusPage> {
        let sql = format!("SELECT {PAGE_COLS} FROM status_pages WHERE org_id = $1 AND id = $2");
        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&page_id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(super::sqlx_err)?;
        row_to_page(row)
    }

    async fn get_page_by_slug(&self, slug: &str) -> Result<Option<StatusPage>> {
        let sql = format!(
            "SELECT {PAGE_COLS} FROM status_pages
             WHERE slug = $1 AND lifecycle = 'active'"
        );
        sqlx::query(&sql)
            .bind(slug)
            .fetch_optional(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .map(row_to_page)
            .transpose()
    }

    async fn get_public_page_by_slug(&self, slug: &str) -> Result<Option<StatusPage>> {
        let sql = format!(
            "SELECT {PAGE_COLS} FROM status_pages
             WHERE slug = $1 AND visibility = 'public' AND lifecycle = 'active'"
        );
        sqlx::query(&sql)
            .bind(slug)
            .fetch_optional(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .map(row_to_page)
            .transpose()
    }

    async fn get_page_by_custom_domain(&self, domain: &str) -> Result<Option<StatusPage>> {
        let sql = format!(
            "SELECT {PAGE_JOIN_COLS}
             FROM status_pages page
             JOIN status_page_domain_configs config
               ON config.org_id = page.org_id AND config.status_page_id = page.id
             JOIN domains domain ON domain.org_id = config.org_id AND domain.id = config.domain_id
             WHERE config.hostname = $1 AND page.lifecycle = 'active'
               AND domain.state = 'active' AND config.routing_valid
             LIMIT 1"
        );
        sqlx::query(&sql)
            .bind(domain)
            .fetch_optional(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .map(row_to_page)
            .transpose()
    }

    async fn list_pages(&self, org_id: &Id) -> Result<Vec<StatusPage>> {
        let sql = format!(
            "SELECT {PAGE_COLS} FROM status_pages
             WHERE org_id = $1
             ORDER BY (lifecycle = 'active') DESC, updated_at_micros DESC, name, id
             LIMIT 200"
        );
        sqlx::query(&sql)
            .bind(&org_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .into_iter()
            .map(row_to_page)
            .collect()
    }
}

async fn delete_page_and_domain(
    transaction: &mut sqlx::PgConnection,
    org_id: &Id,
    page_id: &Id,
    require_archived: bool,
) -> Result<()> {
    let domain_id: Option<String> = sqlx::query_scalar(
        "SELECT domain_id FROM status_page_domain_configs
         WHERE org_id = $1 AND status_page_id = $2",
    )
    .bind(&org_id.0)
    .bind(&page_id.0)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(super::sqlx_err)?
    .flatten();
    let result = sqlx::query(
        "DELETE FROM status_pages
         WHERE org_id = $1 AND id = $2 AND (NOT $3 OR lifecycle = 'archived')",
    )
    .bind(&org_id.0)
    .bind(&page_id.0)
    .bind(require_archived)
    .execute(&mut *transaction)
    .await
    .map_err(super::sqlx_err)?;
    if result.rows_affected() == 0 {
        return Err(Error::not_found("archived status page not found"));
    }
    if let Some(domain_id) = domain_id {
        sqlx::query("DELETE FROM domains WHERE org_id = $1 AND id = $2")
            .bind(&org_id.0)
            .bind(domain_id)
            .execute(&mut *transaction)
            .await
            .map_err(super::sqlx_err)?;
    }
    Ok(())
}
