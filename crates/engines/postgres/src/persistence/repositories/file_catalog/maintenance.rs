// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashMap;

use sqlx::{PgPool, Row};

use super::{
    rows::{
        ARTIFACT_COLS, DATASET_COLS, SEGMENT_COLS, artifact_from_row, dataset_from_row,
        segment_from_row,
    },
    sqlx_err,
};
use crate::{
    domain::storage::{
        Artifact, CatalogInvariant, CatalogInvariantIssue, CatalogObject, IndexRebuildTask,
        ObjectChecksum, ObjectKey, OrganizationScope, PhysicalDatasetId, StoredObject,
    },
    shared::Result,
};

pub(super) async fn index_rebuild_tasks(
    pool: &PgPool,
    scope: &OrganizationScope,
    updated_before_micros: i64,
    limit: u32,
) -> Result<Vec<IndexRebuildTask>> {
    let candidates = sqlx::query(
        "SELECT a.id, a.segment_id, s.dataset_id FROM artifacts a \
         JOIN data_segments s ON s.org_id = a.org_id AND s.id = a.segment_id \
         WHERE a.org_id = $1 AND a.role = 'index' \
           AND (a.state = 'pending' OR (a.state = 'failed' AND a.updated_at_micros <= $2)) \
           AND s.state = 'active' \
         ORDER BY a.updated_at_micros, a.id LIMIT $3",
    )
    .bind(scope.organization_id.as_str())
    .bind(updated_before_micros)
    .bind(i64::from(limit.max(1)))
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?;
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    let segment_ids = candidates
        .iter()
        .map(|row| row.try_get::<String, _>("segment_id").map_err(sqlx_err))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let dataset_ids = candidates
        .iter()
        .map(|row| row.try_get::<String, _>("dataset_id").map_err(sqlx_err))
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let segment_rows = sqlx::query(&format!(
        "SELECT {SEGMENT_COLS} FROM data_segments WHERE org_id = $1 AND id = ANY($2)"
    ))
    .bind(scope.organization_id.as_str())
    .bind(&segment_ids)
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?;
    let artifact_rows = sqlx::query(&format!(
        "SELECT {ARTIFACT_COLS} FROM artifacts \
         WHERE org_id = $1 AND segment_id = ANY($2) AND state <> 'tombstoned'"
    ))
    .bind(scope.organization_id.as_str())
    .bind(&segment_ids)
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?;
    let dataset_rows = sqlx::query(&format!(
        "SELECT {DATASET_COLS} FROM physical_datasets WHERE org_id = $1 AND id = ANY($2)"
    ))
    .bind(scope.organization_id.as_str())
    .bind(&dataset_ids)
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?;

    let mut artifacts: HashMap<String, Vec<Artifact>> = HashMap::new();
    for row in &artifact_rows {
        let (segment_id, artifact) = artifact_from_row(row)?;
        artifacts
            .entry(segment_id.as_str().to_owned())
            .or_default()
            .push(artifact);
    }
    let mut segments = HashMap::new();
    for row in &segment_rows {
        let id: String = row.try_get("id").map_err(sqlx_err)?;
        let segment = segment_from_row(row, artifacts.remove(&id).unwrap_or_default())?;
        segments.insert(id, segment);
    }
    let mut datasets = HashMap::new();
    for row in &dataset_rows {
        let dataset = dataset_from_row(row)?;
        datasets.insert(dataset.id.as_str().to_owned(), dataset);
    }

    let mut tasks = Vec::with_capacity(candidates.len());
    for row in candidates {
        let artifact_id: String = row.try_get("id").map_err(sqlx_err)?;
        let segment_id: String = row.try_get("segment_id").map_err(sqlx_err)?;
        let dataset_id: String = row.try_get("dataset_id").map_err(sqlx_err)?;
        let Some(segment) = segments.get(&segment_id).cloned() else {
            continue;
        };
        let Some(artifact) = segment
            .auxiliaries
            .iter()
            .find(|artifact| artifact.id.as_str() == artifact_id)
            .cloned()
        else {
            continue;
        };
        let Some(dataset) = datasets.get(&dataset_id).cloned() else {
            continue;
        };
        tasks.push(IndexRebuildTask {
            dataset,
            segment,
            artifact,
        });
    }
    Ok(tasks)
}

