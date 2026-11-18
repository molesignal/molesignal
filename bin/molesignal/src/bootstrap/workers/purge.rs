// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Removing what was deleted: streams and organizations.
//!
//! Deleting a stream or an organization only marks it, so that the request stays short. On the
//! compactor primary, this worker then removes the rest in batches:
//!
//! - the catalog rows of deleted streams' datasets ([`DatasetPurgeRepository`]); the orphan scan
//!   finds their objects no longer referenced;
//! - a deleted organization's rows ([`OrganizationPurgeRepository`]) and its objects (every prefix
//!   of [`StorageLayout::organization_prefixes`]), and then the organization row. The row stays
//!   until a round finds nothing left at least [`GRACE`] after the deletion, so that writes
//!   already under way when the organization was deleted are removed as well.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use futures::{StreamExt, TryStreamExt, stream};
use kernel::{Error, Result, drain::DrainController, ids::Id, time::TimestampMicros};
use object_store::{ObjectStore, path::Path};
use postgres::repositories::{
    file_catalog::DatasetPurgeRepository,
    organizations::{DeletedOrganization, OrganizationPurgeRepository},
};
use tokio::task::JoinHandle;

use crate::{app::cluster::WorkOwnership, infra::storage::layout::StorageLayout};

const INTERVAL: Duration = Duration::from_secs(60);
const STARTUP_DELAY: Duration = Duration::from_secs(60);
/// Rows one delete statement takes.
const ROW_BATCH: i64 = 1_000;
/// Objects listed and deleted together.
const OBJECT_BATCH: usize = 1_000;
/// How long deleted datasets, or one organization, may take in a round before the next gets
/// its turn.
const ROUND_BUDGET: Duration = Duration::from_secs(45);
/// How long a deleted organization's row stays, at least.
const GRACE: Duration = Duration::from_secs(10 * 60);

/// What a round did for one organization.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Purged {
    pub rows: u64,
    pub objects: u64,
    /// The organization row is gone too.
    pub removed: bool,
}

pub struct PurgeWorker {
    datasets: Arc<dyn DatasetPurgeRepository>,
    organizations: Arc<dyn OrganizationPurgeRepository>,
    store: Arc<dyn ObjectStore>,
    ownership: Arc<WorkOwnership>,
    drain: Arc<DrainController>,
    grace: Duration,
}

