// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::{BTreeMap, BTreeSet, HashSet};

use sqlx::{Row, postgres::PgRow, types::Json};

use crate::{
    domain::apm::{BucketDimension, BucketKind, BucketMeasurements, MergedBucket, QueryResolution},
    shared::{Error, Result, time::TimestampMicros},
};

pub(super) const HOUR_MICROS: i64 = 3_600 * 1_000_000;
pub(super) const HOT_RESOLUTION_MICROS: i64 = 24 * HOUR_MICROS;
const MERGED_EXEMPLAR_CAP: usize = 8;
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

pub(super) fn minute_table(kind: BucketKind) -> &'static str {
    match kind {
        BucketKind::Service => "apm_service_buckets",
        BucketKind::Transaction => "apm_transaction_buckets",
        BucketKind::Dependency => "apm_dependency_buckets",
        BucketKind::Error => "apm_error_buckets",
    }
}

pub(super) fn hourly_table(kind: BucketKind) -> &'static str {
    match kind {
        BucketKind::Service => "apm_service_buckets_hourly",
        BucketKind::Transaction => "apm_transaction_buckets_hourly",
        BucketKind::Dependency => "apm_dependency_buckets_hourly",
        BucketKind::Error => "apm_error_buckets_hourly",
    }
}

pub(super) fn kind_name(kind: BucketKind) -> &'static str {
    match kind {
        BucketKind::Service => "service",
        BucketKind::Transaction => "transaction",
        BucketKind::Dependency => "dependency",
        BucketKind::Error => "error",
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
            .try_get("bucket_at_micros")
            .map_err(crate::infra::persistence::sqlx_err)?,
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
                merge_measurements(&mut entry.get_mut().1, row.measurements)?;
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
                merge_measurements(&mut entry.get_mut().1, row.measurements)?;
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

fn merge_measurements(target: &mut BucketMeasurements, source: BucketMeasurements) -> Result<()> {
    target.request_count = target.request_count.saturating_add(source.request_count);
    target.error_count = target.error_count.saturating_add(source.error_count);
    target.overflow_count = target.overflow_count.saturating_add(source.overflow_count);
    target.latency.merge(&source.latency)?;

    let mut seen: HashSet<(String, String)> = target
        .exemplars
        .iter()
        .map(|item| (item.trace_id.clone(), item.span_id.clone()))
        .collect();
    for exemplar in source.exemplars {
        if target.exemplars.len() >= MERGED_EXEMPLAR_CAP {
            break;
        }
        if seen.insert((exemplar.trace_id.clone(), exemplar.span_id.clone())) {
            target.exemplars.push(exemplar);
        }
    }
    Ok(())
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
