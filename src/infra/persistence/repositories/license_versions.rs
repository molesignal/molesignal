// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::{
    domain::license::{ActiveLicenseVersion, LicenseVersion, LicenseVersionRepository},
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub struct PgLicenseVersionRepository {
    pool: PgPool,
}

impl PgLicenseVersionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const VERSION_COLS: &str =
    "id, system_org_id, signed_package, payload_digest, summary, created_by, created_at_micros";

fn row_to_version(row: sqlx::postgres::PgRow) -> Result<LicenseVersion> {
    Ok(LicenseVersion {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        system_org_id: Id(row.try_get("system_org_id").map_err(sqlx_err)?),
        signed_package: row
            .try_get::<Json<serde_json::Value>, _>("signed_package")
            .map_err(sqlx_err)?
            .0,
        payload_digest: row.try_get("payload_digest").map_err(sqlx_err)?,
        summary: row
            .try_get::<Json<serde_json::Value>, _>("summary")
            .map_err(sqlx_err)?
            .0,
        created_by: row
            .try_get::<Option<String>, _>("created_by")
            .map_err(sqlx_err)?
            .map(Id),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
    })
}

#[async_trait]
impl LicenseVersionRepository for PgLicenseVersionRepository {
    async fn list(&self) -> Result<Vec<LicenseVersion>> {
        let rows = sqlx::query(&format!(
            "SELECT {VERSION_COLS} FROM license_versions ORDER BY created_at_micros DESC, id DESC"
        ))
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(row_to_version).collect()
    }

    async fn get(&self, id: &Id) -> Result<LicenseVersion> {
        let row = sqlx::query(&format!(
            "SELECT {VERSION_COLS} FROM license_versions WHERE id = $1"
        ))
        .bind(&id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;
        row_to_version(row)
    }

    async fn active(&self) -> Result<Option<ActiveLicenseVersion>> {
        let row = sqlx::query(&format!(
            "SELECT {VERSION_COLS}, a.activated_by, a.activated_at_micros
             FROM active_license_version a
             JOIN license_versions v ON v.id = a.version_id
             WHERE a.singleton_id = 1"
        ))
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;
        row.map(|row| {
            let activated_by = row
                .try_get::<Option<String>, _>("activated_by")
                .map_err(sqlx_err)?
                .map(Id);
            let activated_at =
                TimestampMicros(row.try_get("activated_at_micros").map_err(sqlx_err)?);
            Ok(ActiveLicenseVersion {
                version: row_to_version(row)?,
                activated_by,
                activated_at,
            })
        })
        .transpose()
    }

    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "license_versions")
    )]
    async fn insert_and_activate(
        &self,
        version: LicenseVersion,
        actor_id: Option<&Id>,
    ) -> Result<ActiveLicenseVersion> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext('molesignal.license.active'))")
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        sqlx::query(
            "INSERT INTO license_versions
                (id, system_org_id, signed_package, payload_digest, summary,
                 created_by, created_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(&version.id.0)
        .bind(&version.system_org_id.0)
        .bind(Json(&version.signed_package))
        .bind(&version.payload_digest)
        .bind(Json(&version.summary))
        .bind(version.created_by.as_ref().map(|id| id.0.as_str()))
        .bind(version.created_at.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
        let activated_at = TimestampMicros::now();
        sqlx::query(
            "INSERT INTO active_license_version
                (singleton_id, version_id, activated_by, activated_at_micros)
             VALUES (1, $1, $2, $3)
             ON CONFLICT (singleton_id) DO UPDATE
             SET version_id = EXCLUDED.version_id,
                 activated_by = EXCLUDED.activated_by,
                 activated_at_micros = EXCLUDED.activated_at_micros",
        )
        .bind(&version.id.0)
        .bind(actor_id.map(|id| id.0.as_str()))
        .bind(activated_at.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
        tx.commit().await.map_err(sqlx_err)?;
        Ok(ActiveLicenseVersion {
            version,
            activated_by: actor_id.cloned(),
            activated_at,
        })
    }

    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "license_versions")
    )]
    async fn activate(&self, id: &Id, actor_id: &Id) -> Result<ActiveLicenseVersion> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext('molesignal.license.active'))")
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        let row = sqlx::query(&format!(
            "SELECT {VERSION_COLS} FROM license_versions WHERE id = $1"
        ))
        .bind(&id.0)
        .fetch_one(&mut *tx)
        .await
        .map_err(sqlx_err)?;
        let version = row_to_version(row)?;
        let activated_at = TimestampMicros::now();
        sqlx::query(
            "INSERT INTO active_license_version
                (singleton_id, version_id, activated_by, activated_at_micros)
             VALUES (1, $1, $2, $3)
             ON CONFLICT (singleton_id) DO UPDATE
             SET version_id = EXCLUDED.version_id,
                 activated_by = EXCLUDED.activated_by,
                 activated_at_micros = EXCLUDED.activated_at_micros",
        )
        .bind(&id.0)
        .bind(&actor_id.0)
        .bind(activated_at.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
        tx.commit().await.map_err(sqlx_err)?;
        Ok(ActiveLicenseVersion {
            version,
            activated_by: Some(actor_id.clone()),
            activated_at,
        })
    }
}
