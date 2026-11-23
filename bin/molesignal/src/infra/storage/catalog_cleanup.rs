// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Cleaning up the storage catalog's rows of finished work.
//!
//! Deleted files, the segments and manifests they belonged to, their GC rows and the flush
//! records of closed WAL streams stay in the catalog until the history window of the platform
//! setting `catalog_history_retention_days` has passed; [`FileCatalog::clean_finished`] says what
//! goes and in which order. One organization is cleaned in bounded batches per round, so a large
//! backlog drains over a few rounds instead of in one long statement.

use std::sync::{Arc, OnceLock};

use domain::storage::{CatalogCleanup, FileCatalog, OrganizationScope};
use kernel::Result;
use prometheus::IntCounter;

/// Rows removed per table and statement.
const BATCH: u32 = 1_000;
/// Batches per organization and round.
const MAX_BATCHES: usize = 20;

pub struct CatalogCleanupWorker {
    catalog: Arc<dyn FileCatalog>,
}

impl CatalogCleanupWorker {
    pub fn new(catalog: Arc<dyn FileCatalog>) -> Self {
        Self { catalog }
    }

    /// Removes the organization's rows of work finished before `before_micros`; returns how many.
    pub async fn run_scope(&self, scope: OrganizationScope, before_micros: i64) -> Result<usize> {
        let mut removed = CatalogCleanup::default();

        for _ in 0..MAX_BATCHES {
            let batch = self
                .catalog
                .clean_finished(&scope, before_micros, BATCH)
                .await?;

            add(&mut removed, &batch);

            if !fills_a_batch(&batch) {
                break;
            }
        }

        if removed.total() > 0 {
            tracing::info!(
                organization = %scope.organization_id,
                artifacts = removed.artifacts,
                segments = removed.segments,
                manifests = removed.manifests,
                gc_entries = removed.gc_entries,
                flush_commits = removed.flush_commits,
                checkpoints = removed.checkpoints,
                "storage catalog history cleaned up"
            );
        }

        removed_total().inc_by(removed.total());
        Ok(usize::try_from(removed.total()).unwrap_or(usize::MAX))
    }
}

fn add(total: &mut CatalogCleanup, batch: &CatalogCleanup) {
    total.artifacts += batch.artifacts;
    total.segments += batch.segments;
    total.manifests += batch.manifests;
    total.gc_entries += batch.gc_entries;
    total.flush_commits += batch.flush_commits;
    total.checkpoints += batch.checkpoints;
}

/// Whether some table had a full batch, so more of its rows may be waiting.
fn fills_a_batch(batch: &CatalogCleanup) -> bool {
    let full = u64::from(BATCH);

    [
        batch.artifacts,
        batch.segments,
        batch.manifests,
        batch.gc_entries,
        batch.flush_commits,
        batch.checkpoints,
    ]
    .into_iter()
    .any(|count| count >= full)
}