pub(super) async fn catalog_objects(
    pool: &PgPool,
    scope: &OrganizationScope,
    after: Option<&ObjectKey>,
    limit: u32,
) -> Result<Vec<CatalogObject>> {
    let artifact_rows = sqlx::query(
        "SELECT s.dataset_id, a.segment_id, a.id, a.role, a.state, a.object_key, \
                a.size_bytes, a.checksum, a.etag \
         FROM artifacts a JOIN data_segments s ON s.org_id = a.org_id AND s.id = a.segment_id \
         WHERE a.org_id = $1 AND ($2::TEXT IS NULL OR a.object_key > $2) \
           AND s.state IN ('active', 'sealed') AND a.state <> 'tombstoned' \
         ORDER BY a.object_key LIMIT $3",
    )
    .bind(scope.organization_id.as_str())
    .bind(after.map(ObjectKey::as_str))
    .bind(i64::from(limit.max(1)))
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?;
    let mut objects = Vec::with_capacity(artifact_rows.len());
    for row in artifact_rows {
        objects.push(CatalogObject {
            dataset_id: PhysicalDatasetId::from_string(
                row.try_get::<String, _>("dataset_id").map_err(sqlx_err)?,
            ),
            segment_id: Some(crate::domain::storage::SegmentId::from_string(
                row.try_get::<String, _>("segment_id").map_err(sqlx_err)?,
            )),
            artifact_id: Some(crate::domain::storage::ArtifactId::from_string(
                row.try_get::<String, _>("id").map_err(sqlx_err)?,
            )),
            role: Some(
                row.try_get::<String, _>("role")
                    .map_err(sqlx_err)?
                    .parse()?,
            ),
            state: Some(
                row.try_get::<String, _>("state")
                    .map_err(sqlx_err)?
                    .parse()?,
            ),
            object: StoredObject {
                key: ObjectKey::from_string(
                    row.try_get::<String, _>("object_key").map_err(sqlx_err)?,
                ),
                size_bytes: super::rows::to_u64(
                    row.try_get::<i64, _>("size_bytes").map_err(sqlx_err)?,
                    "size_bytes",
                )?,
                checksum: ObjectChecksum::from_string(
                    row.try_get::<String, _>("checksum").map_err(sqlx_err)?,
                ),
                etag: row.try_get("etag").map_err(sqlx_err)?,
            },
            manifest: None,
        });
    }
    {
        let manifest_rows = sqlx::query(
            "SELECT dataset_id, partition_start_micros, partition_end_micros, partition_shard, \
                    generation, object_key, size_bytes, checksum, etag, segment_count, \
                    created_at_micros \
             FROM partition_manifests WHERE org_id = $1 AND state = 'active' \
               AND ($2::TEXT IS NULL OR object_key > $2) \
             ORDER BY object_key LIMIT $3",
        )
        .bind(scope.organization_id.as_str())
        .bind(after.map(ObjectKey::as_str))
        .bind(i64::from(limit.max(1)))
        .fetch_all(pool)
        .await
        .map_err(sqlx_err)?;
        for row in manifest_rows {
            let object = StoredObject {
                key: ObjectKey::from_string(
                    row.try_get::<String, _>("object_key").map_err(sqlx_err)?,
                ),
                size_bytes: super::rows::to_u64(
                    row.try_get::<i64, _>("size_bytes").map_err(sqlx_err)?,
                    "manifest size_bytes",
                )?,
                checksum: ObjectChecksum::from_string(
                    row.try_get::<String, _>("checksum").map_err(sqlx_err)?,
                ),
                etag: row.try_get("etag").map_err(sqlx_err)?,
            };
            let dataset_id = PhysicalDatasetId::from_string(
                row.try_get::<String, _>("dataset_id").map_err(sqlx_err)?,
            );
            objects.push(CatalogObject {
                dataset_id: dataset_id.clone(),
                segment_id: None,
                artifact_id: None,
                role: None,
                state: None,
                object: object.clone(),
                manifest: Some(crate::domain::storage::PartitionManifestPointer {
                    organization_id: scope.organization_id.clone(),
                    dataset_id,
                    partition: crate::domain::storage::Partition {
                        start_micros: row.try_get("partition_start_micros").map_err(sqlx_err)?,
                        end_micros: row.try_get("partition_end_micros").map_err(sqlx_err)?,
                        shard: row.try_get::<i16, _>("partition_shard").map_err(sqlx_err)? as u16,
                    },
                    generation: super::rows::to_u64(
                        row.try_get::<i64, _>("generation").map_err(sqlx_err)?,
                        "manifest generation",
                    )?,
                    object,
                    segment_count: row.try_get::<i32, _>("segment_count").map_err(sqlx_err)? as u32,
                    created_at_micros: row.try_get("created_at_micros").map_err(sqlx_err)?,
                }),
            });
        }
    }
    objects.sort_by(|left, right| left.object.key.as_str().cmp(right.object.key.as_str()));
    objects.truncate(limit.max(1) as usize);
    Ok(objects)
}

