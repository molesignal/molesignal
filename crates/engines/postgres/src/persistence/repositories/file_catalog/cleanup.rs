// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Removing the rows of finished work while a dataset is still in use.
//!
//! A deleted file leaves a tombstoned artifact, the segment or manifest it belonged to, and a GC
//! row in state `deleted`; every flush leaves a flush record. None of them is read again once the
//! work is over, so they go after the history window, in an order that keeps each safety check
//! whole:
//!
//! 1. Tombstoned artifacts and retired manifests whose GC row says the object was deleted before
//!    the cutoff. The GC row is the proof, so it stays until they are gone.
//! 2. Replaced and tombstoned segments with no artifact left.
//! 3. GC rows in state `deleted` no artifact or manifest names any more.
//! 4. Flush records that can no longer be retried or replayed, their collected bytes folded into
//!    the dataset first, and the checkpoints of closed WAL streams with no flush record left.
//!
//! Live rows are never touched: queries read active and sealed segments only, the GC reference
//! check skips tombstoned artifacts, and a compaction naming a segment that is gone already fails
//! as a conflict.

use domain::storage::{CatalogCleanup, OrganizationScope};
use kernel::{Result, time::TimestampMicros};
use sqlx::PgPool;

use super::sqlx_err;

/// Tombstoned artifacts whose object GC deleted before `$2`.
const ARTIFACTS: &str = "DELETE FROM catalog_artifacts
  WHERE ctid = ANY(ARRAY(
      SELECT artifact.ctid
        FROM catalog_artifacts artifact
        JOIN object_gc_queue gc
          ON gc.org_id = artifact.org_id AND gc.object_key = artifact.object_key
       WHERE artifact.org_id = $1 AND artifact.state = 'tombstoned'
         AND gc.state = 'deleted' AND gc.updated_at <= $2
       LIMIT $3
  ))";

/// Retired manifests whose object GC deleted before `$2`.
const MANIFESTS: &str = "DELETE FROM partition_manifests
  WHERE ctid = ANY(ARRAY(
      SELECT manifest.ctid
        FROM partition_manifests manifest
        JOIN object_gc_queue gc
          ON gc.org_id = manifest.org_id AND gc.object_key = manifest.object_key
       WHERE manifest.org_id = $1 AND manifest.state <> 'active'
         AND gc.state = 'deleted' AND gc.updated_at <= $2
       LIMIT $3
  ))";

/// Replaced and tombstoned segments with no artifact left. Their artifacts went after the
/// cutoff, so the segments are at least as old.
const SEGMENTS: &str = "DELETE FROM catalog_segments
  WHERE ctid = ANY(ARRAY(
      SELECT segment.ctid
        FROM catalog_segments segment
       WHERE segment.org_id = $1 AND segment.state IN ('replaced', 'tombstoned')
         AND NOT EXISTS (SELECT 1 FROM catalog_artifacts artifact
                          WHERE artifact.org_id = segment.org_id
                            AND artifact.segment_id = segment.id)
       LIMIT $2
  ))";

/// GC rows of objects deleted before `$2` that no artifact or manifest names. A live row naming
/// a deleted object would be a fault worth keeping the evidence of.
const GC_ENTRIES: &str = "DELETE FROM object_gc_queue
  WHERE ctid = ANY(ARRAY(
      SELECT gc.ctid
        FROM object_gc_queue gc
       WHERE gc.org_id = $1 AND gc.state = 'deleted' AND gc.updated_at <= $2
         AND NOT EXISTS (SELECT 1 FROM catalog_artifacts artifact
                          WHERE artifact.org_id = gc.org_id AND artifact.object_key = gc.object_key)
         AND NOT EXISTS (SELECT 1 FROM partition_manifests manifest
                          WHERE manifest.org_id = gc.org_id AND manifest.object_key = gc.object_key)
       LIMIT $3
  ))";

