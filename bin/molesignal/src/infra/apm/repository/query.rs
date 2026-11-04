// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::Row;

use super::{
    PgApmRepository,
    codec::{kind_name, merge_rows, resolve_resolution, row_to_bucket},
};
use crate::{
    domain::apm::{
        ApmQueryRepository, BucketQuery, CatalogQuery, ErrorGroupQuery, ErrorGroupRecord,
        ErrorSample, InstanceObservation, MergedBucket, ProjectionGap, ProjectionGapReason,
        ProjectionState, QueryResolution, ServiceObservation, VersionObservation,
    },
    infra::persistence::sqlx_err,
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

#[async_trait]
impl ApmQueryRepository for PgApmRepository {
    #[tracing::instrument(
        name = "db.query",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "SELECT", db.collection.name = "apm_buckets")
    )]
    async fn query_buckets(&self, query: &BucketQuery) -> Result<Vec<MergedBucket>> {
        if query.range.end.0 < query.range.start.0 {
            return Err(Error::invalid("APM query range end precedes start"));
        }

        let resolution = resolve_resolution(query.resolution, query.range.duration_micros());

        let table = match resolution {
            QueryResolution::Minute => "apm_buckets",
            QueryResolution::Hour => "apm_buckets_hourly",
            QueryResolution::Auto => unreachable!("auto resolution is resolved above"),
        };

        let sql = format!(
            "SELECT bucket_at, dimension_key, histogram_schema_version,
                    dimension, measurements
             FROM {table}
             WHERE org_id = $1
               AND bucket_kind = $8
               AND bucket_at BETWEEN $2 AND $3
               AND ($4::TEXT IS NULL OR service_namespace = $4)
               AND ($5::TEXT IS NULL OR service_name = $5)
               AND ($6::TEXT IS NULL OR environment = $6)
               AND ($7::TEXT IS NULL OR version = $7)
             ORDER BY bucket_at ASC, dimension_key ASC,
                      histogram_schema_version ASC"
        );

        let rows = sqlx::query(&sql)
            .bind(query.org_id.as_str())
            .bind(query.range.start)
            .bind(query.range.end)
            .bind(query.namespace.as_deref())
            .bind(query.service_name.as_deref())
            .bind(query.environment.as_deref())
            .bind(query.version.as_deref())
            .bind(kind_name(query.kind))
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;

        let rows = rows
            .into_iter()
            .map(row_to_bucket)
            .collect::<Result<Vec<_>>>()?;

        merge_rows(rows)
    }

    async fn list_services(&self, query: &CatalogQuery) -> Result<Vec<ServiceObservation>> {
        self.query_services(query).await
    }

    async fn list_versions(&self, query: &CatalogQuery) -> Result<Vec<VersionObservation>> {
        self.query_versions(query).await
    }

    async fn list_instances(&self, query: &CatalogQuery) -> Result<Vec<InstanceObservation>> {
        super::instance::list(&self.pool, query).await
    }

    async fn list_error_groups(&self, query: &ErrorGroupQuery) -> Result<Vec<ErrorGroupRecord>> {
        self.query_error_groups(query).await
    }

    async fn list_error_samples(
        &self,
        org_id: &crate::shared::ids::Id,
        fingerprint: &str,
    ) -> Result<Vec<ErrorSample>> {
        self.query_error_samples(org_id, fingerprint).await
    }

    async fn projection_state(
        &self,
        org_id: &crate::shared::ids::Id,
    ) -> Result<Option<ProjectionState>> {
        let row = sqlx::query(
            "SELECT projection_started_at,
                    last_complete_bucket_at,
                    last_rollup_bucket_at
             FROM apm_projection_state
             WHERE org_id = $1",
        )
        .bind(org_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        row.map(|row| {
            Ok(ProjectionState {
                org_id: org_id.clone(),
                projection_started_at: row
                    .try_get::<TimestampMicros, _>("projection_started_at")
                    .map_err(sqlx_err)?,
                last_complete_bucket_at: row
                    .try_get::<Option<TimestampMicros>, _>("last_complete_bucket_at")
                    .map_err(sqlx_err)?,
                last_rollup_bucket_at: row
                    .try_get::<Option<TimestampMicros>, _>("last_rollup_bucket_at")
                    .map_err(sqlx_err)?,
            })
        })
        .transpose()
    }

    async fn projection_gaps(
        &self,
        org_id: &crate::shared::ids::Id,
        range: TimeRange,
    ) -> Result<Vec<ProjectionGap>> {
        if range.end.0 < range.start.0 {
            return Err(Error::invalid("APM gap range end precedes start"));
        }

        let rows = sqlx::query(
            "SELECT range_start, range_end, reason,
                    dropped_facts, recorded_at
             FROM apm_projection_gaps
             WHERE org_id = $1
               AND range_start <= $2
               AND range_end >= $3
             ORDER BY range_start ASC, range_end ASC",
        )
        .bind(org_id.as_str())
        .bind(range.end)
        .bind(range.start)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter()
            .map(|row| {
                let reason: String = row.try_get("reason").map_err(sqlx_err)?;
                let reason = ProjectionGapReason::parse(&reason)
                    .ok_or_else(|| Error::internal("unknown persisted APM gap reason"))?;
                let dropped: i64 = row.try_get("dropped_facts").map_err(sqlx_err)?;

                Ok(ProjectionGap {
                    org_id: org_id.clone(),
                    range: TimeRange::new(
                        row.try_get::<TimestampMicros, _>("range_start")
                            .map_err(sqlx_err)?,
                        row.try_get::<TimestampMicros, _>("range_end")
                            .map_err(sqlx_err)?,
                    ),
                    reason,
                    dropped_facts: u64::try_from(dropped)
                        .map_err(|_| Error::internal("negative APM dropped fact count"))?,
                    recorded_at: row
                        .try_get::<TimestampMicros, _>("recorded_at")
                        .map_err(sqlx_err)?,
                })
            })
            .collect()
    }
}
