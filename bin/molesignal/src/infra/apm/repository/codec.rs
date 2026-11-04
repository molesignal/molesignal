// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::{BTreeMap, BTreeSet};

use sqlx::{
    Row,
    postgres::{PgRow, types::PgInterval},
    types::Json,
};

use crate::{
    domain::apm::{BucketDimension, BucketKind, BucketMeasurements, MergedBucket, QueryResolution},
    shared::{Error, Result, time::TimestampMicros},
};

pub(super) const HOUR_MICROS: i64 = 3_600 * 1_000_000;
pub(super) const HOT_RESOLUTION_MICROS: i64 = 24 * HOUR_MICROS;

/// 阈值一类的时长按 `INTERVAL` 存取，精度为微秒。
pub(super) fn interval(micros: u64) -> PgInterval {
    PgInterval {
        months: 0,
        days: 0,
        microseconds: i64::try_from(micros).unwrap_or(i64::MAX),
    }
}

pub(super) fn interval_micros(interval: PgInterval) -> Option<u64> {
    let days = i64::from(interval.months)
        .saturating_mul(30)
        .saturating_add(i64::from(interval.days));
    let micros = days
        .saturating_mul(24 * HOUR_MICROS)
        .saturating_add(interval.microseconds);

    u64::try_from(micros).ok()
}

type RollupMergeMap =
    BTreeMap<(Vec<u8>, u16), (BucketDimension, BucketMeasurements, BTreeSet<i64>)>;

#[derive(Debug, Clone)]
pub(super) struct PersistedBucketRow {
    pub bucket_at: i64,
    pub dimension_key: Vec<u8>,
    pub histogram_schema_version: u16,
    pub dimension: BucketDimension,
    pub measurements: BucketMeasurements,
}

pub(super) struct RollupBucket {
    pub dimension_key: Vec<u8>,
    pub histogram_schema_version: u16,
    pub dimension: BucketDimension,
    pub measurements: BucketMeasurements,
    pub source_minute_count: u16,
}

pub(super) fn kind_name(kind: BucketKind) -> &'static str {
    match kind {
        BucketKind::Service => "service",
        BucketKind::Transaction => "transaction",
        BucketKind::Dependency => "dependency",
        BucketKind::Error => "error",
        BucketKind::Instance => "instance",
    }
}

pub(super) fn resolve_resolution(
    resolution: QueryResolution,
    range_micros: i64,
) -> QueryResolution {
    match resolution {
        QueryResolution::Auto if range_micros > HOT_RESOLUTION_MICROS => QueryResolution::Hour,
        QueryResolution::Auto => QueryResolution::Minute,
        explicit => explicit,
    }
}

pub(super) fn dimension_key(dimension: &BucketDimension) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(dimension)
        .map_err(|error| Error::internal(format!("serialize APM dimension: {error}")))?;
    Ok(blake3::hash(&bytes).as_bytes().to_vec())
}

pub(super) fn row_to_bucket(row: PgRow) -> Result<PersistedBucketRow> {
    let schema_version: i16 = row
        .try_get("histogram_schema_version")
        .map_err(crate::infra::persistence::sqlx_err)?;

    let schema_version = u16::try_from(schema_version)
        .map_err(|_| Error::internal("negative APM histogram schema version"))?;

    let dimension: Json<BucketDimension> = row
        .try_get("dimension")
        .map_err(crate::infra::persistence::sqlx_err)?;

    let measurements: Json<BucketMeasurements> = row
        .try_get("measurements")
        .map_err(crate::infra::persistence::sqlx_err)?;

    if measurements.0.latency.schema_version != schema_version {
        return Err(Error::internal(
            "APM measurement histogram schema does not match row metadata",
        ));
    }

    let dimension_key: Vec<u8> = row
        .try_get("dimension_key")
        .map_err(crate::infra::persistence::sqlx_err)?;

    if dimension_key.len() != blake3::OUT_LEN {
        return Err(Error::internal("invalid APM dimension key length"));
    }

    Ok(PersistedBucketRow {
        bucket_at: row
            .try_get::<TimestampMicros, _>("bucket_at")
            .map_err(crate::infra::persistence::sqlx_err)?
            .0,
        dimension_key,
        histogram_schema_version: schema_version,
        dimension: dimension.0,
        measurements: measurements.0,
    })
}

pub(super) fn merge_rows(rows: Vec<PersistedBucketRow>) -> Result<Vec<MergedBucket>> {
    let mut merged: BTreeMap<(i64, Vec<u8>, u16), (BucketDimension, BucketMeasurements)> =
        BTreeMap::new();

    for row in rows {
        let key = (
            row.bucket_at,
            row.dimension_key,
            row.histogram_schema_version,
        );

        match merged.entry(key) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert((row.dimension, row.measurements));
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                if entry.get().0 != row.dimension {
                    return Err(Error::internal("APM dimension key collision"));
                }

                entry.get_mut().1.merge(&row.measurements)?;
            }
        }
    }

    Ok(merged
        .into_iter()
        .map(
            |((bucket_at, _, _), (dimension, measurements))| MergedBucket {
                bucket_at: TimestampMicros(bucket_at),
                dimension,
                measurements,
            },
        )
        .collect())
}

pub(super) fn rollup_rows(rows: Vec<PersistedBucketRow>) -> Result<Vec<RollupBucket>> {
    let mut merged = RollupMergeMap::new();

    for row in rows {
        let key = (row.dimension_key.clone(), row.histogram_schema_version);

        match merged.entry(key) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert((
                    row.dimension,
                    row.measurements,
                    BTreeSet::from([row.bucket_at]),
                ));
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                if entry.get().0 != row.dimension {
                    return Err(Error::internal("APM dimension key collision"));
                }

                entry.get_mut().1.merge(&row.measurements)?;
                entry.get_mut().2.insert(row.bucket_at);
            }
        }
    }

    merged
        .into_iter()
        .map(
            |((dimension_key, histogram_schema_version), (dimension, measurements, minutes))| {
                Ok(RollupBucket {
                    dimension_key,
                    histogram_schema_version,
                    dimension,
                    measurements,
                    source_minute_count: u16::try_from(minutes.len())
                        .map_err(|_| Error::internal("APM rollup exceeds 60 minute buckets"))?,
                })
            },
        )
        .collect()
}

pub(super) fn as_i64(value: u64, field: &str) -> Result<i64> {
    i64::try_from(value)
        .map_err(|_| Error::resource_exhausted(format!("APM {field} exceeds BIGINT")))
}

pub(super) fn as_i16(value: u16, field: &str) -> Result<i16> {
    i16::try_from(value).map_err(|_| Error::invalid(format!("APM {field} exceeds SMALLINT")))
}

pub(super) fn gap_id(parts: &[&[u8]]) -> String {
    let mut hasher = blake3::Hasher::new();

    for part in parts {
        hasher.update(&(part.len() as u64).to_le_bytes());
        hasher.update(part);
    }

    hasher.finalize().to_hex().to_string()
}
