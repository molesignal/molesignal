// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use super::sqlx_err;
use crate::{
    domain::intake::usage::source::{
        IntakeUsageBucket, IntakeUsageRepository, IntakeUsageRow, SourceUsage, UsageDimension,
        UsageSelection, hour_bucket_start,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

mod budgets;

pub use budgets::PgIntakeBudgetRepository;

pub struct PgIntakeUsageRepository {
    pool: PgPool,
}

impl PgIntakeUsageRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// The text a row is told apart by in a dimension; the same as the source key in the domain.
fn source_expression(dimension: UsageDimension) -> &'static str {
    match dimension {
        UsageDimension::Credential => "credential_kind || ':' || credential_id",
        UsageDimension::Stream => "stream_type || ':' || stream",
        UsageDimension::Service => "service",
    }
}

/// The condition of a selection, and the source it binds as `$4` if it names one.
fn selection_filter<'a>(selection: &UsageSelection<'a>) -> (String, Option<&'a str>) {
    let mut filter = "org_id = $1 AND bucket_start >= $2 AND bucket_start <= $3".to_string();

    match selection.source {
        Some((dimension, key)) => {
            filter.push_str(&format!(" AND {} = $4", source_expression(dimension)));
            (filter, Some(key))
        }
        None => (filter, None),
    }
}

fn to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn to_u64(value: i64) -> u64 {
    value.max(0) as u64
}