impl PurgeWorker {
    pub fn new(
        datasets: Arc<dyn DatasetPurgeRepository>,
        organizations: Arc<dyn OrganizationPurgeRepository>,
        store: Arc<dyn ObjectStore>,
        ownership: Arc<WorkOwnership>,
        drain: Arc<DrainController>,
    ) -> Self {
        Self {
            datasets,
            organizations,
            store,
            ownership,
            drain,
            grace: GRACE,
        }
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            tokio::time::sleep(STARTUP_DELAY).await;

            while !self.drain.is_draining() {
                if self.ownership.is_primary().await
                    && let Err(error) = self.run_once().await
                {
                    tracing::warn!(%error, "purge failed");
                }

                tokio::time::sleep(INTERVAL).await;
            }
        })
    }

    #[tracing::instrument(
        name = "worker.purge",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "purge")
    )]
    async fn run_once(&self) -> Result<()> {
        let datasets = self
            .datasets
            .purge_deleting(ROW_BATCH, Instant::now() + ROUND_BUDGET)
            .await?;

        if datasets.rows_deleted > 0 {
            tracing::info!(
                rows = datasets.rows_deleted,
                "deleted streams' catalog rows removed"
            );
        }

        for organization in self.organizations.deleted().await? {
            if self.drain.is_draining() {
                break;
            }

            match self.purge_one(&organization).await {
                Ok(purged) if purged != Purged::default() => {
                    tracing::info!(
                        organization = %organization.id,
                        rows = purged.rows,
                        objects = purged.objects,
                        removed = purged.removed,
                        "deleted organization purged"
                    );
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(
                        organization = %organization.id,
                        %error,
                        "deleted organization purge stopped"
                    );
                }
            }
        }

        Ok(())
    }

    /// One round for one organization: its rows, its objects, and its row once nothing is left.
    async fn purge_one(&self, organization: &DeletedOrganization) -> Result<Purged> {
        let deadline = Instant::now() + ROUND_BUDGET;
        let rows = self
            .organizations
            .purge_rows(&organization.id, ROW_BATCH, deadline)
            .await?;

        let objects = self.purge_objects(&organization.id, deadline).await?;

        let grace = i64::try_from(self.grace.as_micros()).unwrap_or(i64::MAX);
        let settled = TimestampMicros::now()
            .0
            .saturating_sub(organization.deleted_at.0)
            >= grace;

        let removed = rows.finished && objects == Some(0) && settled;

        if removed {
            self.organizations.remove(&organization.id).await?;
        }

        Ok(Purged {
            rows: rows.rows_deleted,
            objects: objects.unwrap_or_default(),
            removed,
        })
    }

    /// Delete the organization's objects. Returns how many went, or `None` when the deadline
    /// stopped the sweep before every prefix was empty.
    async fn purge_objects(&self, organization_id: &Id, deadline: Instant) -> Result<Option<u64>> {
        let id = organization_id.as_str();

        // A path segment cannot hold `/`: such an id has no prefix of its own.
        if id.is_empty() || id.contains('/') {
            return Ok(Some(0));
        }

        let mut deleted = 0;

        for prefix in StorageLayout::organization_prefixes(organization_id) {
            let prefix = Path::from(prefix.as_str());

            loop {
                let page = self
                    .store
                    .list(Some(&prefix))
                    .take(OBJECT_BATCH)
                    .map_ok(|object| object.location)
                    .try_collect::<Vec<_>>()
                    .await
                    .map_err(|error| Error::internal(format!("list {prefix}: {error}")))?;

                let listed = page.len();

                let mut results = self
                    .store
                    .delete_stream(stream::iter(page.into_iter().map(Ok)).boxed());

                while let Some(result) = results.next().await {
                    match result {
                        Ok(_) => deleted += 1,
                        Err(object_store::Error::NotFound { .. }) => {}
                        Err(error) => {
                            return Err(Error::internal(format!("delete under {prefix}: {error}")));
                        }
                    }
                }

                if listed < OBJECT_BATCH {
                    break;
                }

                if Instant::now() >= deadline {
                    return Ok(None);
                }
            }

            if Instant::now() >= deadline {
                return Ok(None);
            }
        }

        Ok(Some(deleted))
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use bytes::Bytes;
    use domain::cluster::PeerRole;
    use object_store::{ObjectStoreExt as _, PutPayload, memory::InMemory};
    use parking_lot::Mutex;
    use postgres::repositories::organizations::PurgeProgress;

    use super::*;

    /// A purge that has `rows` rows to delete in its first pass.
    struct Purge {
        deleted_at: TimestampMicros,
        rows: Mutex<u64>,
        removed: Mutex<Vec<Id>>,
    }

    impl Purge {
        fn new(deleted_at: TimestampMicros, rows: u64) -> Arc<Self> {
            Arc::new(Self {
                deleted_at,
                rows: Mutex::new(rows),
                removed: Mutex::default(),
            })
        }

        fn organization(&self) -> DeletedOrganization {
            DeletedOrganization {
                id: Id::from_string("gone"),
                deleted_at: self.deleted_at,
            }
        }
    }

    #[async_trait]
    impl OrganizationPurgeRepository for Purge {
        async fn deleted(&self) -> Result<Vec<DeletedOrganization>> {
            Ok(vec![self.organization()])
        }

        async fn purge_rows(&self, _: &Id, _: i64, _: Instant) -> Result<PurgeProgress> {
            let rows = std::mem::take(&mut *self.rows.lock());

            Ok(PurgeProgress {
                rows_deleted: rows,
                finished: rows == 0,
            })
        }

        async fn remove(&self, organization_id: &Id) -> Result<()> {
            self.removed.lock().push(organization_id.clone());
            Ok(())
        }
    }

    /// No stream is being deleted.
    struct Datasets;

    #[async_trait]
    impl DatasetPurgeRepository for Datasets {
        async fn purge_deleting(&self, _: i64, _: Instant) -> Result<PurgeProgress> {
            Ok(PurgeProgress {
                rows_deleted: 0,
                finished: true,
            })
        }
    }

    fn worker(purge: Arc<Purge>, store: Arc<InMemory>, grace: Duration) -> PurgeWorker {
        let mut worker = PurgeWorker::new(
            Arc::new(Datasets),
            purge,
            store,
            WorkOwnership::solo(PeerRole::Compactor),
            Arc::new(DrainController::new()),
        );

        worker.grace = grace;
        worker
    }

    async fn store_with(keys: &[&str]) -> Arc<InMemory> {
        let store = Arc::new(InMemory::new());

        for key in keys {
            store
                .put(
                    &Path::from(*key),
                    PutPayload::from(Bytes::from_static(b"x")),
                )
                .await
                .expect("put");
        }

        store
    }

    async fn keys(store: &InMemory) -> Vec<String> {
        let mut keys = store
            .list(None)
            .map_ok(|object| object.location.to_string())
            .try_collect::<Vec<_>>()
            .await
            .expect("list");

        keys.sort();
        keys
    }

    #[tokio::test]
    async fn the_organization_goes_once_a_round_finds_nothing_left() {
        let store = store_with(&[
            "artifacts/v1/gone/logs/dataset/p-0-00/segment/data.parquet",
            "blobs/v1/gone/rum/app/session/0000000001-hash.ndjson.zst",
            "gone/synthetics/artifacts/task/screenshot",
            "artifacts/v1/gone2/logs/dataset/p-0-00/segment/data.parquet",
            "artifacts/v1/kept/logs/dataset/p-0-00/segment/data.parquet",
        ])
        .await;

        let purge = Purge::new(TimestampMicros(0), 5);
        let worker = worker(purge.clone(), store.clone(), Duration::ZERO);

        let first = worker
            .purge_one(&purge.organization())
            .await
            .expect("first round");

        assert_eq!(
            first,
            Purged {
                rows: 5,
                objects: 3,
                removed: false,
            }
        );
        assert_eq!(
            keys(&store).await,
            [
                "artifacts/v1/gone2/logs/dataset/p-0-00/segment/data.parquet",
                "artifacts/v1/kept/logs/dataset/p-0-00/segment/data.parquet",
            ]
        );

        let second = worker
            .purge_one(&purge.organization())
            .await
            .expect("second round");

        assert_eq!(
            second,
            Purged {
                rows: 0,
                objects: 0,
                removed: true,
            }
        );
        assert_eq!(*purge.removed.lock(), [Id::from_string("gone")]);
    }

    #[tokio::test]
    async fn the_row_stays_through_the_grace() {
        let purge = Purge::new(TimestampMicros::now(), 0);
        let worker = worker(purge.clone(), store_with(&[]).await, GRACE);

        let purged = worker
            .purge_one(&purge.organization())
            .await
            .expect("round");

        assert_eq!(purged, Purged::default());
        assert!(purge.removed.lock().is_empty());
    }
}