/// Flush records committed before `$2` that nothing can retry or replay, folded into their
/// dataset in the same statement. A record goes when a later one of the same writer stream
/// exists, so the newest record keeps matching the checkpoint, or when its WAL stream is closed:
/// a closed stream is fully published and refuses commits, so neither a retry nor a takeover
/// replays it.
const FLUSH_COMMITS: &str = "WITH removed AS (
    DELETE FROM storage_flush_commits
     WHERE ctid = ANY(ARRAY(
         SELECT flush.ctid
           FROM storage_flush_commits flush
          WHERE flush.org_id = $1 AND flush.committed_at <= $2
            AND (EXISTS (SELECT 1 FROM storage_flush_commits later
                          WHERE later.org_id = flush.org_id
                            AND later.dataset_id = flush.dataset_id
                            AND later.writer_node_id = flush.writer_node_id
                            AND later.writer_epoch = flush.writer_epoch
                            AND later.sequence_end > flush.sequence_end)
                 OR NOT EXISTS (SELECT 1 FROM wal_streams stream
                                 WHERE stream.org_id = flush.org_id
                                   AND stream.dataset_id = flush.dataset_id
                                   AND stream.writer_epoch = flush.writer_epoch))
          LIMIT $3
     ))
    RETURNING dataset_id, collected_bytes
), folded AS (
    SELECT dataset_id,
           COALESCE(SUM(collected_bytes), 0)::BIGINT AS collected_bytes,
           BOOL_AND(collected_bytes IS NOT NULL) AS complete,
           COUNT(*) AS records
      FROM removed
     GROUP BY dataset_id
), updated AS (
    UPDATE physical_datasets dataset
       SET folded_collected_bytes = dataset.folded_collected_bytes + folded.collected_bytes,
           folded_collected_complete = dataset.folded_collected_complete AND folded.complete
      FROM folded
     WHERE dataset.org_id = $1 AND dataset.id = folded.dataset_id
)
SELECT COALESCE(SUM(records), 0)::BIGINT FROM folded";

/// Checkpoints of closed WAL streams with no flush record left: nothing reads them again.
const CHECKPOINTS: &str = "DELETE FROM wal_checkpoints
  WHERE ctid = ANY(ARRAY(
      SELECT checkpoint.ctid
        FROM wal_checkpoints checkpoint
       WHERE checkpoint.org_id = $1
         AND NOT EXISTS (SELECT 1 FROM wal_streams stream
                          WHERE stream.org_id = checkpoint.org_id
                            AND stream.dataset_id = checkpoint.dataset_id
                            AND stream.writer_epoch = checkpoint.writer_epoch)
         AND NOT EXISTS (SELECT 1 FROM storage_flush_commits flush
                          WHERE flush.org_id = checkpoint.org_id
                            AND flush.dataset_id = checkpoint.dataset_id
                            AND flush.writer_node_id = checkpoint.writer_node_id
                            AND flush.writer_epoch = checkpoint.writer_epoch)
       LIMIT $2
  ))";

pub(super) async fn clean_finished(
    pool: &PgPool,
    scope: &OrganizationScope,
    before_micros: i64,
    limit: u32,
) -> Result<CatalogCleanup> {
    let org = scope.organization_id.as_str();
    let before = TimestampMicros(before_micros);
    let limit = i64::from(limit.max(1));

    let artifacts = delete_before(pool, ARTIFACTS, org, before, limit).await?;
    let manifests = delete_before(pool, MANIFESTS, org, before, limit).await?;
    let segments = sqlx::query(SEGMENTS)
        .bind(org)
        .bind(limit)
        .execute(pool)
        .await
        .map_err(sqlx_err)?
        .rows_affected();

    let gc_entries = delete_before(pool, GC_ENTRIES, org, before, limit).await?;

    // The records and the checkpoints go together, so the reconciler never sees a checkpoint
    // whose newest record is gone.
    let mut tx = sqlx::begin(pool).await.map_err(sqlx_err)?;
    let flush_commits: i64 = sqlx::query_scalar(FLUSH_COMMITS)
        .bind(org)
        .bind(before)
        .bind(limit)
        .fetch_one(&mut *tx)
        .await
        .map_err(sqlx_err)?;

    let checkpoints = sqlx::query(CHECKPOINTS)
        .bind(org)
        .bind(limit)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?
        .rows_affected();

    tx.commit().await.map_err(sqlx_err)?;

    Ok(CatalogCleanup {
        artifacts,
        segments,
        manifests,
        gc_entries,
        flush_commits: flush_commits.max(0) as u64,
        checkpoints,
    })
}

async fn delete_before(
    pool: &PgPool,
    statement: &str,
    org: &str,
    before: TimestampMicros,
    limit: i64,
) -> Result<u64> {
    Ok(sqlx::query(statement)
        .bind(org)
        .bind(before)
        .bind(limit)
        .execute(pool)
        .await
        .map_err(sqlx_err)?
        .rows_affected())
}
