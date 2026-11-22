// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The write-ahead log in object storage.
//!
//! A write is acknowledged only after the WAL object holding it is in the object store and while
//! this process's writer lease holds ([`group`]). The node's disk holds nothing acknowledged.
//!
//! ```text
//! wal/v1/{org}/{dataset}/{writer_epoch:020}/{sequence:020}.wal   one group commit
//! ```
//!
//! - **Writer epoch:** taken from [`WalWriterRegistry`] at start, unique across the cluster. A
//!   flush publishes `(node, epoch, sequence range)` with a deterministic flush id, so publishing
//!   the same range twice is a no-op, and a node that lost its disk can never reuse an epoch.
//! - **Cleanup:** after a flush commits, the objects it covers are deleted
//!   ([`ObjectWal::cleanup`]); a failed deletion is retried and never blocks the flush.
//! - **Fenced:** when the registry says the writer was fenced (the lease lapsed and another node
//!   took over), this process stops acknowledging under it, takes a new epoch, and leaves the old
//!   epoch's objects to the node that claimed them.
//! - **Release:** a draining node fences itself and closes the streams it flushed completely;
//!   what is left is claimed at once by another node.

mod frame;
mod group;
mod lease;
mod memory;
pub(crate) mod metrics;

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use dashmap::DashMap;
use domain::storage::{
    DatasetTypeId, PhysicalDatasetId, WalCodecId, WalSequence, WalWriterRegistry, WriterEpoch,
    WriterNodeId,
};
use futures::{StreamExt, stream};
use kernel::{Error, Result, ids::Id};
use object_store::{ObjectStore, ObjectStoreExt};
use parking_lot::RwLock;
use postgres::cipher::CipherRootKey;
use settings::WalSettings;
use tokio::{sync::oneshot, task::JoinHandle};

use self::group::{CommitContext, DatasetLog, Pending};
pub use self::{
    frame::{decode as decode_object, encode as encode_object},
    group::{ApplyToBuffer, WalAppend},
    lease::WriterLease,
    memory::MemoryWalWriters,
};

/// Parallel deletions of one cleanup.
const DELETE_PARALLELISM: usize = 16;

/// The identity of one dataset's WAL, made by the dataset resolver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalStreamIdentity {
    pub organization_id: Id,
    pub dataset_id: PhysicalDatasetId,
    pub dataset_type: DatasetTypeId,
    pub wal_codec: WalCodecId,
}

/// Where an acknowledged write sits in the WAL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalAppendPosition {
    pub writer_epoch: WriterEpoch,
    pub sequence: WalSequence,
}

#[derive(Debug, Clone)]
pub struct ObjectWalConfig {
    pub lease: Duration,
    pub max_group_bytes: usize,
    pub put_timeout: Duration,
    pub put_attempts: u32,
}

impl ObjectWalConfig {
    pub fn from_settings(settings: &WalSettings) -> Self {
        Self {
            lease: settings.lease(),
            max_group_bytes: settings.max_group_bytes(),
            put_timeout: settings.put_timeout(),
            put_attempts: settings.put_attempts,
        }
    }
}

type LogKey = (u64, PhysicalDatasetId);

pub struct ObjectWal {
    registry: Arc<dyn WalWriterRegistry>,
    node_id: WriterNodeId,
    config: ObjectWalConfig,
    context: Arc<CommitContext>,
    lease: RwLock<Arc<WriterLease>>,
    logs: DashMap<LogKey, Arc<DatasetLog>>,
    /// No longer acknowledging: draining. The lease is still renewed until the release.
    stopping: AtomicBool,
    released: AtomicBool,
}

impl ObjectWal {
    /// Registers this process as a new writer and holds its lease.
    pub async fn start(
        store: Arc<dyn ObjectStore>,
        registry: Arc<dyn WalWriterRegistry>,
        node_id: impl Into<String>,
        config: ObjectWalConfig,
        cipher: Option<CipherRootKey>,
    ) -> Result<Self> {
        let node_id = WriterNodeId::new(node_id);
        let lease = Self::register(registry.as_ref(), &node_id, config.lease).await?;

        tracing::info!(
            node_id = %node_id,
            writer_epoch = %lease.epoch(),
            "object-storage WAL writer registered"
        );

        Ok(Self {
            context: Arc::new(CommitContext {
                store,
                cipher,
                max_group_bytes: config.max_group_bytes.max(1),
                put_timeout: config.put_timeout,
                put_attempts: config.put_attempts,
            }),
            registry,
            node_id,
            config,
            lease: RwLock::new(Arc::new(lease)),
            logs: DashMap::new(),
            stopping: AtomicBool::new(false),
            released: AtomicBool::new(false),
        })
    }

