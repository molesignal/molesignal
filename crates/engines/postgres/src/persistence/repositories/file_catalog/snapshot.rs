// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 跨 Dataset 一致性快照。
//!
//! 同一个 REPEATABLE READ 只读事务内读取版本号、WAL checkpoint、Active Segment
//! 与其 Artifact，保证 raw / rollup 等多数据集读到同一时点；调用方拿到快照后
//! 不再触碰活动 Catalog 状态。

use std::collections::HashMap;

use sqlx::{PgPool, Row};

use super::{
    rows::{ARTIFACT_COLS, SEGMENT_COLS, artifact_from_row, segment_from_row, to_u64},
    sqlx_err,
};
use crate::{
    domain::storage::{
        Artifact, CatalogSnapshot, DatasetSelection, DatasetSnapshot, ObjectChecksum, ObjectKey,
        OrganizationScope, Partition, PartitionManifestPointer, PhysicalDatasetId, StoredObject,
        WalCheckpointView, WalSequence, WriterEpoch, WriterNodeId,
    },
    shared::{Error, Result},
};

pub(super) async fn snapshot(
    pool: &PgPool,
    scope: &OrganizationScope,
    selection: DatasetSelection,
) -> Result<CatalogSnapshot> {
    if selection.dataset_ids.is_empty() {
        return Ok(CatalogSnapshot::default());
    }
    let dataset_ids: Vec<String> = selection
        .dataset_ids
        .iter()
        .map(|id| id.as_str().to_owned())
        .collect();

    let mut tx = sqlx::begin(pool).await.map_err(sqlx_err)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

    let version_rows = sqlx::query(
        "SELECT id, catalog_version FROM physical_datasets WHERE org_id = $1 AND id = ANY($2)",
    )
    .bind(scope.organization_id.as_str())
    .bind(&dataset_ids)
    .fetch_all(&mut *tx)
    .await
    .map_err(sqlx_err)?;
    let mut versions: HashMap<String, u64> = HashMap::with_capacity(version_rows.len());
    for row in &version_rows {
        let id: String = row.try_get("id").map_err(sqlx_err)?;
        let version = to_u64(
            row.try_get::<i64, _>("catalog_version").map_err(sqlx_err)?,
            "catalog_version",
        )?;
        versions.insert(id, version);
    }
    if let Some(missing) = dataset_ids.iter().find(|id| !versions.contains_key(*id)) {
        return Err(Error::not_found(format!("physical dataset {missing}")));
    }

    let checkpoint_rows = sqlx::query(
        "SELECT dataset_id, writer_node_id, writer_epoch, committed_sequence \
         FROM wal_checkpoints WHERE org_id = $1 AND dataset_id = ANY($2)",
    )
    .bind(scope.organization_id.as_str())
    .bind(&dataset_ids)
    .fetch_all(&mut *tx)
    .await
    .map_err(sqlx_err)?;
    let mut checkpoints: HashMap<String, Vec<WalCheckpointView>> = HashMap::new();
    for row in &checkpoint_rows {
        let dataset_id: String = row.try_get("dataset_id").map_err(sqlx_err)?;
        checkpoints
            .entry(dataset_id)
            .or_default()
            .push(WalCheckpointView {
                writer_node_id: WriterNodeId::new(
                    row.try_get::<String, _>("writer_node_id")
                        .map_err(sqlx_err)?,
                ),
                writer_epoch: WriterEpoch(to_u64(
                    row.try_get::<i64, _>("writer_epoch").map_err(sqlx_err)?,
                    "writer_epoch",
                )?),
                committed_sequence: WalSequence(to_u64(
                    row.try_get::<i64, _>("committed_sequence")
                        .map_err(sqlx_err)?,
                    "committed_sequence",
                )?),
            });
    }

    let mut segment_sql = format!(
        "SELECT {SEGMENT_COLS} FROM data_segments \
         WHERE org_id = $1 AND dataset_id = ANY($2) AND state = 'active' \
           AND min_event_micros <= $3 AND max_event_micros >= $4"
    );
    if selection.partition_shard.is_some() {
        segment_sql.push_str(" AND partition_shard = $5");
    }
    segment_sql.push_str(" ORDER BY partition_start_micros, id");
    let mut segment_query = sqlx::query(&segment_sql)
        .bind(scope.organization_id.as_str())
        .bind(&dataset_ids)
        .bind(selection.time_range.end.0)
        .bind(selection.time_range.start.0);
    if let Some(shard) = selection.partition_shard {
        segment_query = segment_query.bind(shard as i16);
    }
    let segment_rows = segment_query.fetch_all(&mut *tx).await.map_err(sqlx_err)?;

    let mut manifest_sql = "SELECT org_id, dataset_id, partition_start_micros, \
         partition_end_micros, partition_shard, generation, object_key, size_bytes, checksum, \
         etag, segment_count, created_at_micros FROM partition_manifests \
         WHERE org_id = $1 AND dataset_id = ANY($2) AND state = 'active' \
           AND partition_start_micros <= $3 AND partition_end_micros >= $4"
        .to_owned();
    if selection.partition_shard.is_some() {
        manifest_sql.push_str(" AND partition_shard = $5");
    }
    manifest_sql.push_str(" ORDER BY partition_start_micros, generation");
    let mut manifest_query = sqlx::query(&manifest_sql)
        .bind(scope.organization_id.as_str())
        .bind(&dataset_ids)
        .bind(selection.time_range.end.0)
        .bind(selection.time_range.start.0);
    if let Some(shard) = selection.partition_shard {
        manifest_query = manifest_query.bind(shard as i16);
    }
    let manifest_rows = manifest_query.fetch_all(&mut *tx).await.map_err(sqlx_err)?;

    let segment_ids: Vec<String> = segment_rows
        .iter()
        .map(|row| row.try_get::<String, _>("id").map_err(sqlx_err))
        .collect::<std::result::Result<_, _>>()?;
    let mut artifacts_by_segment: HashMap<String, Vec<Artifact>> = HashMap::new();
    if !segment_ids.is_empty() {
        let artifact_rows = sqlx::query(&format!(
            "SELECT {ARTIFACT_COLS} FROM artifacts \
             WHERE org_id = $1 AND segment_id = ANY($2) AND state <> 'tombstoned'"
        ))
        .bind(scope.organization_id.as_str())
        .bind(&segment_ids)
        .fetch_all(&mut *tx)
        .await
        .map_err(sqlx_err)?;
        for row in &artifact_rows {
            let (segment_id, artifact) = artifact_from_row(row)?;
            artifacts_by_segment
                .entry(segment_id.as_str().to_owned())
                .or_default()
                .push(artifact);
        }
    }
    tx.commit().await.map_err(sqlx_err)?;

    let mut segments_by_dataset: HashMap<String, Vec<_>> = HashMap::new();
    for row in &segment_rows {
        let dataset_id: String = row.try_get("dataset_id").map_err(sqlx_err)?;
        let id: String = row.try_get("id").map_err(sqlx_err)?;
        let artifacts = artifacts_by_segment.remove(&id).unwrap_or_default();
        segments_by_dataset
            .entry(dataset_id)
            .or_default()
            .push(segment_from_row(row, artifacts)?);
    }
    let mut manifests_by_dataset: HashMap<String, Vec<PartitionManifestPointer>> = HashMap::new();
    for row in &manifest_rows {
        let dataset_id: String = row.try_get("dataset_id").map_err(sqlx_err)?;
        manifests_by_dataset
            .entry(dataset_id.clone())
            .or_default()
            .push(PartitionManifestPointer {
                organization_id: scope.organization_id.clone(),
                dataset_id: PhysicalDatasetId::from_string(dataset_id),
                partition: Partition {
                    start_micros: row.try_get("partition_start_micros").map_err(sqlx_err)?,
                    end_micros: row.try_get("partition_end_micros").map_err(sqlx_err)?,
                    shard: row.try_get::<i16, _>("partition_shard").map_err(sqlx_err)? as u16,
                },
                generation: to_u64(
                    row.try_get::<i64, _>("generation").map_err(sqlx_err)?,
                    "manifest generation",
                )?,
                object: StoredObject {
                    key: ObjectKey::from_string(
                        row.try_get::<String, _>("object_key").map_err(sqlx_err)?,
                    ),
                    size_bytes: to_u64(
                        row.try_get::<i64, _>("size_bytes").map_err(sqlx_err)?,
                        "manifest size_bytes",
                    )?,
                    checksum: ObjectChecksum::from_string(
                        row.try_get::<String, _>("checksum").map_err(sqlx_err)?,
                    ),
                    etag: row.try_get("etag").map_err(sqlx_err)?,
                },
                segment_count: row.try_get::<i32, _>("segment_count").map_err(sqlx_err)? as u32,
                created_at_micros: row.try_get("created_at_micros").map_err(sqlx_err)?,
            });
    }

    Ok(CatalogSnapshot {
        datasets: dataset_ids
            .iter()
            .map(|id| DatasetSnapshot {
                dataset_id: PhysicalDatasetId::from_string(id.clone()),
                catalog_version: versions.get(id).copied().unwrap_or(0),
                wal_checkpoints: checkpoints.remove(id).unwrap_or_default(),
                segments: segments_by_dataset.remove(id).unwrap_or_default(),
                manifests: manifests_by_dataset.remove(id).unwrap_or_default(),
            })
            .collect(),
    })
}

pub(super) async fn wal_checkpoints(
    pool: &PgPool,
    scope: &OrganizationScope,
    dataset_id: &PhysicalDatasetId,
) -> Result<Vec<WalCheckpointView>> {
    let rows = sqlx::query(
        "SELECT writer_node_id, writer_epoch, committed_sequence \
         FROM wal_checkpoints WHERE org_id = $1 AND dataset_id = $2",
    )
    .bind(scope.organization_id.as_str())
    .bind(dataset_id.as_str())
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?;
    rows.iter()
        .map(|row| {
            Ok(WalCheckpointView {
                writer_node_id: WriterNodeId::new(
                    row.try_get::<String, _>("writer_node_id")
                        .map_err(sqlx_err)?,
                ),
                writer_epoch: WriterEpoch(to_u64(
                    row.try_get::<i64, _>("writer_epoch").map_err(sqlx_err)?,
                    "writer_epoch",
                )?),
                committed_sequence: WalSequence(to_u64(
                    row.try_get::<i64, _>("committed_sequence")
                        .map_err(sqlx_err)?,
                    "committed_sequence",
                )?),
            })
        })
        .collect()
}
