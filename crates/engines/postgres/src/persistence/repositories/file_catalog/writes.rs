// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! flush / compaction / retention 共用的事务内写入原语。

use sqlx::{PgConnection, Row, types::Json};

use super::{rows::to_i64, sqlx_err};
use crate::{
    domain::storage::{
        Artifact, DataSegment, GcReason, ObjectChecksum, ObjectKey, OrganizationScope,
        PhysicalDatasetId, SegmentId,
    },
    shared::{Error, Result, time::TimestampMicros},
};

/// 锁定 dataset 行并返回当前 catalog_version。所有改动可见文件集合的事务都
/// 必须先拿这把锁，保证版本号串行递增。
pub(super) async fn lock_dataset_version(
    conn: &mut PgConnection,
    scope: &OrganizationScope,
    dataset_id: &PhysicalDatasetId,
) -> Result<u64> {
    let row = sqlx::query(
        "SELECT catalog_version FROM physical_datasets WHERE org_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(scope.organization_id.as_str())
    .bind(dataset_id.as_str())
    .fetch_optional(conn)
    .await
    .map_err(sqlx_err)?
    .ok_or_else(|| Error::not_found(format!("physical dataset {dataset_id}")))?;
    super::rows::to_u64(
        row.try_get::<i64, _>("catalog_version").map_err(sqlx_err)?,
        "catalog_version",
    )
}

pub(super) async fn bump_dataset_version(
    conn: &mut PgConnection,
    scope: &OrganizationScope,
    dataset_id: &PhysicalDatasetId,
    new_version: u64,
) -> Result<()> {
    sqlx::query(
        "UPDATE physical_datasets SET catalog_version = $3, updated_at_micros = $4 \
         WHERE org_id = $1 AND id = $2",
    )
    .bind(scope.organization_id.as_str())
    .bind(dataset_id.as_str())
    .bind(to_i64(new_version, "catalog_version")?)
    .bind(TimestampMicros::now().0)
    .execute(conn)
    .await
    .map_err(sqlx_err)?;
    Ok(())
}

/// 插入一个 Segment 及其全部 Artifact 行。`visible_from_version` 由调用方
/// 统一指定为本事务递增后的版本号。
pub(super) async fn insert_segment(
    conn: &mut PgConnection,
    scope: &OrganizationScope,
    segment: &DataSegment,
    visible_from_version: u64,
) -> Result<()> {
    let now = TimestampMicros::now().0;
    sqlx::query(
        "INSERT INTO data_segments (org_id, id, dataset_id, partition_start_micros, \
         partition_end_micros, partition_shard, min_event_micros, max_event_micros, row_count, \
         schema_fingerprint, column_stats, flush_id, sequence_start, sequence_end, \
         output_ordinal, state, visible_from_version, retired_at_version, created_at_micros)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, \
                 'active', $16, NULL, $17)",
    )
    .bind(scope.organization_id.as_str())
    .bind(segment.id.as_str())
    .bind(segment.dataset_id.as_str())
    .bind(segment.partition.start_micros)
    .bind(segment.partition.end_micros)
    .bind(segment.partition.shard as i16)
    .bind(segment.time_range.start.0)
    .bind(segment.time_range.end.0)
    .bind(to_i64(segment.row_count, "row_count")?)
    .bind(segment.schema_fingerprint.map(|f| f.0))
    .bind(Json(&segment.column_stats))
    .bind(segment.flush_id.as_ref().map(|f| f.as_str()))
    .bind(
        segment
            .sequence_range
            .map(|r| to_i64(r.start.0, "sequence_start"))
            .transpose()?,
    )
    .bind(
        segment
            .sequence_range
            .map(|r| to_i64(r.end.0, "sequence_end"))
            .transpose()?,
    )
    .bind(segment.output_ordinal as i32)
    .bind(to_i64(visible_from_version, "visible_from_version")?)
    .bind(now)
    .execute(&mut *conn)
    .await
    .map_err(sqlx_err)?;

    for artifact in segment.artifacts() {
        insert_artifact(conn, scope, &segment.id, artifact).await?;
    }
    Ok(())
}

pub(super) async fn insert_artifact(
    conn: &mut PgConnection,
    scope: &OrganizationScope,
    segment_id: &SegmentId,
    artifact: &Artifact,
) -> Result<()> {
    let now = TimestampMicros::now().0;
    sqlx::query(
        "INSERT INTO artifacts (org_id, id, segment_id, role, artifact_type, format_version, \
         object_key, size_bytes, checksum, etag, source_artifact_id, source_checksum, schema_fingerprint, \
         state, failure_reason, created_at_micros, updated_at_micros)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $16)",
    )
    .bind(scope.organization_id.as_str())
    .bind(artifact.id.as_str())
    .bind(segment_id.as_str())
    .bind(artifact.role.as_str())
    .bind(artifact.artifact_type.as_str())
    .bind(artifact.format_version as i32)
    .bind(artifact.object.key.as_str())
    .bind(to_i64(artifact.object.size_bytes, "size_bytes")?)
    .bind(artifact.object.checksum.as_str())
    .bind(artifact.object.etag.as_deref())
    .bind(artifact.source_artifact_id.as_ref().map(|id| id.as_str()))
    .bind(artifact.source_checksum.as_ref().map(ObjectChecksum::as_str))
    .bind(artifact.schema_fingerprint.map(|f| f.0))
    .bind(artifact.state.as_str())
    .bind(artifact.failure_reason.as_deref())
    .bind(now)
    .execute(conn)
    .await
    .map_err(sqlx_err)?;
    Ok(())
}

/// 把对象排进延迟 GC 队列。同一对象重复入队时取更晚的 not_before（更保守）。
pub(super) async fn enqueue_object_gc(
    conn: &mut PgConnection,
    scope: &OrganizationScope,
    object_key: &ObjectKey,
    checksum: Option<&ObjectChecksum>,
    reason: GcReason,
    not_before_micros: i64,
) -> Result<()> {
    let now = TimestampMicros::now().0;
    sqlx::query(
        "INSERT INTO object_gc_queue (org_id, object_key, checksum, reason, not_before_micros, \
         attempt_count, state, last_error, created_at_micros, updated_at_micros)
         VALUES ($1, $2, $3, $4, $5, 0, 'pending', NULL, $6, $6)
         ON CONFLICT (org_id, object_key) DO UPDATE SET
             not_before_micros = GREATEST(object_gc_queue.not_before_micros, \
                                          EXCLUDED.not_before_micros),
             checksum = COALESCE(EXCLUDED.checksum, object_gc_queue.checksum),
             reason = EXCLUDED.reason,
             attempt_count = 0,
             state = 'pending',
             last_error = NULL,
             updated_at_micros = EXCLUDED.updated_at_micros",
    )
    .bind(scope.organization_id.as_str())
    .bind(object_key.as_str())
    .bind(checksum.map(|c| c.as_str()))
    .bind(reason.as_str())
    .bind(not_before_micros)
    .bind(now)
    .execute(conn)
    .await
    .map_err(sqlx_err)?;
    Ok(())
}

/// 把一组 Segment 的全部存活 Artifact 置为 tombstoned 并入 GC 队列。
/// 返回受影响的 Artifact 数。
pub(super) async fn tombstone_segment_artifacts(
    conn: &mut PgConnection,
    scope: &OrganizationScope,
    segment_ids: &[String],
    reason: GcReason,
    gc_not_before_micros: i64,
) -> Result<usize> {
    if segment_ids.is_empty() {
        return Ok(0);
    }
    let rows = sqlx::query(
        "SELECT object_key, checksum FROM artifacts \
         WHERE org_id = $1 AND segment_id = ANY($2) AND state <> 'tombstoned'",
    )
    .bind(scope.organization_id.as_str())
    .bind(segment_ids)
    .fetch_all(&mut *conn)
    .await
    .map_err(sqlx_err)?;

    sqlx::query(
        "UPDATE artifacts SET state = 'tombstoned', updated_at_micros = $3 \
         WHERE org_id = $1 AND segment_id = ANY($2) AND state <> 'tombstoned'",
    )
    .bind(scope.organization_id.as_str())
    .bind(segment_ids)
    .bind(TimestampMicros::now().0)
    .execute(&mut *conn)
    .await
    .map_err(sqlx_err)?;

    for row in &rows {
        let key = ObjectKey::from_string(row.try_get::<String, _>("object_key").map_err(sqlx_err)?);
        let checksum =
            ObjectChecksum::from_string(row.try_get::<String, _>("checksum").map_err(sqlx_err)?);
        enqueue_object_gc(
            conn,
            scope,
            &key,
            Some(&checksum),
            reason,
            gc_not_before_micros,
        )
        .await?;
    }
    Ok(rows.len())
}