pub(super) async fn object_is_referenced(
    pool: &PgPool,
    scope: &OrganizationScope,
    object_key: &ObjectKey,
) -> Result<bool> {
    let row = sqlx::query(
        "SELECT \
           EXISTS(SELECT 1 FROM artifacts a JOIN data_segments s \
                  ON s.org_id = a.org_id AND s.id = a.segment_id \
                  WHERE a.org_id = $1 AND a.object_key = $2 \
                    AND a.state <> 'tombstoned' AND s.state IN ('active', 'sealed')) \
           OR EXISTS(SELECT 1 FROM partition_manifests \
                     WHERE org_id = $1 AND object_key = $2 AND state = 'active') \
           OR EXISTS(SELECT 1 FROM artifacts source JOIN artifacts derived \
                     ON derived.org_id = source.org_id AND derived.source_artifact_id = source.id \
                     WHERE source.org_id = $1 AND source.object_key = $2 \
                       AND derived.state <> 'tombstoned') AS referenced",
    )
    .bind(scope.organization_id.as_str())
    .bind(object_key.as_str())
    .fetch_one(pool)
    .await
    .map_err(sqlx_err)?;
    row.try_get("referenced").map_err(sqlx_err)
}

pub(super) async fn enqueue_orphan(
    pool: &PgPool,
    scope: &OrganizationScope,
    object_key: &ObjectKey,
    not_before_micros: i64,
) -> Result<()> {
    let now = crate::shared::time::TimestampMicros::now().0;
    sqlx::query(
        "INSERT INTO object_gc_queue (org_id, object_key, checksum, reason, not_before_micros, \
         attempt_count, state, last_error, created_at_micros, updated_at_micros) \
         VALUES ($1, $2, NULL, 'orphan', $3, 0, 'pending', NULL, $4, $4) \
         ON CONFLICT (org_id, object_key) DO NOTHING",
    )
    .bind(scope.organization_id.as_str())
    .bind(object_key.as_str())
    .bind(not_before_micros)
    .bind(now)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;
    Ok(())
}