#[async_trait]
impl IntakeUsageRepository for PgIntakeUsageRepository {
    async fn add(&self, rows: &[IntakeUsageRow]) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }

        let mut org_ids = Vec::with_capacity(rows.len());
        let mut buckets = Vec::with_capacity(rows.len());
        let mut kinds = Vec::with_capacity(rows.len());
        let mut credential_ids = Vec::with_capacity(rows.len());
        let mut stream_types = Vec::with_capacity(rows.len());
        let mut streams = Vec::with_capacity(rows.len());
        let mut services = Vec::with_capacity(rows.len());
        let mut bytes = Vec::with_capacity(rows.len());
        let mut events = Vec::with_capacity(rows.len());

        for row in rows {
            org_ids.push(row.org_id.as_str());
            buckets.push(TimestampMicros(row.bucket_start_micros));
            kinds.push(row.credential.kind.as_str());
            credential_ids.push(row.credential.id.as_str());
            stream_types.push(row.stream_type.external_name());
            streams.push(row.stream.as_str());
            services.push(row.service.as_str());
            bytes.push(to_i64(row.intake_bytes));
            events.push(to_i64(row.events));
        }

        sqlx::query(
            "INSERT INTO intake_usage_hourly (
                 org_id, bucket_start, credential_kind, credential_id,
                 stream_type, stream, service, intake_bytes, events
             )
             SELECT * FROM UNNEST(
                 $1::VARCHAR[], $2::TIMESTAMPTZ[], $3::VARCHAR[], $4::VARCHAR[],
                 $5::VARCHAR[], $6::VARCHAR[], $7::VARCHAR[], $8::BIGINT[], $9::BIGINT[]
             )
             ON CONFLICT (
                 org_id, bucket_start, credential_kind, credential_id,
                 stream_type, stream, service
             ) DO UPDATE
             SET intake_bytes = intake_usage_hourly.intake_bytes + EXCLUDED.intake_bytes,
                 events = intake_usage_hourly.events + EXCLUDED.events",
        )
        .bind(&org_ids)
        .bind(&buckets)
        .bind(&kinds)
        .bind(&credential_ids)
        .bind(&stream_types)
        .bind(&streams)
        .bind(&services)
        .bind(&bytes)
        .bind(&events)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn by_source(
        &self,
        dimension: UsageDimension,
        selection: UsageSelection<'_>,
    ) -> Result<Vec<SourceUsage>> {
        let (filter, source) = selection_filter(&selection);

        let statement = format!(
            "SELECT {} AS source_key,
                    SUM(intake_bytes)::BIGINT AS intake_bytes,
                    SUM(events)::BIGINT AS events
             FROM intake_usage_hourly
             WHERE {filter}
             GROUP BY 1
             ORDER BY 2 DESC, 1",
            source_expression(dimension),
        );

        let mut query = sqlx::query(&statement)
            .bind(selection.org_id.as_str())
            .bind(TimestampMicros(hour_bucket_start(selection.start_micros)))
            .bind(TimestampMicros(selection.end_micros));

        if let Some(source) = source {
            query = query.bind(source);
        }

        let rows = query.fetch_all(&self.pool).await.map_err(sqlx_err)?;

        rows.into_iter()
            .map(|row| {
                Ok(SourceUsage {
                    source_key: row.try_get("source_key").map_err(sqlx_err)?,
                    intake_bytes: to_u64(row.try_get("intake_bytes").map_err(sqlx_err)?),
                    events: to_u64(row.try_get("events").map_err(sqlx_err)?),
                })
            })
            .collect()
    }

    async fn hourly(&self, selection: UsageSelection<'_>) -> Result<Vec<IntakeUsageBucket>> {
        let (filter, source) = selection_filter(&selection);

        let statement = format!(
            "SELECT bucket_start,
                    SUM(intake_bytes)::BIGINT AS intake_bytes,
                    SUM(events)::BIGINT AS events
             FROM intake_usage_hourly
             WHERE {filter}
             GROUP BY bucket_start
             ORDER BY bucket_start ASC",
        );

        let mut query = sqlx::query(&statement)
            .bind(selection.org_id.as_str())
            .bind(TimestampMicros(hour_bucket_start(selection.start_micros)))
            .bind(TimestampMicros(selection.end_micros));

        if let Some(source) = source {
            query = query.bind(source);
        }

        let rows = query.fetch_all(&self.pool).await.map_err(sqlx_err)?;

        rows.into_iter()
            .map(|row| {
                Ok(IntakeUsageBucket {
                    bucket_start_micros: row
                        .try_get::<TimestampMicros, _>("bucket_start")
                        .map_err(sqlx_err)?
                        .0,
                    intake_bytes: to_u64(row.try_get("intake_bytes").map_err(sqlx_err)?),
                    events: to_u64(row.try_get("events").map_err(sqlx_err)?),
                })
            })
            .collect()
    }

    async fn first_bucket(&self, org_id: &Id) -> Result<Option<i64>> {
        let row = sqlx::query(
            "SELECT MIN(bucket_start) AS first_bucket
             FROM intake_usage_hourly
             WHERE org_id = $1",
        )
        .bind(org_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;

        row.try_get::<Option<TimestampMicros>, _>("first_bucket")
            .map(|first| first.map(|first| first.0))
            .map_err(sqlx_err)
    }

    async fn delete_before(&self, before_micros: i64, limit: i64) -> Result<u64> {
        // ctid：一次只锁住一批行，不让一次清理长时间占着表。子查询走 `idx_intake_usage_hourly_bucket`。
        let result = sqlx::query(
            "DELETE FROM intake_usage_hourly
             WHERE ctid = ANY(ARRAY(
                 SELECT ctid FROM intake_usage_hourly
                 WHERE bucket_start < $1
                 ORDER BY bucket_start
                 LIMIT $2
             ))",
        )
        .bind(TimestampMicros(before_micros))
        .bind(limit)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(result.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selection<'a>(
        org_id: &'a Id,
        source: Option<(UsageDimension, &'a str)>,
    ) -> (String, Option<&'a str>) {
        selection_filter(&UsageSelection {
            org_id,
            start_micros: 0,
            end_micros: 1,
            source,
        })
    }

    #[test]
    fn a_selection_binds_the_source_only_when_it_names_one() {
        let org_id = Id::from_string("org-1");
        let (filter, source) = selection(&org_id, None);

        assert!(!filter.contains("$4"));
        assert_eq!(source, None);

        let (filter, source) = selection(&org_id, Some((UsageDimension::Stream, "logs:app")));

        assert!(filter.ends_with("AND stream_type || ':' || stream = $4"));
        assert_eq!(source, Some("logs:app"));
    }

    #[test]
    fn counts_do_not_overflow_a_column() {
        assert_eq!(to_i64(u64::MAX), i64::MAX);
        assert_eq!(to_u64(-1), 0);
    }
}
