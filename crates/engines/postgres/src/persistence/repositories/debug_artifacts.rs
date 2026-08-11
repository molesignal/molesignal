// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! PostgreSQL adapter for RUM debug artifact metadata.

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use super::sqlx_err;
use crate::{
    domain::rum::{
        DebugArtifactKind, DebugArtifactLookup, DebugArtifactMeta, DebugArtifactRepository,
        DebugArtifactUpsert,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub struct PgDebugArtifactRepository {
    pool: PgPool,
}

impl PgDebugArtifactRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, application_id, service, release, artifact_kind, platform,
                    architecture, debug_id, filename, object_key, size_bytes, checksum_sha256,
                    uploaded_at_micros";

fn row_to(row: sqlx::postgres::PgRow) -> DebugArtifactMeta {
    let kind = row
        .try_get::<String, _>("artifact_kind")
        .ok()
        .and_then(|value| DebugArtifactKind::parse(&value))
        .unwrap_or(DebugArtifactKind::JavascriptSourcemap);
    DebugArtifactMeta {
        id: Id(row.try_get::<String, _>("id").unwrap_or_default()),
        org_id: Id(row.try_get::<String, _>("org_id").unwrap_or_default()),
        application_id: row
            .try_get::<String, _>("application_id")
            .unwrap_or_default(),
        service: row.try_get::<String, _>("service").unwrap_or_default(),
        release: row.try_get::<String, _>("release").unwrap_or_default(),
        kind,
        platform: row.try_get::<String, _>("platform").unwrap_or_default(),
        architecture: row.try_get::<String, _>("architecture").unwrap_or_default(),
        debug_id: row.try_get::<String, _>("debug_id").unwrap_or_default(),
        filename: row.try_get::<String, _>("filename").unwrap_or_default(),
        object_key: row.try_get::<String, _>("object_key").unwrap_or_default(),
        size_bytes: row
            .try_get::<i64, _>("size_bytes")
            .unwrap_or_default()
            .max(0) as u64,
        checksum_sha256: row
            .try_get::<String, _>("checksum_sha256")
            .unwrap_or_default(),
        uploaded_at: TimestampMicros(
            row.try_get::<i64, _>("uploaded_at_micros")
                .unwrap_or_default(),
        ),
    }
}

#[async_trait]
impl DebugArtifactRepository for PgDebugArtifactRepository {
    async fn create(&self, artifact: DebugArtifactMeta) -> Result<DebugArtifactUpsert> {
        let mut transaction = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        let identity_lock = format!(
            "debug-artifact:{}:{}:{}:{}:{}:{}:{}:{}:{}",
            artifact.org_id.0,
            artifact.application_id,
            artifact.service,
            artifact.release,
            artifact.kind.as_str(),
            artifact.platform,
            artifact.architecture,
            artifact.debug_id,
            artifact.filename,
        );
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(identity_lock)
            .execute(&mut *transaction)
            .await
            .map_err(sqlx_err)?;
        let replaced_object_key = sqlx::query_scalar::<String>(
            "SELECT object_key FROM debug_artifacts
             WHERE org_id = $1 AND application_id = $2 AND service = $3 AND release = $4
               AND artifact_kind = $5 AND platform = $6 AND architecture = $7
               AND debug_id = $8 AND filename = $9",
        )
        .bind(&artifact.org_id.0)
        .bind(&artifact.application_id)
        .bind(&artifact.service)
        .bind(&artifact.release)
        .bind(artifact.kind.as_str())
        .bind(&artifact.platform)
        .bind(&artifact.architecture)
        .bind(&artifact.debug_id)
        .bind(&artifact.filename)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(sqlx_err)?;
        let sql = format!(
            "INSERT INTO debug_artifacts
                (id, org_id, application_id, service, release, artifact_kind, platform,
                 architecture, debug_id, filename, object_key, size_bytes, checksum_sha256,
                 uploaded_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
             ON CONFLICT (
                 org_id, application_id, service, release, artifact_kind, platform,
                 architecture, debug_id, filename
             ) DO UPDATE SET
                 object_key = EXCLUDED.object_key,
                 size_bytes = EXCLUDED.size_bytes,
                 checksum_sha256 = EXCLUDED.checksum_sha256,
                 uploaded_at_micros = EXCLUDED.uploaded_at_micros
             RETURNING {COLS}"
        );
        let row = sqlx::query(&sql)
            .bind(&artifact.id.0)
            .bind(&artifact.org_id.0)
            .bind(&artifact.application_id)
            .bind(&artifact.service)
            .bind(&artifact.release)
            .bind(artifact.kind.as_str())
            .bind(&artifact.platform)
            .bind(&artifact.architecture)
            .bind(&artifact.debug_id)
            .bind(&artifact.filename)
            .bind(&artifact.object_key)
            .bind(artifact.size_bytes.min(i64::MAX as u64) as i64)
            .bind(&artifact.checksum_sha256)
            .bind(artifact.uploaded_at.0)
            .fetch_one(&mut *transaction)
            .await
            .map_err(sqlx_err)?;
        transaction.commit().await.map_err(sqlx_err)?;
        Ok(DebugArtifactUpsert {
            artifact: row_to(row),
            replaced_object_key,
        })
    }

    async fn list(
        &self,
        org_id: &Id,
        application_id: Option<&str>,
        service: Option<&str>,
        kind: Option<DebugArtifactKind>,
        platform: Option<&str>,
    ) -> Result<Vec<DebugArtifactMeta>> {
        let sql = format!(
            "SELECT {COLS} FROM debug_artifacts
             WHERE org_id = $1
               AND ($2::TEXT IS NULL OR application_id = $2)
               AND ($3::TEXT IS NULL OR service = $3)
               AND ($4::TEXT IS NULL OR artifact_kind = $4)
               AND ($5::TEXT IS NULL OR platform = $5)
             ORDER BY uploaded_at_micros DESC LIMIT 500"
        );
        let rows = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(application_id)
            .bind(service)
            .bind(kind.map(DebugArtifactKind::as_str))
            .bind(platform)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(rows.into_iter().map(row_to).collect())
    }

    async fn find_best(
        &self,
        org_id: &Id,
        lookup: &DebugArtifactLookup<'_>,
    ) -> Result<Option<DebugArtifactMeta>> {
        let sql = format!(
            "SELECT {COLS} FROM debug_artifacts
             WHERE org_id = $1
               AND application_id = $2
               AND service = $3
               AND release = $4
               AND artifact_kind = $5
               AND ($6::TEXT IS NULL OR platform = $6)
               AND ($7::TEXT IS NULL OR architecture = $7 OR architecture = '')
               AND ($8::TEXT IS NULL OR debug_id = $8 OR debug_id = '')
               AND ($9::TEXT IS NULL OR filename = $9)
             ORDER BY
               CASE WHEN architecture = COALESCE($7, '') THEN 0 ELSE 1 END,
               CASE WHEN debug_id = COALESCE($8, '') THEN 0 ELSE 1 END,
               uploaded_at_micros DESC
             LIMIT 65"
        );
        let rows = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(lookup.application_id)
            .bind(lookup.service)
            .bind(lookup.release)
            .bind(lookup.kind.as_str())
            .bind(lookup.platform)
            .bind(lookup.architecture)
            .bind(lookup.debug_id)
            .bind(lookup.filename)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;
        let artifacts: Vec<_> = rows.into_iter().map(row_to).collect();
        let preferred: Vec<_> = artifacts
            .iter()
            .filter(|artifact| {
                lookup
                    .architecture
                    .map_or(artifact.architecture.is_empty(), |value| {
                        artifact.architecture.as_str() == value
                    })
                    && lookup
                        .debug_id
                        .map_or(artifact.debug_id.is_empty(), |value| {
                            artifact.debug_id.as_str() == value
                        })
            })
            .collect();
        if preferred.len() == 1 && (artifacts.len() < 65 || lookup.filename.is_some()) {
            return Ok(preferred.first().map(|artifact| (*artifact).clone()));
        }
        if preferred.len() > 1 || artifacts.len() != 1 {
            return Ok(None);
        }
        Ok(artifacts.into_iter().next())
    }

    async fn delete(&self, org_id: &Id, id: &Id) -> Result<Option<DebugArtifactMeta>> {
        let sql =
            format!("DELETE FROM debug_artifacts WHERE org_id = $1 AND id = $2 RETURNING {COLS}");
        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(row.map(row_to))
    }
}