pub(super) async fn catalog_invariant_issues(
    pool: &PgPool,
    scope: &OrganizationScope,
    gc_stuck_before_micros: i64,
) -> Result<Vec<CatalogInvariantIssue>> {
    let row = sqlx::query(
        "WITH committed AS ( \
             SELECT dataset_id, writer_node_id, writer_epoch, MAX(sequence_end) AS sequence_end \
             FROM storage_flush_commits WHERE org_id = $1 \
             GROUP BY dataset_id, writer_node_id, writer_epoch), \
         checkpoints AS ( \
             SELECT dataset_id, writer_node_id, writer_epoch, committed_sequence \
             FROM wal_checkpoints WHERE org_id = $1), \
         checkpoint_mismatches AS ( \
             SELECT 1 FROM committed c FULL OUTER JOIN checkpoints w \
               ON w.dataset_id = c.dataset_id \
              AND w.writer_node_id = c.writer_node_id AND w.writer_epoch = c.writer_epoch \
             WHERE w.committed_sequence IS DISTINCT FROM c.sequence_end) \
         SELECT \
           (SELECT COUNT(*) FROM data_segments s \
            WHERE s.org_id = $1 AND s.state = 'active' AND NOT EXISTS ( \
              SELECT 1 FROM artifacts a WHERE a.org_id = s.org_id AND a.segment_id = s.id \
                AND a.role = 'primary_data' AND a.state = 'ready')) AS missing_primary, \
           (SELECT COUNT(*) FROM checkpoint_mismatches) AS checkpoint_mismatch, \
           (SELECT COUNT(*) FROM object_gc_queue q WHERE q.org_id = $1 \
              AND q.state IN ('pending', 'processing') AND q.attempt_count > 0 \
              AND q.updated_at_micros <= $2) AS gc_stuck, \
           (SELECT COUNT(*) FROM artifacts a \
              JOIN data_segments s ON s.org_id = a.org_id AND s.id = a.segment_id \
              JOIN artifacts p ON p.org_id = s.org_id AND p.segment_id = s.id \
               AND p.role = 'primary_data' AND p.state = 'ready' \
            WHERE a.org_id = $1 AND s.state IN ('active', 'sealed') \
              AND a.role = 'index' AND a.state = 'ready' AND ( \
                a.source_artifact_id IS DISTINCT FROM p.id OR \
                a.source_checksum IS DISTINCT FROM p.checksum OR \
                a.schema_fingerprint IS DISTINCT FROM p.schema_fingerprint OR \
                s.schema_fingerprint IS DISTINCT FROM p.schema_fingerprint)) AS stale_index",
    )
    .bind(scope.organization_id.as_str())
    .bind(gc_stuck_before_micros)
    .fetch_one(pool)
    .await
    .map_err(sqlx_err)?;

    let counts = [
        (
            CatalogInvariant::ActiveSegmentMissingReadyPrimary,
            row.try_get::<i64, _>("missing_primary").map_err(sqlx_err)?,
        ),
        (
            CatalogInvariant::WalCheckpointFlushMismatch,
            row.try_get::<i64, _>("checkpoint_mismatch")
                .map_err(sqlx_err)?,
        ),
        (
            CatalogInvariant::GcStuck,
            row.try_get::<i64, _>("gc_stuck").map_err(sqlx_err)?,
        ),
        (
            CatalogInvariant::StaleIndexSource,
            row.try_get::<i64, _>("stale_index").map_err(sqlx_err)?,
        ),
    ];
    counts
        .into_iter()
        .filter(|(_, count)| *count > 0)
        .map(|(invariant, count)| {
            Ok(CatalogInvariantIssue {
                invariant,
                occurrences: u64::try_from(count).map_err(|_| {
                    crate::shared::Error::internal("negative Catalog invariant count")
                })?,
            })
        })
        .collect()
}

pub(super) async fn checksum_matches_retired_catalog(
    pool: &PgPool,
    scope: &OrganizationScope,
    object_key: &ObjectKey,
    expected: &ObjectChecksum,
) -> Result<bool> {
    let row = sqlx::query(
        "SELECT NOT EXISTS( \
           SELECT 1 FROM artifacts WHERE org_id = $1 AND object_key = $2 AND checksum <> $3 \
           UNION ALL \
           SELECT 1 FROM partition_manifests \
             WHERE org_id = $1 AND object_key = $2 AND checksum <> $3) AS matches",
    )
    .bind(scope.organization_id.as_str())
    .bind(object_key.as_str())
    .bind(expected.as_str())
    .fetch_one(pool)
    .await
    .map_err(sqlx_err)?;
    row.try_get("matches").map_err(sqlx_err)
}
