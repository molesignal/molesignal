// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::{Postgres, QueryBuilder, Row, types::Json};

use super::{
    PgApmRepository,
    codec::{as_i16, as_i64, dimension_key, gap_id, minute_table},
};
use crate::{
    domain::apm::{
        ApmWriteRepository, BucketKind, ErrorGroupRecord, ErrorSample, OwnerSnapshot,
        ProjectionGap, ProjectionState, ServiceObservation, SnapshotWriteStats, VersionObservation,
    },
    infra::persistence::sqlx_err,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const SNAPSHOT_BIND_COUNT: usize = 14;
const POSTGRES_BIND_LIMIT: usize = 65_535;
const SNAPSHOT_BATCH_SIZE: usize = POSTGRES_BIND_LIMIT / SNAPSHOT_BIND_COUNT;
const BUCKET_KINDS: [BucketKind; 4] = [
    BucketKind::Service,
    BucketKind::Transaction,
    BucketKind::Dependency,
    BucketKind::Error,
];

struct PreparedSnapshot<'a> {
    snapshot: &'a OwnerSnapshot,
    dimension_key: Vec<u8>,
    snapshot_seq: i64,
    persistence_schema_version: i16,
    histogram_schema_version: i16,
}

#[async_trait]
impl ApmWriteRepository for PgApmRepository {
    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "apm_services")
    )]
    async fn upsert_catalog(
        &self,
        services: &[ServiceObservation],
        versions: &[VersionObservation],
    ) -> Result<()> {
        if services.is_empty() && versions.is_empty() {
            return Ok(());
        }
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        for service in services {
            sqlx::query(
                "INSERT INTO apm_services (
                    org_id, service_namespace, service_name, environment,
                    first_seen_at_micros, last_seen_at_micros, runtime_language,
                    telemetry_sdk_name, telemetry_sdk_version, recent_instance_count
                 ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
                 ON CONFLICT (org_id, service_namespace, service_name, environment)
                 DO UPDATE SET
                    first_seen_at_micros = LEAST(
                        apm_services.first_seen_at_micros, EXCLUDED.first_seen_at_micros
                    ),
                    last_seen_at_micros = GREATEST(
                        apm_services.last_seen_at_micros, EXCLUDED.last_seen_at_micros
                    ),
                    runtime_language = COALESCE(
                        EXCLUDED.runtime_language, apm_services.runtime_language
                    ),
                    telemetry_sdk_name = COALESCE(
                        EXCLUDED.telemetry_sdk_name, apm_services.telemetry_sdk_name
                    ),
                    telemetry_sdk_version = COALESCE(
                        EXCLUDED.telemetry_sdk_version, apm_services.telemetry_sdk_version
                    ),
                    recent_instance_count = EXCLUDED.recent_instance_count",
            )
            .bind(service.org_id.as_str())
            .bind(&service.service.namespace)
            .bind(&service.service.name)
            .bind(&service.service.environment)
            .bind(service.first_seen_at.0)
            .bind(service.last_seen_at.0)
            .bind(service.runtime_language.as_deref())
            .bind(service.telemetry_sdk_name.as_deref())
            .bind(service.telemetry_sdk_version.as_deref())
            .bind(i32::try_from(service.recent_instance_count).map_err(|_| {
                Error::resource_exhausted("APM recent instance count exceeds INTEGER")
            })?)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        }
        for version in versions {
            sqlx::query(
                "INSERT INTO apm_service_versions (
                    org_id, service_namespace, service_name, environment, version,
                    first_seen_at_micros, last_seen_at_micros, observation_count
                 ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
                 ON CONFLICT (
                    org_id, service_namespace, service_name, environment, version
                 ) DO UPDATE SET
                    first_seen_at_micros = LEAST(
                        apm_service_versions.first_seen_at_micros,
                        EXCLUDED.first_seen_at_micros
                    ),
                    last_seen_at_micros = GREATEST(
                        apm_service_versions.last_seen_at_micros,
                        EXCLUDED.last_seen_at_micros
                    ),
                    observation_count = GREATEST(
                        apm_service_versions.observation_count,
                        EXCLUDED.observation_count
                    )",
            )
            .bind(version.org_id.as_str())
            .bind(&version.service.namespace)
            .bind(&version.service.name)
            .bind(&version.service.environment)
            .bind(&version.version)
            .bind(version.first_seen_at.0)
            .bind(version.last_seen_at.0)
            .bind(as_i64(
                version.observation_count,
                "version observation count",
            )?)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        }
        tx.commit().await.map_err(sqlx_err)?;
        Ok(())
    }

    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "UPSERT", db.collection.name = "apm_owner_buckets")
    )]
    async fn replace_owner_snapshots(
        &self,
        snapshots: &[OwnerSnapshot],
    ) -> Result<SnapshotWriteStats> {
        if snapshots.is_empty() {
            return Ok(SnapshotWriteStats::default());
        }
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        let mut stats = SnapshotWriteStats {
            attempted: snapshots.len() as u64,
            ..SnapshotWriteStats::default()
        };
        let prepared = snapshots
            .iter()
            .map(|snapshot| {
                Ok(PreparedSnapshot {
                    snapshot,
                    dimension_key: dimension_key(&snapshot.dimension)?,
                    snapshot_seq: as_i64(snapshot.snapshot_seq, "snapshot sequence")?,
                    persistence_schema_version: as_i16(
                        snapshot.schema_version,
                        "persistence schema version",
                    )?,
                    histogram_schema_version: as_i16(
                        snapshot.measurements.latency.schema_version,
                        "histogram schema version",
                    )?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        for kind in BUCKET_KINDS {
            let table = minute_table(kind);
            let rows = prepared
                .iter()
                .filter(|row| row.snapshot.dimension.kind() == kind)
                .collect::<Vec<_>>();
            for chunk in rows.chunks(SNAPSHOT_BATCH_SIZE) {
                let mut query = QueryBuilder::<Postgres>::new(format!(
                    "INSERT INTO {table} (
                    org_id, service_namespace, service_name, environment, version,
                    bucket_at_micros, owner_id, snapshot_seq,
                    persistence_schema_version, histogram_schema_version,
                    dimension_key, dimension, measurements, updated_at_micros
                 ) "
                ));
                query.push_values(chunk, |mut row, prepared| {
                    let snapshot = prepared.snapshot;
                    let service = snapshot.dimension.service();
                    row.push_bind(snapshot.org_id.as_str())
                        .push_bind(&service.namespace)
                        .push_bind(&service.name)
                        .push_bind(&service.environment)
                        .push_bind(snapshot.dimension.version().unwrap_or_default())
                        .push_bind(snapshot.bucket_at.0)
                        .push_bind(&snapshot.owner_id)
                        .push_bind(prepared.snapshot_seq)
                        .push_bind(prepared.persistence_schema_version)
                        .push_bind(prepared.histogram_schema_version)
                        .push_bind(&prepared.dimension_key)
                        .push_bind(Json(&snapshot.dimension))
                        .push_bind(Json(&snapshot.measurements))
                        .push_bind(snapshot.updated_at.0);
                });
                query.push(format!(
                    " ON CONFLICT (
                    org_id, bucket_at_micros, dimension_key, owner_id,
                    histogram_schema_version
                 ) DO UPDATE SET
                    service_namespace = EXCLUDED.service_namespace,
                    service_name = EXCLUDED.service_name,
                    environment = EXCLUDED.environment,
                    version = EXCLUDED.version,
                    snapshot_seq = EXCLUDED.snapshot_seq,
                    persistence_schema_version = EXCLUDED.persistence_schema_version,
                    dimension = EXCLUDED.dimension,
                    measurements = EXCLUDED.measurements,
                    updated_at_micros = EXCLUDED.updated_at_micros
                 WHERE {table}.snapshot_seq < EXCLUDED.snapshot_seq"
                ));
                let result = query.build().execute(&mut *tx).await.map_err(sqlx_err)?;
                stats.applied = stats.applied.saturating_add(result.rows_affected());
            }
        }
        stats.stale = stats.attempted.saturating_sub(stats.applied);
        tx.commit().await.map_err(sqlx_err)?;
        Ok(stats)
    }

    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "UPSERT", db.collection.name = "apm_error_groups")
    )]
    async fn upsert_error_groups(
        &self,
        groups: &[ErrorGroupRecord],
        samples: &[ErrorSample],
        max_samples_per_group: usize,
    ) -> Result<()> {
        if max_samples_per_group == 0 {
            return Err(Error::invalid(
                "APM error sample capacity must be greater than zero",
            ));
        }
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        for group in groups {
            sqlx::query(
                "INSERT INTO apm_error_groups (
                    org_id, fingerprint, service_namespace, service_name, environment,
                    error_identity, first_seen_at_micros, last_seen_at_micros,
                    occurrence_count, representative_message, representative_stack
                 ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
                 ON CONFLICT (org_id, fingerprint) DO UPDATE SET
                    first_seen_at_micros = LEAST(
                        apm_error_groups.first_seen_at_micros,
                        EXCLUDED.first_seen_at_micros
                    ),
                    last_seen_at_micros = GREATEST(
                        apm_error_groups.last_seen_at_micros,
                        EXCLUDED.last_seen_at_micros
                    ),
                    occurrence_count = GREATEST(
                        apm_error_groups.occurrence_count, EXCLUDED.occurrence_count
                    ),
                    representative_message = COALESCE(
                        EXCLUDED.representative_message,
                        apm_error_groups.representative_message
                    ),
                    representative_stack = CASE
                        WHEN jsonb_array_length(EXCLUDED.representative_stack) > 0
                        THEN EXCLUDED.representative_stack
                        ELSE apm_error_groups.representative_stack
                    END",
            )
            .bind(group.org_id.as_str())
            .bind(&group.error.fingerprint)
            .bind(&group.service.namespace)
            .bind(&group.service.name)
            .bind(&group.service.environment)
            .bind(Json(&group.error))
            .bind(group.first_seen_at.0)
            .bind(group.last_seen_at.0)
            .bind(as_i64(group.occurrence_count, "error occurrence count")?)
            .bind(group.representative_message.as_deref())
            .bind(Json(&group.representative_stack))
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        }
        for sample in samples {
            let slot = sample_slot(sample, max_samples_per_group)?;
            sqlx::query(
                "INSERT INTO apm_error_samples (
                    org_id, fingerprint, sample_slot, event_time_micros, trace_id,
                    span_id, trace_available, representative_message, representative_stack
                 ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
                 ON CONFLICT (org_id, fingerprint, sample_slot) DO UPDATE SET
                    event_time_micros = EXCLUDED.event_time_micros,
                    trace_id = EXCLUDED.trace_id,
                    span_id = EXCLUDED.span_id,
                    trace_available = EXCLUDED.trace_available,
                    representative_message = EXCLUDED.representative_message,
                    representative_stack = EXCLUDED.representative_stack
                 WHERE apm_error_samples.event_time_micros <= EXCLUDED.event_time_micros",
            )
            .bind(sample.org_id.as_str())
            .bind(&sample.error.fingerprint)
            .bind(slot)
            .bind(sample.event_time.0)
            .bind(&sample.trace_id)
            .bind(&sample.span_id)
            .bind(sample.trace_available)
            .bind(sample.representative_message.as_deref())
            .bind(Json(&sample.representative_stack))
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        }
        tx.commit().await.map_err(sqlx_err)?;
        Ok(())
    }

    async fn record_projection_gaps(&self, gaps: &[ProjectionGap]) -> Result<()> {
        if gaps.is_empty() {
            return Ok(());
        }
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        for gap in gaps {
            let start = gap.range.start.0.to_le_bytes();
            let end = gap.range.end.0.to_le_bytes();
            let id = gap_id(&[
                gap.org_id.as_str().as_bytes(),
                &start,
                &end,
                gap.reason.as_str().as_bytes(),
            ]);
            sqlx::query(
                "INSERT INTO apm_projection_gaps (
                    id, org_id, range_start_micros, range_end_micros, reason,
                    dropped_facts, recorded_at_micros
                 ) VALUES ($1,$2,$3,$4,$5,$6,$7)
                 ON CONFLICT (org_id, id) DO UPDATE SET
                    dropped_facts = GREATEST(
                        apm_projection_gaps.dropped_facts, EXCLUDED.dropped_facts
                    ),
                    recorded_at_micros = GREATEST(
                        apm_projection_gaps.recorded_at_micros,
                        EXCLUDED.recorded_at_micros
                    )",
            )
            .bind(id)
            .bind(gap.org_id.as_str())
            .bind(gap.range.start.0)
            .bind(gap.range.end.0)
            .bind(gap.reason.as_str())
            .bind(as_i64(gap.dropped_facts, "dropped fact count")?)
            .bind(gap.recorded_at.0)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        }
        tx.commit().await.map_err(sqlx_err)?;
        Ok(())
    }

    async fn ensure_projection_started(
        &self,
        org_id: &Id,
        started_at: TimestampMicros,
    ) -> Result<ProjectionState> {
        let row = sqlx::query(
            "INSERT INTO apm_projection_state (
                org_id, projection_started_at_micros
             ) VALUES ($1,$2)
             ON CONFLICT (org_id) DO UPDATE SET
                projection_started_at_micros = LEAST(
                    apm_projection_state.projection_started_at_micros,
                    EXCLUDED.projection_started_at_micros
                )
             RETURNING projection_started_at_micros,
                       last_complete_bucket_at_micros,
                       last_rollup_bucket_at_micros",
        )
        .bind(org_id.as_str())
        .bind(started_at.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(ProjectionState {
            org_id: org_id.clone(),
            projection_started_at: TimestampMicros(
                row.try_get("projection_started_at_micros")
                    .map_err(sqlx_err)?,
            ),
            last_complete_bucket_at: row
                .try_get::<Option<i64>, _>("last_complete_bucket_at_micros")
                .map_err(sqlx_err)?
                .map(TimestampMicros),
            last_rollup_bucket_at: row
                .try_get::<Option<i64>, _>("last_rollup_bucket_at_micros")
                .map_err(sqlx_err)?
                .map(TimestampMicros),
        })
    }

    async fn advance_projection_complete(
        &self,
        org_id: &Id,
        bucket_at: TimestampMicros,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE apm_projection_state
             SET last_complete_bucket_at_micros = GREATEST(
                 COALESCE(last_complete_bucket_at_micros, $2), $2
             )
             WHERE org_id = $1",
        )
        .bind(org_id.as_str())
        .bind(bucket_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }
}

fn sample_slot(sample: &ErrorSample, capacity: usize) -> Result<i16> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(sample.trace_id.as_bytes());
    hasher.update(&[0]);
    hasher.update(sample.span_id.as_bytes());
    let bytes = hasher.finalize();
    let prefix: [u8; 8] = bytes.as_bytes()[..8]
        .try_into()
        .map_err(|_| Error::internal("APM sample hash prefix"))?;
    let slot = u64::from_le_bytes(prefix) % capacity as u64;
    i16::try_from(slot).map_err(|_| Error::invalid("APM error sample capacity exceeds SMALLINT"))
}