fn removed_total() -> &'static IntCounter {
    static METRIC: OnceLock<IntCounter> = OnceLock::new();

    METRIC.get_or_init(|| {
        signals::metrics::register_int_counter(
            "storage_catalog_history_removed_total",
            "Storage catalog rows of finished work removed after the history window",
        )
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use domain::storage::{
        CatalogSnapshot, CommitFlush, DatasetSelection, FlushCommitResult, PhysicalDataset,
        PhysicalDatasetId, PhysicalDatasetSpec, ReplaceSegments, TombstoneSegments, UpdateArtifact,
        WalCheckpointView,
    };
    use kernel::ids::Id;

    use super::*;

    /// Answers each cleanup round from a script and records the cutoffs it was given.
    struct Scripted {
        rounds: Mutex<Vec<CatalogCleanup>>,
        cutoffs: Mutex<Vec<i64>>,
    }

    #[async_trait]
    impl FileCatalog for Scripted {
        async fn intake_usage(
            &self,
            _scope: &OrganizationScope,
        ) -> Result<Vec<domain::storage::DatasetIntakeUsage>> {
            unimplemented!()
        }

        async fn ensure_datasets(
            &self,
            _scope: &OrganizationScope,
            _logical_stream_id: &Id,
            _specs: &[PhysicalDatasetSpec],
        ) -> Result<Vec<PhysicalDataset>> {
            unimplemented!()
        }

        async fn list_datasets(
            &self,
            _scope: &OrganizationScope,
            _logical_stream_id: &Id,
        ) -> Result<Vec<PhysicalDataset>> {
            unimplemented!()
        }

        async fn snapshot(
            &self,
            _scope: &OrganizationScope,
            _selection: DatasetSelection,
        ) -> Result<CatalogSnapshot> {
            unimplemented!()
        }

        async fn commit_flush(
            &self,
            _scope: &OrganizationScope,
            _command: CommitFlush,
        ) -> Result<FlushCommitResult> {
            unimplemented!()
        }

        async fn replace_segments(
            &self,
            _scope: &OrganizationScope,
            _command: ReplaceSegments,
        ) -> Result<u64> {
            unimplemented!()
        }

        async fn update_artifact(
            &self,
            _scope: &OrganizationScope,
            _command: UpdateArtifact,
        ) -> Result<()> {
            unimplemented!()
        }

        async fn tombstone_segments(
            &self,
            _scope: &OrganizationScope,
            _command: TombstoneSegments,
        ) -> Result<u64> {
            unimplemented!()
        }

        async fn wal_checkpoints(
            &self,
            _scope: &OrganizationScope,
            _dataset_id: &PhysicalDatasetId,
        ) -> Result<Vec<WalCheckpointView>> {
            unimplemented!()
        }

        async fn clean_finished(
            &self,
            _scope: &OrganizationScope,
            before_micros: i64,
            _limit: u32,
        ) -> Result<CatalogCleanup> {
            self.cutoffs.lock().unwrap().push(before_micros);

            let mut rounds = self.rounds.lock().unwrap();
            Ok(if rounds.is_empty() {
                CatalogCleanup::default()
            } else {
                rounds.remove(0)
            })
        }
    }

    fn scripted(rounds: Vec<CatalogCleanup>) -> Arc<Scripted> {
        Arc::new(Scripted {
            rounds: Mutex::new(rounds),
            cutoffs: Mutex::new(Vec::new()),
        })
    }

    #[tokio::test]
    async fn a_full_batch_is_followed_by_another_until_one_comes_back_short() {
        let catalog = scripted(vec![
            CatalogCleanup {
                gc_entries: u64::from(BATCH),
                artifacts: 3,
                ..CatalogCleanup::default()
            },
            CatalogCleanup {
                gc_entries: 5,
                ..CatalogCleanup::default()
            },
            CatalogCleanup {
                gc_entries: 99,
                ..CatalogCleanup::default()
            },
        ]);
        let worker = CatalogCleanupWorker::new(catalog.clone());

        let removed = worker
            .run_scope(OrganizationScope::new(Id::from_string("org")), 42)
            .await
            .expect("cleanup");

        assert_eq!(removed, BATCH as usize + 3 + 5);
        assert_eq!(*catalog.cutoffs.lock().unwrap(), vec![42, 42]);
    }

    #[tokio::test]
    async fn a_backlog_is_left_for_later_rounds_after_the_batch_cap() {
        let full = CatalogCleanup {
            flush_commits: u64::from(BATCH),
            ..CatalogCleanup::default()
        };
        let catalog = scripted(vec![full; MAX_BATCHES + 5]);
        let worker = CatalogCleanupWorker::new(catalog.clone());

        worker
            .run_scope(OrganizationScope::new(Id::from_string("org")), 1)
            .await
            .expect("cleanup");

        assert_eq!(catalog.cutoffs.lock().unwrap().len(), MAX_BATCHES);
    }
}
