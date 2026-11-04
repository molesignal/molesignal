// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::{Row, types::Json};

use super::{
    PgApmRepository,
    codec::{
        HOUR_MICROS, PersistedBucketRow, RollupBucket, as_i16, kind_name, rollup_rows,
        row_to_bucket,
    },
};
use crate::{
    domain::apm::{
        APM_PERSISTENCE_SCHEMA_VERSION, ApmMaintenanceRepository, BucketKind, RollupCandidate,
        RollupRequest, RollupStats,
    },
    infra::persistence::sqlx_err,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const BUCKET_KINDS: [BucketKind; 5] = [
    BucketKind::Service,
    BucketKind::Transaction,
    BucketKind::Dependency,
    BucketKind::Error,
    BucketKind::Instance,
];

#[async_trait]
impl ApmMaintenanceRepository for PgApmRepository {
    async fn rollup_candidates(
        &self,
        closed_before: TimestampMicros,
        limit: usize,
    ) -> Result<Vec<RollupCandidate>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let rows = sqlx::query(
            "SELECT org_id,
                    date_bin(INTERVAL '1 hour', bucket_at, TIMESTAMPTZ 'epoch') AS hour_at
             FROM apm_buckets
             WHERE bucket_at < $1
               AND bucket_kind IN ('service', 'transaction', 'dependency', 'error')
             GROUP BY org_id, hour_at
             ORDER BY hour_at ASC, org_id ASC
             LIMIT $2",
        )
        .bind(closed_before)
        .bind(i64::try_from(limit).map_err(|_| Error::invalid("APM rollup limit too large"))?)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter()
            .map(|row| {
                Ok(RollupCandidate {
                    org_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
                    hour_at: row.try_get("hour_at").map_err(sqlx_err)?,
                })
            })
            .collect()
    }

    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "apm_rollup")
    )]
    async fn rollup_and_retain(&self, request: &RollupRequest) -> Result<RollupStats> {
        if request.hour_at.0.rem_euclid(HOUR_MICROS) != 0 {
            return Err(Error::invalid(
                "APM rollup hour must align to a UTC hour boundary",
            ));
        }

        let hour_end = request.hour_at.0.saturating_add(HOUR_MICROS);
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        let lock_key = format!("apm-rollup:{}:{}", request.org_id, request.hour_at.0);

        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
            .bind(lock_key)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;

        let completed_at = TimestampMicros::now();
        let mut stats = RollupStats::default();

        for kind in BUCKET_KINDS {
            let source = load_source_rows(
                &mut tx,
                kind,
                request.org_id.as_str(),
                request.hour_at,
                TimestampMicros(hour_end),
            )
            .await?;

            stats.source_rows = stats.source_rows.saturating_add(source.len() as u64);
            let rollups = rollup_rows(source)?;

            for rollup in &rollups {
                upsert_rollup(&mut tx, kind, request, rollup, completed_at).await?;
            }

            stats.rollup_rows = stats.rollup_rows.saturating_add(rollups.len() as u64);
        }

        let deleted = sqlx::query(
            "DELETE FROM apm_buckets
             WHERE org_id = $1 AND bucket_at >= $2 AND bucket_at < $3",
        )
        .bind(request.org_id.as_str())
        .bind(request.hour_at)
        .bind(TimestampMicros(hour_end))
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        stats.deleted_hot_rows = stats
            .deleted_hot_rows
            .saturating_add(deleted.rows_affected());

        let deleted = sqlx::query("DELETE FROM apm_buckets WHERE org_id = $1 AND bucket_at < $2")
            .bind(request.org_id.as_str())
            .bind(request.hot_retention_cutoff)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;

        stats.deleted_hot_rows = stats
            .deleted_hot_rows
            .saturating_add(deleted.rows_affected());

        let deleted =
            sqlx::query("DELETE FROM apm_buckets_hourly WHERE org_id = $1 AND bucket_at < $2")
                .bind(request.org_id.as_str())
                .bind(request.rollup_retention_cutoff)
                .execute(&mut *tx)
                .await
                .map_err(sqlx_err)?;

        stats.deleted_rollup_rows = stats
            .deleted_rollup_rows
            .saturating_add(deleted.rows_affected());

        // An instance not seen for as long as hourly aggregates are kept is gone.
        let deleted = sqlx::query(
            "DELETE FROM apm_service_instances WHERE org_id = $1 AND last_seen_at < $2",
        )
        .bind(request.org_id.as_str())
        .bind(request.rollup_retention_cutoff)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        stats.deleted_rollup_rows = stats
            .deleted_rollup_rows
            .saturating_add(deleted.rows_affected());

        sqlx::query(
            "UPDATE apm_projection_state
             SET last_rollup_bucket_at = GREATEST(
                COALESCE(last_rollup_bucket_at, $2), $2
             )
             WHERE org_id = $1",
        )
        .bind(request.org_id.as_str())
        .bind(request.hour_at)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        tx.commit().await.map_err(sqlx_err)?;
        Ok(stats)
    }
}

