// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::Row;

use super::{
    PgApmRepository,
    codec::{hourly_table, merge_rows, minute_table, resolve_resolution, row_to_bucket},
};
use crate::{
    domain::apm::{
        ApmQueryRepository, BucketQuery, CatalogQuery, ErrorGroupQuery, ErrorGroupRecord,
        ErrorSample, MergedBucket, ProjectionGap, ProjectionGapReason, ProjectionState,
        QueryResolution, ServiceObservation, VersionObservation,
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
            QueryResolution::Minute => minute_table(query.kind),
            QueryResolution::Hour => hourly_table(query.kind),
            QueryResolution::Auto => unreachable!("auto resolution is resolved above"),
        };
        let sql = format!(
            "SELECT bucket_at_micros, dimension_key, histogram_schema_version,
                    dimension, measurements
             FROM {table}
             WHERE org_id = $1
               AND bucket_at_micros BETWEEN $2 AND $3
               AND ($4::TEXT IS NULL OR service_namespace = $4)
               AND ($5::TEXT IS NULL OR service_name = $5)
               AND ($6::TEXT IS NULL OR environment = $6)
               AND ($7::TEXT IS NULL OR version = $7)
             ORDER BY bucket_at_micros ASC, dimension_key ASC,
                      histogram_schema_version ASC"
        );
        let rows = sqlx::query(&sql)
            .bind(query.org_id.as_str())
            .bind(query.range.start.0)
            .bind(query.range.end.0)
            .bind(query.namespace.as_deref())
            .bind(query.service_name.as_deref())
            .bind(query.environment.as_deref())
            .bind(query.version.as_deref())
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
            "SELECT projection_started_at_micros,
                    last_complete_bucket_at_micros,
                    last_rollup_bucket_at_micros
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
            "SELECT range_start_micros, range_end_micros, reason,
                    dropped_facts, recorded_at_micros
             FROM apm_projection_gaps
             WHERE org_id = $1
               AND range_start_micros <= $2
               AND range_end_micros >= $3
             ORDER BY range_start_micros ASC, range_end_micros ASC",
        )
        .bind(org_id.as_str())
        .bind(range.end.0)
        .bind(range.start.0)
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
                        TimestampMicros(row.try_get("range_start_micros").map_err(sqlx_err)?),
                        TimestampMicros(row.try_get("range_end_micros").map_err(sqlx_err)?),
                    ),
                    reason,
                    dropped_facts: u64::try_from(dropped)
                        .map_err(|_| Error::internal("negative APM dropped fact count"))?,
                    recorded_at: TimestampMicros(
                        row.try_get("recorded_at_micros").map_err(sqlx_err)?,
                    ),
                })
            })
            .collect()
    }
}