    async fn register(
        registry: &dyn WalWriterRegistry,
        node_id: &WriterNodeId,
        lease: Duration,
    ) -> Result<WriterLease> {
        let sent_at = Instant::now();
        let epoch = registry.register(node_id, lease).await?;
        Ok(WriterLease::granted(node_id.clone(), epoch, sent_at, lease))
    }

    pub fn node_id(&self) -> &str {
        self.node_id.as_str()
    }

    pub fn lease(&self) -> Arc<WriterLease> {
        self.lease.read().clone()
    }

    pub fn registry(&self) -> &Arc<dyn WalWriterRegistry> {
        &self.registry
    }

    pub fn store(&self) -> &Arc<dyn ObjectStore> {
        &self.context.store
    }

    pub fn cipher(&self) -> Option<&CipherRootKey> {
        self.context.cipher.as_ref()
    }

    fn log(&self, lease: &Arc<WriterLease>, identity: &WalStreamIdentity) -> Arc<DatasetLog> {
        self.logs
            .entry((lease.epoch().0, identity.dataset_id.clone()))
            .or_insert_with(|| Arc::new(DatasetLog::new(lease.clone(), identity.clone())))
            .clone()
    }

    /// Writes `append` to the WAL of `identity`'s dataset and puts it in the buffer. Returns once
    /// both are done; an error means the write was not acknowledged.
    pub async fn append(
        &self,
        identity: &WalStreamIdentity,
        append: WalAppend,
    ) -> Result<WalAppendPosition> {
        let lease = self.lease();

        if !lease.is_valid() {
            return Err(Error::unavailable(
                "this node does not hold its WAL writer lease; retry",
            ));
        }

        let log = self.log(&lease, identity);
        log.ensure_open(self.registry.as_ref()).await?;

        let (done, answer) = oneshot::channel();

        if log.enqueue(Pending { append, done }) {
            tokio::spawn(group::run(self.context.clone(), log));
        }

        answer
            .await
            .map_err(|_| Error::internal("WAL committer stopped before answering"))?
    }

    /// Deletes the objects of `writer_epoch`'s WAL of `dataset_id` up to `sequence`, after a flush
    /// covering them committed. The objects of an epoch this process no longer writes are left
    /// to the node that claimed them.
    pub async fn cleanup(
        &self,
        dataset_id: &PhysicalDatasetId,
        writer_epoch: WriterEpoch,
        sequence: u64,
    ) -> Result<()> {
        let Some(log) = self
            .logs
            .get(&(writer_epoch.0, dataset_id.clone()))
            .map(|entry| entry.clone())
        else {
            return Ok(());
        };

        let doomed = log.take_objects_up_to(sequence);
        let failed = delete_objects(self.store(), doomed).await;

        if failed.is_empty() {
            return Ok(());
        }

        let count = failed.len();
        log.restore_objects(failed);

        Err(Error::unavailable(format!(
            "{count} WAL objects of dataset {dataset_id} could not be deleted"
        )))
    }

    /// Extends the lease once. When the writer turns out fenced, takes a new epoch.
    pub async fn renew(&self) {
        if self.released.load(Ordering::Acquire) {
            return;
        }

        let lease = self.lease();
        let sent_at = Instant::now();

        match self.registry.renew(lease.epoch(), self.config.lease).await {
            Ok(true) => lease.extend(sent_at, self.config.lease),
            Ok(false) => self.replace_fenced(&lease).await,
            Err(error) => tracing::warn!(
                writer_epoch = %lease.epoch(),
                %error,
                "WAL writer lease renewal failed; writes are refused once the lease runs out"
            ),
        }
    }

