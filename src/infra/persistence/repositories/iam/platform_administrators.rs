// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use super::super::sqlx_err;
use crate::{
    domain::iam::{IamPlatformAdministrator, IamPlatformAdministratorRepository},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct PgIamPlatformAdministratorRepository {
    pool: PgPool,
}

impl PgIamPlatformAdministratorRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "user_id, active, granted_by, granted_at_micros, revoked_by, revoked_at_micros";

fn row_to_assignment(row: sqlx::postgres::PgRow) -> Result<IamPlatformAdministrator> {
    Ok(IamPlatformAdministrator {
        user_id: Id(row.try_get("user_id").map_err(sqlx_err)?),
        active: row.try_get("active").map_err(sqlx_err)?,
        granted_by: row
            .try_get::<Option<String>, _>("granted_by")
            .map_err(sqlx_err)?
            .map(Id),
        granted_at: TimestampMicros(row.try_get("granted_at_micros").map_err(sqlx_err)?),
        revoked_by: row
            .try_get::<Option<String>, _>("revoked_by")
            .map_err(sqlx_err)?
            .map(Id),
        revoked_at: row
            .try_get::<Option<i64>, _>("revoked_at_micros")
            .map_err(sqlx_err)?
            .map(TimestampMicros),
    })
}

#[async_trait]
impl IamPlatformAdministratorRepository for PgIamPlatformAdministratorRepository {
    async fn list(&self) -> Result<Vec<IamPlatformAdministrator>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM iam_platform_administrators ORDER BY granted_at_micros, user_id"
        ))
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(row_to_assignment).collect()
    }

    async fn is_active(&self, user_id: &Id) -> Result<bool> {
        let row = sqlx::query(
            "SELECT EXISTS (
                SELECT 1
                FROM iam_platform_administrators pa
                JOIN users u ON u.id = pa.user_id
                WHERE pa.user_id = $1
                  AND pa.active
                  AND NOT u.disabled
                  AND u.status = 'active'
             ) AS active",
        )
        .bind(&user_id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;
        row.try_get("active").map_err(sqlx_err)
    }

    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "iam_platform_administrators")
    )]
    async fn bootstrap_root(&self, user_id: &Id) -> Result<bool> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext('molesignal.platform.admins'))")
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        let valid_root: bool = sqlx::query_scalar(
            "SELECT EXISTS (
                SELECT 1 FROM users
                 WHERE id = $1 AND NOT disabled AND status = 'active'
             )",
        )
        .bind(&user_id.0)
        .fetch_one(&mut *tx)
        .await
        .map_err(sqlx_err)?;
        if !valid_root {
            return Err(Error::invalid(
                "configured root user is not active and cannot become platform administrator",
            ));
        }

        let already_reconciled: bool = sqlx::query_scalar(
            "SELECT COUNT(*) = 1
                AND COALESCE(BOOL_OR(user_id = $1), FALSE)
               FROM iam_platform_administrators
              WHERE active",
        )
        .bind(&user_id.0)
        .fetch_one(&mut *tx)
        .await
        .map_err(sqlx_err)?;
        if already_reconciled {
            tx.commit().await.map_err(sqlx_err)?;
            return Ok(false);
        }

        sqlx::query("SET LOCAL molesignal.root_reconcile = 'true'")
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        let now = TimestampMicros::now();
        sqlx::query(
            "UPDATE iam_platform_administrators
                SET active = FALSE,
                    revoked_by = $1,
                    revoked_at_micros = $2
              WHERE active AND user_id <> $1",
        )
        .bind(&user_id.0)
        .bind(now.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
        let reconciled = sqlx::query(
            "INSERT INTO iam_platform_administrators
                (user_id, active, granted_by, granted_at_micros, revoked_by, revoked_at_micros)
             VALUES ($1, TRUE, $1, $2, NULL, NULL)
             ON CONFLICT (user_id) DO UPDATE
             SET active = TRUE,
                 granted_by = EXCLUDED.granted_by,
                 granted_at_micros = EXCLUDED.granted_at_micros,
                 revoked_by = NULL,
                 revoked_at_micros = NULL",
        )
        .bind(&user_id.0)
        .bind(now.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?
        .rows_affected();
        if reconciled != 1 {
            return Err(Error::internal("failed to reconcile configured root user"));
        }
        tx.commit().await.map_err(sqlx_err)?;
        Ok(true)
    }
}