async fn load_source_rows(
    tx: &mut sqlx::TracedTransaction<'_>,
    kind: BucketKind,
    org_id: &str,
    hour_start: TimestampMicros,
    hour_end: TimestampMicros,
) -> Result<Vec<PersistedBucketRow>> {
    sqlx::query(
        "SELECT bucket_at, dimension_key, histogram_schema_version,
                dimension, measurements
         FROM apm_buckets
         WHERE org_id = $1 AND bucket_kind = $4 AND bucket_at >= $2 AND bucket_at < $3
         ORDER BY bucket_at, dimension_key, owner_id
         FOR UPDATE",
    )
    .bind(org_id)
    .bind(hour_start)
    .bind(hour_end)
    .bind(kind_name(kind))
    .fetch_all(&mut **tx)
    .await
    .map_err(sqlx_err)?
    .into_iter()
    .map(row_to_bucket)
    .collect()
}

async fn upsert_rollup(
    tx: &mut sqlx::TracedTransaction<'_>,
    kind: BucketKind,
    request: &RollupRequest,
    rollup: &RollupBucket,
    completed_at: TimestampMicros,
) -> Result<()> {
    let service = rollup.dimension.service();

    sqlx::query(
        "INSERT INTO apm_buckets_hourly (
            org_id, service_namespace, service_name, environment, version,
            bucket_at, persistence_schema_version, histogram_schema_version,
            dimension_key, dimension, measurements, source_minute_count,
            completed_at, bucket_kind
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
         ON CONFLICT (
            org_id, bucket_kind, bucket_at, dimension_key, histogram_schema_version
         ) DO UPDATE SET
            service_namespace = EXCLUDED.service_namespace,
            service_name = EXCLUDED.service_name,
            environment = EXCLUDED.environment,
            version = EXCLUDED.version,
            persistence_schema_version = EXCLUDED.persistence_schema_version,
            dimension = EXCLUDED.dimension,
            measurements = EXCLUDED.measurements,
            source_minute_count = EXCLUDED.source_minute_count,
            completed_at = EXCLUDED.completed_at",
    )
    .bind(request.org_id.as_str())
    .bind(&service.namespace)
    .bind(&service.name)
    .bind(&service.environment)
    .bind(rollup.dimension.version().unwrap_or_default())
    .bind(request.hour_at)
    .bind(as_i16(
        APM_PERSISTENCE_SCHEMA_VERSION,
        "persistence schema version",
    )?)
    .bind(as_i16(
        rollup.histogram_schema_version,
        "histogram schema version",
    )?)
    .bind(&rollup.dimension_key)
    .bind(Json(&rollup.dimension))
    .bind(Json(&rollup.measurements))
    .bind(as_i16(rollup.source_minute_count, "source minute count")?)
    .bind(completed_at)
    .bind(kind_name(kind))
    .execute(&mut **tx)
    .await
    .map_err(sqlx_err)?;

    Ok(())
}
