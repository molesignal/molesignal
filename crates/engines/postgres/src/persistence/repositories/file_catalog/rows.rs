// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! PG 行 ↔ 领域结构映射。
//!
//! Segment 与 Artifact 分表存储；快照读取后按 `segment_id` 归并，
//! 主数据 Artifact 缺失视为不变量破坏直接报内部错误。

use sqlx::{Row, postgres::PgRow, types::Json};

use super::sqlx_err;
use crate::{
    domain::storage::{
        Artifact, ArtifactId, ArtifactRole, ArtifactState, ColumnStats, DataSegment, DatasetState,
        FlushId, IndexPolicy, ObjectChecksum, ObjectKey, Partition, PartitionPolicy,
        PhysicalDataset, PhysicalDatasetId, SchemaFingerprint, SegmentId, SegmentState,
        SequenceRange, StoragePolicy, StoredObject, WalSequence,
    },
    shared::{Error, Result, ids::Id, time::TimeRange},
};

pub(super) const DATASET_COLS: &str = "org_id, id, logical_stream_id, dataset_type, \
     dataset_type_version, partition_policy, storage_policy, index_policy, catalog_version, \
     state, created_at_micros, updated_at_micros";

pub(super) const SEGMENT_COLS: &str = "org_id, id, dataset_id, partition_start_micros, \
     partition_end_micros, partition_shard, min_event_micros, max_event_micros, row_count, \
     schema_fingerprint, column_stats, flush_id, sequence_start, sequence_end, output_ordinal, \
     state, visible_from_version, retired_at_version, created_at_micros";

pub(super) const ARTIFACT_COLS: &str = "org_id, id, segment_id, role, artifact_type, \
     format_version, object_key, size_bytes, checksum, etag, source_artifact_id, \
     source_checksum, schema_fingerprint, state, failure_reason, created_at_micros, \
     updated_at_micros";

pub(super) fn to_i64(value: u64, what: &str) -> Result<i64> {
    i64::try_from(value).map_err(|_| Error::invalid(format!("{what} {value} exceeds BIGINT")))
}

pub(super) fn to_u64(value: i64, what: &str) -> Result<u64> {
    u64::try_from(value).map_err(|_| Error::internal(format!("negative {what} {value} in catalog")))
}

pub(super) fn dataset_from_row(row: &PgRow) -> Result<PhysicalDataset> {
    let partition_policy: Json<PartitionPolicy> =
        row.try_get("partition_policy").map_err(sqlx_err)?;
    let storage_policy: Json<StoragePolicy> = row.try_get("storage_policy").map_err(sqlx_err)?;
    let index_policy: Json<IndexPolicy> = row.try_get("index_policy").map_err(sqlx_err)?;
    let state: String = row.try_get("state").map_err(sqlx_err)?;
    let kind: String = row.try_get("dataset_type").map_err(sqlx_err)?;
    Ok(PhysicalDataset {
        id: PhysicalDatasetId::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        organization_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        logical_stream_id: Id::from_string(
            row.try_get::<String, _>("logical_stream_id")
                .map_err(sqlx_err)?,
        ),
        dataset_type: kind.parse()?,
        dataset_type_version: row
            .try_get::<i32, _>("dataset_type_version")
            .map_err(sqlx_err)? as u32,
        partition_policy: partition_policy.0,
        storage_policy: storage_policy.0,
        index_policy: index_policy.0,
        catalog_version: to_u64(
            row.try_get::<i64, _>("catalog_version").map_err(sqlx_err)?,
            "catalog_version",
        )?,
        state: row_state::<DatasetState>(&state)?,
        created_at_micros: row.try_get("created_at_micros").map_err(sqlx_err)?,
        updated_at_micros: row.try_get("updated_at_micros").map_err(sqlx_err)?,
    })
}

fn row_state<T: std::str::FromStr<Err = Error>>(value: &str) -> Result<T> {
    value.parse()
}

