// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::Row;

use super::PgStatusPageRepository;
use crate::{
    domain::status_page::{
        StatusPageDomainCheckUpdate, StatusPageDomainConfig, StatusPageDomainRepository,
        StatusPageDomainState,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const DOMAIN_CONFIG_COLS: &str = "config.org_id, config.status_page_id, config.hostname,
    config.verification_token,
    CASE
        WHEN config.state = 'degraded' THEN 'degraded'
        WHEN domain.state = 'active' AND domain.last_error IS NOT NULL THEN 'degraded'
        WHEN domain.state = 'active' THEN 'active'
        WHEN domain.state IN ('pending', 'provisioning') THEN 'provisioning_tls'
        WHEN domain.state = 'failed' THEN 'failed'
        WHEN domain.state = 'expired' THEN 'degraded'
        ELSE config.state
    END AS effective_state,
    config.domain_id, config.routing_valid, config.last_checked_at_micros,
    COALESCE(config.last_error, domain.last_error) AS effective_error,
    domain.cert_not_after_micros, config.last_alerted_state,
    config.last_alerted_at_micros, config.created_at_micros, config.updated_at_micros";

#[async_trait]
impl StatusPageDomainRepository for PgStatusPageRepository {
    async fn create_domain_config(
        &self,
        config: StatusPageDomainConfig,
    ) -> Result<StatusPageDomainConfig> {
        let mut transaction = sqlx::begin(&self.pool).await.map_err(super::sqlx_err)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(format!("custom-domain:{}", config.hostname))
            .execute(&mut *transaction)
            .await
            .map_err(super::sqlx_err)?;
        let previous_domain_id: Option<String> = sqlx::query_scalar(
            "SELECT domain_id FROM status_page_domain_configs
             WHERE org_id = $1 AND status_page_id = $2 FOR UPDATE",
        )
        .bind(&config.org_id.0)
        .bind(&config.status_page_id.0)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?
        .flatten();
        let reserved_by_generic_domain: bool = sqlx::query_scalar(
            "SELECT EXISTS (
                 SELECT 1 FROM domains
                 WHERE hostname = $1 AND ($2::VARCHAR IS NULL OR id <> $2)
             )",
        )
        .bind(&config.hostname)
        .bind(previous_domain_id.as_deref())
        .fetch_one(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;
        if reserved_by_generic_domain {
            return Err(Error::conflict(
                "custom domain is already reserved by another domain configuration",
            ));
        }
        sqlx::query(
            "DELETE FROM status_page_domain_configs
             WHERE org_id = $1 AND status_page_id = $2",
        )
        .bind(&config.org_id.0)
        .bind(&config.status_page_id.0)
        .execute(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;
        if let Some(domain_id) = previous_domain_id {
            sqlx::query("DELETE FROM domains WHERE org_id = $1 AND id = $2")
                .bind(&config.org_id.0)
                .bind(domain_id)
                .execute(&mut *transaction)
                .await
                .map_err(super::sqlx_err)?;
        }
        sqlx::query(
            "INSERT INTO status_page_domain_configs
                (org_id, status_page_id, hostname, verification_token, state, domain_id,
                 routing_valid, last_checked_at_micros, last_error,
                 created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, NULL, $6, $7, $8, $9, $10)",
        )
        .bind(&config.org_id.0)
        .bind(&config.status_page_id.0)
        .bind(&config.hostname)
        .bind(&config.verification_token)
        .bind(config.state.as_str())
        .bind(config.routing_valid)
        .bind(config.last_checked_at.map(|at| at.0))
        .bind(&config.last_error)
        .bind(config.created_at.0)
        .bind(config.updated_at.0)
        .execute(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;
        let updated = sqlx::query(
            "UPDATE status_pages SET custom_domain = $3, updated_at_micros = $4
             WHERE org_id = $1 AND id = $2 AND lifecycle = 'active'",
        )
        .bind(&config.org_id.0)
        .bind(&config.status_page_id.0)
        .bind(&config.hostname)
        .bind(config.updated_at.0)
        .execute(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;
        if updated.rows_affected() != 1 {
            return Err(Error::not_found("active status page not found"));
        }
        transaction.commit().await.map_err(super::sqlx_err)?;
        self.get_domain_config(&config.org_id, &config.status_page_id)
            .await?
            .ok_or_else(|| Error::internal("custom domain configuration was not persisted"))
    }

    async fn get_domain_config(
        &self,
        org_id: &Id,
        page_id: &Id,
    ) -> Result<Option<StatusPageDomainConfig>> {
        let sql = format!(
            "SELECT {DOMAIN_CONFIG_COLS}
             FROM status_page_domain_configs config
             LEFT JOIN domains domain
               ON domain.org_id = config.org_id AND domain.id = config.domain_id
             WHERE config.org_id = $1 AND config.status_page_id = $2"
        );
        sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&page_id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .map(row_to_domain_config)
            .transpose()
    }

    async fn list_domain_configs_due(
        &self,
        checked_before: TimestampMicros,
        limit: u32,
    ) -> Result<Vec<StatusPageDomainConfig>> {
        let sql = format!(
            "SELECT {DOMAIN_CONFIG_COLS}
             FROM status_page_domain_configs config
             JOIN status_pages page
               ON page.org_id = config.org_id AND page.id = config.status_page_id
             LEFT JOIN domains domain
               ON domain.org_id = config.org_id AND domain.id = config.domain_id
             WHERE page.lifecycle = 'active'
               AND (config.last_checked_at_micros IS NULL
                    OR config.last_checked_at_micros <= $1)
             ORDER BY config.last_checked_at_micros NULLS FIRST, config.updated_at_micros
             LIMIT $2"
        );
        sqlx::query(&sql)
            .bind(checked_before.0)
            .bind(i64::from(limit.min(100)))
            .fetch_all(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .into_iter()
            .map(row_to_domain_config)
            .collect()
    }

    async fn record_domain_check(
        &self,
        update: StatusPageDomainCheckUpdate,
    ) -> Result<StatusPageDomainConfig> {
        let StatusPageDomainCheckUpdate {
            org_id,
            status_page_id,
            state,
            routing_valid,
            last_error,
            checked_at,
            provision_tls,
        } = update;
        let mut transaction = sqlx::begin(&self.pool).await.map_err(super::sqlx_err)?;
        let row = sqlx::query(
            "SELECT hostname, domain_id FROM status_page_domain_configs
             WHERE org_id = $1 AND status_page_id = $2 FOR UPDATE",
        )
        .bind(&org_id.0)
        .bind(&status_page_id.0)
        .fetch_one(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;
        let hostname: String = row.try_get("hostname").map_err(super::sqlx_err)?;
        let mut domain_id: Option<String> = row.try_get("domain_id").map_err(super::sqlx_err)?;
        let stored_state = if provision_tls {
            if domain_id.is_none() {
                let new_domain_id = Id::new();
                sqlx::query(
                    "INSERT INTO domains
                        (id, org_id, hostname, state, cert_pem, cert_not_after_micros,
                         last_error, created_at_micros, updated_at_micros)
                     VALUES ($1, $2, $3, 'pending', NULL, NULL, NULL, $4, $4)",
                )
                .bind(&new_domain_id.0)
                .bind(&org_id.0)
                .bind(&hostname)
                .bind(checked_at.0)
                .execute(&mut *transaction)
                .await
                .map_err(super::sqlx_err)?;
                domain_id = Some(new_domain_id.0);
            }
            StatusPageDomainState::ProvisioningTls
        } else {
            state
        };
        sqlx::query(
            "UPDATE status_page_domain_configs SET
                 state = $3, domain_id = $4, routing_valid = $5,
                 last_checked_at_micros = $6, last_error = $7,
                 updated_at_micros = $6
             WHERE org_id = $1 AND status_page_id = $2",
        )
        .bind(&org_id.0)
        .bind(&status_page_id.0)
        .bind(stored_state.as_str())
        .bind(&domain_id)
        .bind(routing_valid)
        .bind(checked_at.0)
        .bind(last_error)
        .execute(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;
        transaction.commit().await.map_err(super::sqlx_err)?;
        self.get_domain_config(&org_id, &status_page_id)
            .await?
            .ok_or_else(|| Error::not_found("custom domain configuration not found"))
    }

    async fn retry_domain_tls(
        &self,
        org_id: &Id,
        page_id: &Id,
        updated_at: TimestampMicros,
    ) -> Result<StatusPageDomainConfig> {
        let mut transaction = sqlx::begin(&self.pool).await.map_err(super::sqlx_err)?;
        let domain_id: String = sqlx::query_scalar(
            "SELECT domain_id FROM status_page_domain_configs
             WHERE org_id = $1 AND status_page_id = $2
               AND domain_id IS NOT NULL AND routing_valid
             FOR UPDATE",
        )
        .bind(&org_id.0)
        .bind(&page_id.0)
        .fetch_one(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;
        sqlx::query(
            "UPDATE domains SET state = 'pending', last_error = NULL,
                 updated_at_micros = $3
             WHERE org_id = $1 AND id = $2",
        )
        .bind(&org_id.0)
        .bind(&domain_id)
        .bind(updated_at.0)
        .execute(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;
        sqlx::query(
            "UPDATE status_page_domain_configs SET state = 'provisioning_tls',
                 last_error = NULL, updated_at_micros = $3
             WHERE org_id = $1 AND status_page_id = $2",
        )
        .bind(&org_id.0)
        .bind(&page_id.0)
        .bind(updated_at.0)
        .execute(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;
        transaction.commit().await.map_err(super::sqlx_err)?;
        self.get_domain_config(org_id, page_id)
            .await?
            .ok_or_else(|| Error::not_found("custom domain configuration not found"))
    }

    async fn mark_domain_health_alerted(
        &self,
        org_id: &Id,
        page_id: &Id,
        state: Option<StatusPageDomainState>,
        updated_at: TimestampMicros,
    ) -> Result<()> {
        let result = sqlx::query(
            "UPDATE status_page_domain_configs SET
                 last_alerted_state = $3,
                 last_alerted_at_micros = CASE WHEN $3 IS NULL THEN NULL ELSE $4 END,
                 updated_at_micros = $4
             WHERE org_id = $1 AND status_page_id = $2",
        )
        .bind(&org_id.0)
        .bind(&page_id.0)
        .bind(state.map(StatusPageDomainState::as_str))
        .bind(updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(super::sqlx_err)?;
        if result.rows_affected() != 1 {
            return Err(Error::not_found("custom domain configuration not found"));
        }
        Ok(())
    }

    async fn delete_domain_config(&self, org_id: &Id, page_id: &Id) -> Result<()> {
        let mut transaction = sqlx::begin(&self.pool).await.map_err(super::sqlx_err)?;
        let domain_id: Option<String> = sqlx::query_scalar(
            "DELETE FROM status_page_domain_configs
             WHERE org_id = $1 AND status_page_id = $2
             RETURNING domain_id",
        )
        .bind(&org_id.0)
        .bind(&page_id.0)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?
        .flatten();
        sqlx::query(
            "UPDATE status_pages SET custom_domain = NULL, updated_at_micros = $3
             WHERE org_id = $1 AND id = $2",
        )
        .bind(&org_id.0)
        .bind(&page_id.0)
        .bind(TimestampMicros::now().0)
        .execute(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;
        if let Some(domain_id) = domain_id {
            sqlx::query("DELETE FROM domains WHERE org_id = $1 AND id = $2")
                .bind(&org_id.0)
                .bind(domain_id)
                .execute(&mut *transaction)
                .await
                .map_err(super::sqlx_err)?;
        }
        transaction.commit().await.map_err(super::sqlx_err)
    }
}

fn row_to_domain_config(row: sqlx::postgres::PgRow) -> Result<StatusPageDomainConfig> {
    let state: String = row.try_get("effective_state").map_err(super::sqlx_err)?;
    Ok(StatusPageDomainConfig {
        org_id: Id(row.try_get("org_id").map_err(super::sqlx_err)?),
        status_page_id: Id(row.try_get("status_page_id").map_err(super::sqlx_err)?),
        hostname: row.try_get("hostname").map_err(super::sqlx_err)?,
        verification_token: row.try_get("verification_token").map_err(super::sqlx_err)?,
        state: StatusPageDomainState::parse(&state)
            .ok_or_else(|| Error::internal(format!("unknown status-page domain state: {state}")))?,
        domain_id: row
            .try_get::<Option<String>, _>("domain_id")
            .map_err(super::sqlx_err)?
            .map(Id),
        routing_valid: row.try_get("routing_valid").map_err(super::sqlx_err)?,
        last_checked_at: row
            .try_get::<Option<i64>, _>("last_checked_at_micros")
            .map_err(super::sqlx_err)?
            .map(TimestampMicros),
        last_error: row.try_get("effective_error").map_err(super::sqlx_err)?,
        cert_not_after: row
            .try_get::<Option<i64>, _>("cert_not_after_micros")
            .map_err(super::sqlx_err)?
            .map(TimestampMicros),
        last_alerted_state: row
            .try_get::<Option<String>, _>("last_alerted_state")
            .map_err(super::sqlx_err)?
            .map(|state| {
                StatusPageDomainState::parse(&state).ok_or_else(|| {
                    Error::internal(format!("unknown alerted domain state: {state}"))
                })
            })
            .transpose()?,
        last_alerted_at: row
            .try_get::<Option<i64>, _>("last_alerted_at_micros")
            .map_err(super::sqlx_err)?
            .map(TimestampMicros),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(super::sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(super::sqlx_err)?),
    })
}
