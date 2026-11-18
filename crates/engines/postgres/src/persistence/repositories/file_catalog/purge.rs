// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Removing the catalog rows of deleted streams.
//!
//! Deleting a stream leaves its datasets in state `deleting`. The purge removes their rows in
//! batches, the rows that refer to a dataset before the dataset itself: artifacts, segments,
//! manifests, flush commits and WAL checkpoints. A dataset goes once none of them is left. The
//! objects behind those rows are left to the orphan scan, which finds them no longer referenced.

use std::time::Instant;

use async_trait::async_trait;
use kernel::Result;
use sqlx::PgPool;

use super::sqlx_err;
use crate::persistence::repositories::organizations::PurgeProgress;

/// One statement per table, the tables that refer to others first; `$1` is the batch size.
const BATCHES: &[&str] = &[
    "DELETE FROM catalog_artifacts
      WHERE ctid = ANY(ARRAY(
          SELECT artifact.ctid
            FROM physical_datasets dataset
            JOIN catalog_segments segment
              ON segment.org_id = dataset.org_id AND segment.dataset_id = dataset.id
            JOIN catalog_artifacts artifact
              ON artifact.org_id = segment.org_id AND artifact.segment_id = segment.id
           WHERE dataset.state = 'deleting'
           LIMIT $1
      ))",
    "DELETE FROM catalog_segments
      WHERE ctid = ANY(ARRAY(
          SELECT segment.ctid
            FROM physical_datasets dataset
            JOIN catalog_segments segment
              ON segment.org_id = dataset.org_id AND segment.dataset_id = dataset.id
           WHERE dataset.state = 'deleting'
           LIMIT $1
      ))",
    "DELETE FROM partition_manifests
      WHERE ctid = ANY(ARRAY(
          SELECT manifest.ctid
            FROM physical_datasets dataset
            JOIN partition_manifests manifest
              ON manifest.org_id = dataset.org_id AND manifest.dataset_id = dataset.id
           WHERE dataset.state = 'deleting'
           LIMIT $1
      ))",
    "DELETE FROM storage_flush_commits
      WHERE ctid = ANY(ARRAY(
          SELECT flush.ctid
            FROM physical_datasets dataset
            JOIN storage_flush_commits flush
              ON flush.org_id = dataset.org_id AND flush.dataset_id = dataset.id
           WHERE dataset.state = 'deleting'
           LIMIT $1
      ))",
    "DELETE FROM wal_checkpoints
      WHERE ctid = ANY(ARRAY(
          SELECT checkpoint.ctid
            FROM physical_datasets dataset
            JOIN wal_checkpoints checkpoint
              ON checkpoint.org_id = dataset.org_id AND checkpoint.dataset_id = dataset.id
           WHERE dataset.state = 'deleting'
           LIMIT $1
      ))",
];

/// Datasets in state `deleting` with no row left that refers to them.
const EMPTIED: &str = "DELETE FROM physical_datasets dataset
  WHERE dataset.state = 'deleting'
    AND NOT EXISTS (SELECT 1 FROM catalog_segments segment
                     WHERE segment.org_id = dataset.org_id AND segment.dataset_id = dataset.id)
    AND NOT EXISTS (SELECT 1 FROM partition_manifests manifest
                     WHERE manifest.org_id = dataset.org_id AND manifest.dataset_id = dataset.id)
    AND NOT EXISTS (SELECT 1 FROM storage_flush_commits flush
                     WHERE flush.org_id = dataset.org_id AND flush.dataset_id = dataset.id)
    AND NOT EXISTS (SELECT 1 FROM wal_checkpoints checkpoint
                     WHERE checkpoint.org_id = dataset.org_id
                       AND checkpoint.dataset_id = dataset.id)";

#[async_trait]
pub trait DatasetPurgeRepository: Send + Sync {
    /// Delete the catalog rows of datasets in state `deleting`, `batch` rows a statement, and
    /// then the datasets left with none, until nothing is left or `deadline` passes.
    async fn purge_deleting(&self, batch: i64, deadline: Instant) -> Result<PurgeProgress>;
}

pub struct PgDatasetPurge {
    pool: PgPool,
}

impl PgDatasetPurge {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl DatasetPurgeRepository for PgDatasetPurge {
    async fn purge_deleting(&self, batch: i64, deadline: Instant) -> Result<PurgeProgress> {
        let mut deleted = 0;

        for statement in BATCHES {
            loop {
                let removed = sqlx::query(statement)
                    .bind(batch)
                    .execute(&self.pool)
                    .await
                    .map_err(sqlx_err)?
                    .rows_affected();

                deleted += removed;

                if removed < batch.max(1) as u64 {
                    break;
                }

                if Instant::now() >= deadline {
                    return Ok(PurgeProgress {
                        rows_deleted: deleted,
                        finished: false,
                    });
                }
            }
        }

        deleted += sqlx::query(EMPTIED)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?
            .rows_affected();

        Ok(PurgeProgress {
            rows_deleted: deleted,
            finished: deleted == 0,
        })
    }
}