/// artifacts 行 → (所属 segment, Artifact)。
pub(super) fn artifact_from_row(row: &PgRow) -> Result<(SegmentId, Artifact)> {
    let segment_id =
        SegmentId::from_string(row.try_get::<String, _>("segment_id").map_err(sqlx_err)?);
    let role: String = row.try_get("role").map_err(sqlx_err)?;
    let kind: String = row.try_get("artifact_type").map_err(sqlx_err)?;
    let state: String = row.try_get("state").map_err(sqlx_err)?;
    let artifact = Artifact {
        id: ArtifactId::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        role: role.parse()?,
        artifact_type: kind.parse()?,
        format_version: row.try_get::<i32, _>("format_version").map_err(sqlx_err)? as u32,
        object: StoredObject {
            key: ObjectKey::from_string(row.try_get::<String, _>("object_key").map_err(sqlx_err)?),
            size_bytes: to_u64(
                row.try_get::<i64, _>("size_bytes").map_err(sqlx_err)?,
                "size_bytes",
            )?,
            checksum: ObjectChecksum::from_string(
                row.try_get::<String, _>("checksum").map_err(sqlx_err)?,
            ),
            etag: row.try_get("etag").map_err(sqlx_err)?,
        },
        source_artifact_id: row
            .try_get::<Option<String>, _>("source_artifact_id")
            .map_err(sqlx_err)?
            .map(ArtifactId::from_string),
        source_checksum: row
            .try_get::<Option<String>, _>("source_checksum")
            .map_err(sqlx_err)?
            .map(ObjectChecksum::from_string),
        schema_fingerprint: row
            .try_get::<Option<i64>, _>("schema_fingerprint")
            .map_err(sqlx_err)?
            .map(SchemaFingerprint),
        state: row_state::<ArtifactState>(&state)?,
        failure_reason: row.try_get("failure_reason").map_err(sqlx_err)?,
    };
    Ok((segment_id, artifact))
}

/// data_segments 行 + 该段全部存活 Artifact → DataSegment。
pub(super) fn segment_from_row(row: &PgRow, artifacts: Vec<Artifact>) -> Result<DataSegment> {
    let id = SegmentId::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?);
    let state: String = row.try_get("state").map_err(sqlx_err)?;
    let column_stats: Json<ColumnStats> = row.try_get("column_stats").map_err(sqlx_err)?;
    let sequence_start: Option<i64> = row.try_get("sequence_start").map_err(sqlx_err)?;
    let sequence_end: Option<i64> = row.try_get("sequence_end").map_err(sqlx_err)?;
    let sequence_range = match (sequence_start, sequence_end) {
        (Some(start), Some(end)) => Some(SequenceRange::new(
            WalSequence(to_u64(start, "sequence_start")?),
            WalSequence(to_u64(end, "sequence_end")?),
        )),
        _ => None,
    };

    let mut primary = None;
    let mut auxiliaries = Vec::new();
    for artifact in artifacts {
        if artifact.role == ArtifactRole::PrimaryData && artifact.state != ArtifactState::Tombstoned
        {
            if primary.is_some() {
                return Err(Error::internal(format!(
                    "segment {id} has multiple live primary artifacts"
                )));
            }
            primary = Some(artifact);
        } else {
            auxiliaries.push(artifact);
        }
    }
    let primary = primary
        .ok_or_else(|| Error::internal(format!("segment {id} has no live primary artifact")))?;

    Ok(DataSegment {
        id,
        organization_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        dataset_id: PhysicalDatasetId::from_string(
            row.try_get::<String, _>("dataset_id").map_err(sqlx_err)?,
        ),
        partition: Partition {
            start_micros: row.try_get("partition_start_micros").map_err(sqlx_err)?,
            end_micros: row.try_get("partition_end_micros").map_err(sqlx_err)?,
            shard: row.try_get::<i16, _>("partition_shard").map_err(sqlx_err)? as u16,
        },
        time_range: TimeRange::new(
            crate::shared::time::TimestampMicros(
                row.try_get("min_event_micros").map_err(sqlx_err)?,
            ),
            crate::shared::time::TimestampMicros(
                row.try_get("max_event_micros").map_err(sqlx_err)?,
            ),
        ),
        sequence_range,
        row_count: to_u64(
            row.try_get::<i64, _>("row_count").map_err(sqlx_err)?,
            "row_count",
        )?,
        schema_fingerprint: row
            .try_get::<Option<i64>, _>("schema_fingerprint")
            .map_err(sqlx_err)?
            .map(SchemaFingerprint),
        column_stats: column_stats.0,
        flush_id: row
            .try_get::<Option<String>, _>("flush_id")
            .map_err(sqlx_err)?
            .map(FlushId::from_string),
        output_ordinal: row.try_get::<i32, _>("output_ordinal").map_err(sqlx_err)? as u32,
        primary,
        auxiliaries,
        state: row_state::<SegmentState>(&state)?,
        visible_from_version: to_u64(
            row.try_get::<i64, _>("visible_from_version")
                .map_err(sqlx_err)?,
            "visible_from_version",
        )?,
        retired_at_version: row
            .try_get::<Option<i64>, _>("retired_at_version")
            .map_err(sqlx_err)?
            .map(|v| to_u64(v, "retired_at_version"))
            .transpose()?,
        created_at_micros: row.try_get("created_at_micros").map_err(sqlx_err)?,
    })
}
