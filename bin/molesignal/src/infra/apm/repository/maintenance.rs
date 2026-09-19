// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeSet;

use async_trait::async_trait;
use sqlx::{Row, types::Json};

use super::{
    PgApmRepository,
    codec::{
        HOUR_MICROS, PersistedBucketRow, RollupBucket, as_i16, hourly_table, kind_name,
        minute_table, rollup_rows, row_to_bucket,
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

const BUCKET_KINDS: [BucketKind; 4] = [
    BucketKind::Service,
    BucketKind::Transaction,
    BucketKind::Dependency,
    BucketKind::Error,
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
            "WITH minute_rows AS (
                SELECT org_id, bucket_at_micros FROM apm_service_buckets
                UNION ALL
                SELECT org_id, bucket_at_micros FROM apm_transaction_buckets
                UNION ALL
                SELECT org_id, bucket_at_micros FROM apm_dependency_buckets
                UNION ALL
                SELECT org_id, bucket_at_micros FROM apm_error_buckets
             )
             SELECT org_id,
                    (bucket_at_micros / 3600000000) * 3600000000 AS hour_at_micros
             FROM minute_rows
             WHERE bucket_at_micros < $1
             GROUP BY org_id, hour_at_micros
             ORDER BY hour_at_micros ASC, org_id ASC
             LIMIT $2",
        )
        .bind(closed_before.0)
        .bind(i64::try_from(limit).map_err(|_| Error::invalid("APM rollup limit too large"))?)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter()
            .map(|row| {
                Ok(RollupCandidate {
                    org_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
                    hour_at: TimestampMicros(row.try_get("hour_at_micros").map_err(sqlx_err)?),
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
                minute_table(kind),
                request.org_id.as_str(),
                request.hour_at.0,
                hour_end,
            )
            .await?;
            stats.source_rows = stats.source_rows.saturating_add(source.len() as u64);
            let rollups = rollup_rows(source)?;
            let mut schema_versions = BTreeSet::new();
            for rollup in &rollups {
                schema_versions.insert(rollup.histogram_schema_version);
                upsert_rollup(&mut tx, hourly_table(kind), request, rollup, completed_at).await?;
            }
            stats.rollup_rows = stats.rollup_rows.saturating_add(rollups.len() as u64);

            let delete_source_sql = format!(
                "DELETE FROM {} WHERE org_id = $1
                 AND bucket_at_micros >= $2 AND bucket_at_micros < $3",
                minute_table(kind)
            );
            let deleted = sqlx::query(&delete_source_sql)
                .bind(request.org_id.as_str())
                .bind(request.hour_at.0)
                .bind(hour_end)
                .execute(&mut *tx)
                .await
                .map_err(sqlx_err)?;
            stats.deleted_hot_rows = stats
                .deleted_hot_rows
                .saturating_add(deleted.rows_affected());

            for schema_version in schema_versions {
                sqlx::query(
                    "INSERT INTO apm_rollup_state (
                        org_id, bucket_kind, histogram_schema_version,
                        completed_through_micros, updated_at_micros
                     ) VALUES ($1,$2,$3,$4,$5)
                     ON CONFLICT (org_id, bucket_kind, histogram_schema_version)
                     DO UPDATE SET
                        completed_through_micros = GREATEST(
                            apm_rollup_state.completed_through_micros,
                            EXCLUDED.completed_through_micros
                        ),
                        updated_at_micros = EXCLUDED.updated_at_micros",
                )
                .bind(request.org_id.as_str())
                .bind(kind_name(kind))
                .bind(as_i16(schema_version, "histogram schema version")?)
                .bind(hour_end.saturating_sub(1))
                .bind(completed_at.0)
                .execute(&mut *tx)
                .await
                .map_err(sqlx_err)?;
            }
        }

        for kind in BUCKET_KINDS {
            let hot_retention_sql = format!(
                "DELETE FROM {} WHERE org_id = $1 AND bucket_at_micros < $2",
                minute_table(kind)
            );
            let deleted = sqlx::query(&hot_retention_sql)
                .bind(request.org_id.as_str())
                .bind(request.hot_retention_cutoff.0)
                .execute(&mut *tx)
                .await
                .map_err(sqlx_err)?;
            stats.deleted_hot_rows = stats
                .deleted_hot_rows
                .saturating_add(deleted.rows_affected());

            let rollup_retention_sql = format!(
                "DELETE FROM {} WHERE org_id = $1 AND bucket_at_micros < $2",
                hourly_table(kind)
            );
            let deleted = sqlx::query(&rollup_retention_sql)
                .bind(request.org_id.as_str())
                .bind(request.rollup_retention_cutoff.0)
                .execute(&mut *tx)
                .await
                .map_err(sqlx_err)?;
            stats.deleted_rollup_rows = stats
                .deleted_rollup_rows
                .saturating_add(deleted.rows_affected());
        }

        sqlx::query(
            "UPDATE apm_projection_state
             SET last_rollup_bucket_at_micros = GREATEST(
                COALESCE(last_rollup_bucket_at_micros, $2), $2
             )
             WHERE org_id = $1",
        )
        .bind(request.org_id.as_str())
        .bind(request.hour_at.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        tx.commit().await.map_err(sqlx_err)?;
        Ok(stats)
    }
}

async fn load_source_rows(
    tx: &mut sqlx::TracedTransaction<'_>,
    table: &str,
    org_id: &str,
    hour_start: i64,
    hour_end: i64,
) -> Result<Vec<PersistedBucketRow>> {
    let sql = format!(
        "SELECT bucket_at_micros, dimension_key, histogram_schema_version,
                dimension, measurements
         FROM {table}
         WHERE org_id = $1 AND bucket_at_micros >= $2 AND bucket_at_micros < $3
         ORDER BY bucket_at_micros, dimension_key, owner_id
         FOR UPDATE"
    );
    sqlx::query(&sql)
        .bind(org_id)
        .bind(hour_start)
        .bind(hour_end)
        .fetch_all(&mut **tx)
        .await
        .map_err(sqlx_err)?
        .into_iter()
        .map(row_to_bucket)
        .collect()
}

async fn upsert_rollup(
    tx: &mut sqlx::TracedTransaction<'_>,
    table: &str,
    request: &RollupRequest,
    rollup: &RollupBucket,
    completed_at: TimestampMicros,
) -> Result<()> {
    let service = rollup.dimension.service();
    let sql = format!(
        "INSERT INTO {table} (
            org_id, service_namespace, service_name, environment, version,
            bucket_at_micros, persistence_schema_version, histogram_schema_version,
            dimension_key, dimension, measurements, source_minute_count,
            completed_at_micros
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
         ON CONFLICT (
            org_id, bucket_at_micros, dimension_key, histogram_schema_version
         ) DO UPDATE SET
            service_namespace = EXCLUDED.service_namespace,
            service_name = EXCLUDED.service_name,
            environment = EXCLUDED.environment,
            version = EXCLUDED.version,
            persistence_schema_version = EXCLUDED.persistence_schema_version,
            dimension = EXCLUDED.dimension,
            measurements = EXCLUDED.measurements,
            source_minute_count = EXCLUDED.source_minute_count,
            completed_at_micros = EXCLUDED.completed_at_micros"
    );
    sqlx::query(&sql)
        .bind(request.org_id.as_str())
        .bind(&service.namespace)
        .bind(&service.name)
        .bind(&service.environment)
        .bind(rollup.dimension.version().unwrap_or_default())
        .bind(request.hour_at.0)
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
        .bind(completed_at.0)
        .execute(&mut **tx)
        .await
        .map_err(sqlx_err)?;
    Ok(())
}
