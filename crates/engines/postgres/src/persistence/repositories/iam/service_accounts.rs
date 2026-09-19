// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use super::super::sqlx_err;
use crate::{
    domain::iam::{
        api_token::{ApiToken, ApiTokenKind},
        service_account::{ServiceAccount, ServiceAccountRepository},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct PgServiceAccountRepository {
    pool: PgPool,
}

impl PgServiceAccountRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, name, description, role_id, disabled, created_by,
                    created_at_micros, updated_at_micros";

fn row_to(row: sqlx::postgres::PgRow) -> Result<ServiceAccount> {
    Ok(ServiceAccount {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        name: row.try_get("name").map_err(sqlx_err)?,
        description: row.try_get("description").map_err(sqlx_err)?,
        role_id: Id(row.try_get("role_id").map_err(sqlx_err)?),
        disabled: row.try_get("disabled").map_err(sqlx_err)?,
        created_by: Id(row.try_get("created_by").map_err(sqlx_err)?),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

#[async_trait]
impl ServiceAccountRepository for PgServiceAccountRepository {
    async fn provision(
        &self,
        account: ServiceAccount,
        initial_token: ApiToken,
    ) -> Result<(ServiceAccount, ApiToken)> {
        if initial_token.token_kind != ApiTokenKind::ServiceAccount
            || initial_token.org_id != account.org_id
            || initial_token.role_id != account.role_id
            || initial_token.service_account_id.as_ref() != Some(&account.id)
        {
            return Err(Error::invalid(
                "initial API token must be bound to the new service account, organization, and role",
            ));
        }
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        let sql = format!(
            "INSERT INTO service_accounts
                (id, org_id, name, description, role_id, disabled, created_by,
                 created_at_micros, updated_at_micros)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
             RETURNING {COLS}"
        );
        let row = sqlx::query(&sql)
            .bind(&account.id.0)
            .bind(&account.org_id.0)
            .bind(&account.name)
            .bind(&account.description)
            .bind(&account.role_id.0)
            .bind(account.disabled)
            .bind(&account.created_by.0)
            .bind(account.created_at.0)
            .bind(account.updated_at.0)
            .fetch_one(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        sqlx::query(
            "INSERT INTO api_tokens
                (id, prefix, secret_hash, org_id, user_id, role_id, name,
                 expires_at_micros, last_used_at_micros, revoked, created_at_micros,
                 is_default, token_kind, application_id, service_account_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NULL, $9, $10, $11, $12, $13, $14)",
        )
        .bind(&initial_token.id.0)
        .bind(&initial_token.prefix)
        .bind(&initial_token.secret_hash)
        .bind(&initial_token.org_id.0)
        .bind(&initial_token.user_id.0)
        .bind(&initial_token.role_id.0)
        .bind(&initial_token.name)
        .bind(initial_token.expires_at.map(|value| value.0))
        .bind(initial_token.revoked)
        .bind(initial_token.created_at.0)
        .bind(initial_token.is_default)
        .bind(initial_token.token_kind.as_str())
        .bind(&initial_token.application_id)
        .bind(initial_token.service_account_id.as_ref().map(|id| &id.0))
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
        let saved_account = row_to(row)?;
        tx.commit().await.map_err(sqlx_err)?;
        Ok((saved_account, initial_token))
    }

    async fn get(&self, org_id: &Id, id: &Id) -> Result<ServiceAccount> {
        let sql = format!(
            "SELECT {COLS} FROM service_accounts
             WHERE org_id = $1 AND id = $2 AND deleted_at_micros IS NULL"
        );
        row_to(
            sqlx::query(&sql)
                .bind(&org_id.0)
                .bind(&id.0)
                .fetch_one(&self.pool)
                .await
                .map_err(sqlx_err)?,
        )
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<ServiceAccount>> {
        let sql = format!(
            "SELECT {COLS} FROM service_accounts
             WHERE org_id = $1 AND deleted_at_micros IS NULL
             ORDER BY name, created_at_micros DESC LIMIT 1000"
        );
        sqlx::query(&sql)
            .bind(&org_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?
            .into_iter()
            .map(row_to)
            .collect()
    }

    async fn update(&self, account: ServiceAccount) -> Result<ServiceAccount> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        let sql = format!(
            "UPDATE service_accounts
             SET name = $3, description = $4, role_id = $5, updated_at_micros = $6
             WHERE org_id = $1 AND id = $2 AND deleted_at_micros IS NULL
             RETURNING {COLS}"
        );
        let row = sqlx::query(&sql)
            .bind(&account.org_id.0)
            .bind(&account.id.0)
            .bind(&account.name)
            .bind(&account.description)
            .bind(&account.role_id.0)
            .bind(account.updated_at.0)
            .fetch_optional(&mut *tx)
            .await
            .map_err(sqlx_err)?
            .ok_or_else(|| Error::not_found("service account not found"))?;
        sqlx::query(
            "UPDATE api_tokens SET role_id = $3
             WHERE org_id = $1 AND service_account_id = $2
               AND token_kind = 'service_account' AND NOT revoked",
        )
        .bind(&account.org_id.0)
        .bind(&account.id.0)
        .bind(&account.role_id.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
        tx.commit().await.map_err(sqlx_err)?;
        row_to(row)
    }

    async fn set_disabled(
        &self,
        org_id: &Id,
        id: &Id,
        disabled: bool,
        updated_at: TimestampMicros,
    ) -> Result<ServiceAccount> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        let sql = format!(
            "UPDATE service_accounts SET disabled = $3, updated_at_micros = $4
             WHERE org_id = $1 AND id = $2 AND deleted_at_micros IS NULL
             RETURNING {COLS}"
        );
        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&id.0)
            .bind(disabled)
            .bind(updated_at.0)
            .fetch_optional(&mut *tx)
            .await
            .map_err(sqlx_err)?
            .ok_or_else(|| Error::not_found("service account not found"))?;
        if disabled {
            sqlx::query(
                "UPDATE api_tokens SET revoked = TRUE
                 WHERE org_id = $1 AND service_account_id = $2
                   AND token_kind = 'service_account' AND NOT revoked",
            )
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        }
        tx.commit().await.map_err(sqlx_err)?;
        row_to(row)
    }

    async fn delete(&self, org_id: &Id, id: &Id, deleted_at: TimestampMicros) -> Result<()> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        let result = sqlx::query(
            "UPDATE service_accounts
             SET disabled = TRUE, deleted_at_micros = $3, updated_at_micros = $3
             WHERE org_id = $1 AND id = $2 AND deleted_at_micros IS NULL",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .bind(deleted_at.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
        if result.rows_affected() == 0 {
            return Err(Error::not_found("service account not found"));
        }
        sqlx::query(
            "UPDATE api_tokens SET revoked = TRUE
             WHERE org_id = $1 AND service_account_id = $2
               AND token_kind = 'service_account' AND NOT revoked",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
        tx.commit().await.map_err(sqlx_err)?;
        Ok(())
    }
}