    async fn replace_fenced(&self, fenced: &Arc<WriterLease>) {
        fenced.fence();
        metrics::inc_writer_fenced();

        tracing::error!(
            writer_epoch = %fenced.epoch(),
            "WAL writer was fenced (its lease lapsed and another node took it over); unflushed \
             rows of that epoch are published from its objects by that node"
        );

        if self.stopping.load(Ordering::Acquire) {
            return;
        }

        match Self::register(self.registry.as_ref(), &self.node_id, self.config.lease).await {
            Ok(lease) => {
                tracing::info!(writer_epoch = %lease.epoch(), "new WAL writer epoch registered");
                *self.lease.write() = Arc::new(lease);
                self.logs.retain(|(epoch, _), _| *epoch != fenced.epoch().0);
            }
            Err(error) => tracing::error!(%error, "registering a new WAL writer epoch failed"),
        }
    }

    /// Renews the lease every quarter lease until the WAL is released.
    pub fn spawn_lease_loop(self: &Arc<Self>) -> JoinHandle<()> {
        let wal = Arc::downgrade(self);
        let period = (self.config.lease / 4).max(Duration::from_millis(100));

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(period);
            interval.tick().await;

            loop {
                interval.tick().await;

                let Some(wal) = wal.upgrade() else {
                    return;
                };

                if wal.released.load(Ordering::Acquire) {
                    return;
                }

                wal.renew().await;
            }
        })
    }

    /// Stops acknowledging under the current epoch and waits up to `wait` for groups in flight.
    /// The first step of a release.
    pub async fn stop_acknowledging(&self, wait: Duration) {
        self.stopping.store(true, Ordering::Release);
        self.lease().fence();
        let deadline = Instant::now() + wait;

        while Instant::now() < deadline && self.logs.iter().any(|log| !log.is_idle()) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// The datasets the current epoch wrote.
    pub fn current_streams(&self) -> Vec<WalStreamIdentity> {
        let epoch = self.lease().epoch().0;

        self.logs
            .iter()
            .filter(|entry| entry.key().0 == epoch)
            .map(|entry| entry.identity.clone())
            .collect()
    }

    /// Deletes every object of the current epoch's WAL of `identity` and forgets the stream. Only
    /// for a dataset whose rows are all published.
    pub async fn close_flushed_stream(&self, identity: &WalStreamIdentity) -> Result<()> {
        let epoch = self.lease().epoch();
        self.cleanup(&identity.dataset_id, epoch, u64::MAX).await?;

        self.registry
            .close_stream(
                &identity.organization_id,
                &identity.dataset_id,
                epoch,
                epoch,
            )
            .await?;

        Ok(())
    }

    /// Ends the writer in the registry: its lease ends now and what it did not close is claimed
    /// by another node.
    pub async fn release(&self) -> Result<()> {
        self.stopping.store(true, Ordering::Release);
        self.released.store(true, Ordering::Release);
        let lease = self.lease();
        lease.fence();
        self.registry.release(lease.epoch()).await
    }
}

/// Deletes `objects`; returns those whose deletion failed. An object already gone counts as
/// deleted.
pub(crate) async fn delete_objects(
    store: &Arc<dyn ObjectStore>,
    objects: Vec<(u64, object_store::path::Path)>,
) -> Vec<(u64, object_store::path::Path)> {
    stream::iter(objects)
        .map(|(sequence, path)| async move {
            match store.delete(&path).await {
                Ok(()) | Err(object_store::Error::NotFound { .. }) => None,
                Err(error) => {
                    tracing::warn!(%path, %error, "WAL object deletion failed");
                    Some((sequence, path))
                }
            }
        })
        .buffer_unordered(DELETE_PARALLELISM)
        .filter_map(|failed| async move { failed })
        .collect()
        .await
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::{sync::Arc, time::Duration};

    use object_store::memory::InMemory;

    use super::{MemoryWalWriters, ObjectWal, ObjectWalConfig};

    pub(crate) fn config(lease: Duration) -> ObjectWalConfig {
        ObjectWalConfig {
            lease,
            max_group_bytes: 8 * 1024 * 1024,
            put_timeout: Duration::from_secs(5),
            put_attempts: 2,
        }
    }

    /// A WAL over an in-memory object store and registry.
    pub(crate) async fn memory_wal() -> (Arc<ObjectWal>, Arc<InMemory>, Arc<MemoryWalWriters>) {
        let store = Arc::new(InMemory::new());
        let registry = Arc::new(MemoryWalWriters::default());

        let wal = ObjectWal::start(
            store.clone(),
            registry.clone(),
            "node-test",
            config(Duration::from_secs(30)),
            None,
        )
        .await
        .unwrap();

        (Arc::new(wal), store, registry)
    }
}
