// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Group commit of one dataset's WAL.
//!
//! One committer task per dataset writes one object at a time. Writes that arrive while an object
//! is being written wait in the queue and go into the next object together, up to
//! `max_group_bytes`: no fixed window, so a lone write is not delayed and a busy dataset writes
//! large objects. Every write in an object shares its sequence.
//!
//! Once the object is written, and only while the writer's lease holds, the committer appends
//! the group to the memory buffer under the buffer's lock and then answers the writers. Appending
//! in sequence order, by the one task that writes the objects, means a flush generation only ever
//! covers sequences whose objects exist, and covers each one whole.

use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use bytes::Bytes;
use domain::storage::{WalSequence, WalStreamRow, WalWriterRegistry};
use kernel::{Error, Result};
use object_store::{ObjectStore, ObjectStoreExt, PutPayload, path::Path};
use postgres::cipher::CipherRootKey;
use tokio::sync::{OnceCell, oneshot};

use super::{
    WalAppendPosition, WalStreamIdentity, frame, lease::WriterLease, metrics as wal_metrics,
};
use crate::infra::{
    intake::{BufferWriter, DatasetBuffer, RecordBuilder},
    storage::layout::StorageLayout,
};

/// Appends a batch to the buffer once its WAL object is written. Called under the buffer lock.
pub type ApplyToBuffer =
    Box<dyn FnOnce(&mut RecordBuilder, &BufferWriter, WalSequence) -> anyhow::Result<()> + Send>;

/// One write waiting for its group: the WAL record, and how to put it in the buffer after.
pub struct WalAppend {
    pub payload: Vec<u8>,
    pub buffer: Arc<DatasetBuffer>,
    pub apply: ApplyToBuffer,
}

pub(super) struct Pending {
    pub append: WalAppend,
    pub done: oneshot::Sender<Result<WalAppendPosition>>,
}

#[derive(Default)]
struct Queue {
    items: VecDeque<Pending>,
    running: bool,
}

/// What every committer shares.
pub(super) struct CommitContext {
    pub store: Arc<dyn ObjectStore>,
    pub cipher: Option<CipherRootKey>,
    pub max_group_bytes: usize,
    pub put_timeout: Duration,
    pub put_attempts: u32,
}

/// One writer epoch's WAL of one dataset.
pub(super) struct DatasetLog {
    pub lease: Arc<WriterLease>,
    pub identity: WalStreamIdentity,
    opened: OnceCell<()>,
    queue: Mutex<Queue>,
    next_sequence: AtomicU64,
    /// Objects written, or perhaps written, and not deleted yet, by sequence.
    objects: Mutex<BTreeMap<u64, Path>>,
}

impl DatasetLog {
    pub fn new(lease: Arc<WriterLease>, identity: WalStreamIdentity) -> Self {
        Self {
            lease,
            identity,
            opened: OnceCell::new(),
            queue: Mutex::new(Queue::default()),
            next_sequence: AtomicU64::new(1),
            objects: Mutex::new(BTreeMap::new()),
        }
    }

    /// Records the stream in the registry before its first object, so a takeover can find it.
    pub async fn ensure_open(&self, registry: &dyn WalWriterRegistry) -> Result<()> {
        self.opened
            .get_or_try_init(|| async {
                registry
                    .open_stream(&WalStreamRow {
                        organization_id: self.identity.organization_id.clone(),
                        dataset_id: self.identity.dataset_id.clone(),
                        dataset_type: self.identity.dataset_type.clone(),
                        writer_node_id: self.lease.node_id().clone(),
                        writer_epoch: self.lease.epoch(),
                        publisher_epoch: self.lease.epoch(),
                    })
                    .await
                    .map_err(|error| Error::unavailable(format!("open WAL stream: {error}")))
            })
            .await
            .map(|_| ())
    }

    /// Queues a write; true when no committer runs and the caller must start one.
    pub fn enqueue(&self, pending: Pending) -> bool {
        let mut queue = self.queue.lock().expect("wal queue lock");
        queue.items.push_back(pending);
        !std::mem::replace(&mut queue.running, true)
    }

    /// The next group, or `None` (and the committer stops) when the queue is empty.
    fn take_group(&self, max_bytes: usize) -> Option<Vec<Pending>> {
        let mut queue = self.queue.lock().expect("wal queue lock");
        let mut group = Vec::new();
        let mut bytes = 0usize;

        while let Some(next) = queue.items.front() {
            let size = next.append.payload.len();

            if !group.is_empty() && bytes + size > max_bytes {
                break;
            }

            bytes += size;
            group.extend(queue.items.pop_front());
        }

        if group.is_empty() {
            queue.running = false;
            return None;
        }

        Some(group)
    }

    pub fn is_idle(&self) -> bool {
        let queue = self.queue.lock().expect("wal queue lock");
        !queue.running && queue.items.is_empty()
    }

    fn remember(&self, sequence: u64, path: Path) {
        self.objects
            .lock()
            .expect("wal objects lock")
            .insert(sequence, path);
    }

    /// Takes the objects up to and including `sequence` out for deletion.
    pub fn take_objects_up_to(&self, sequence: u64) -> Vec<(u64, Path)> {
        let mut objects = self.objects.lock().expect("wal objects lock");
        let kept = objects.split_off(&sequence.saturating_add(1));
        std::mem::replace(&mut *objects, kept).into_iter().collect()
    }

    /// Puts back objects whose deletion failed.
    pub fn restore_objects(&self, failed: Vec<(u64, Path)>) {
        self.objects
            .lock()
            .expect("wal objects lock")
            .extend(failed);
    }
}

/// Writes groups until the queue is empty.
pub(super) async fn run(context: Arc<CommitContext>, log: Arc<DatasetLog>) {
    while let Some(group) = log.take_group(context.max_group_bytes) {
        commit(&context, &log, group).await;
    }
}

async fn commit(context: &CommitContext, log: &DatasetLog, group: Vec<Pending>) {
    let started = Instant::now();
    let sequence = log.next_sequence.fetch_add(1, Ordering::SeqCst);

    let key = StorageLayout::wal_object_key(
        &log.identity.organization_id,
        &log.identity.dataset_id,
        log.lease.epoch().0,
        sequence,
    );

    let path = Path::from(key.as_str());

    let body = match frame::encode(
        group
            .iter()
            .map(|pending| pending.append.payload.as_slice()),
        context.cipher.as_ref(),
    ) {
        Ok(body) => Bytes::from(body),
        Err(error) => {
            let message = format!("encode WAL object: {error}");
            fail(group, || Error::internal(message.clone()));
            return;
        }
    };

    let bytes = body.len();

    // Remembered before the write: a write that times out may still land, and is deleted later.
    log.remember(sequence, path.clone());

    if let Err(error) = put(context, &path, body).await {
        wal_metrics::observe_group(wal_metrics::STORE_ERROR, started.elapsed().as_secs_f64(), 0);
        let message = format!("write WAL object {path}: {error}");
        fail(group, || Error::unavailable(message.clone()));
        return;
    }

    // Past the lease, a takeover may already be reading this writer's objects: what was not
    // acknowledged by now is never acknowledged.
    if !log.lease.is_valid() {
        wal_metrics::observe_group(wal_metrics::LEASE_LOST, started.elapsed().as_secs_f64(), 0);
        fail(group, || {
            Error::unavailable("WAL writer lease lapsed before the write was acknowledged")
        });
        return;
    }

    wal_metrics::observe_group(wal_metrics::OK, started.elapsed().as_secs_f64(), bytes);

    let position = WalAppendPosition {
        writer_epoch: log.lease.epoch(),
        sequence: WalSequence(sequence),
    };

    apply(log, group, position).await;
}

/// Appends the group to the buffers it names, in order, each buffer under its lock once.
async fn apply(log: &DatasetLog, group: Vec<Pending>, position: WalAppendPosition) {
    let writer = log.lease.writer();
    let mut remaining = group;

    while let Some(first) = remaining.first() {
        let buffer = first.append.buffer.clone();

        let (same, rest): (Vec<Pending>, Vec<Pending>) = remaining
            .into_iter()
            .partition(|pending| Arc::ptr_eq(&pending.append.buffer, &buffer));

        let mut records = buffer.records().lock().await;

        for pending in same {
            let result = (pending.append.apply)(&mut records, &writer, position.sequence)
                .map(|()| position)
                .map_err(|error| Error::internal(format!("buffer push: {error}")));

            let _ = pending.done.send(result);
        }

        drop(records);
        remaining = rest;
    }
}

fn fail(group: Vec<Pending>, error: impl Fn() -> Error) {
    for pending in group {
        let _ = pending.done.send(Err(error()));
    }
}

async fn put(context: &CommitContext, path: &Path, body: Bytes) -> anyhow::Result<()> {
    let attempts = context.put_attempts.max(1);
    let mut last_error = String::new();

    for attempt in 0..attempts {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(100 << attempt.min(5))).await;
        }

        let write = context.store.put(path, PutPayload::from(body.clone()));

        match tokio::time::timeout(context.put_timeout, write).await {
            Ok(Ok(_)) => return Ok(()),
            Ok(Err(error)) => last_error = error.to_string(),
            Err(_) => last_error = format!("timed out after {:?}", context.put_timeout),
        }
    }

    Err(anyhow::anyhow!(
        "{attempts} attempts failed, last: {last_error}"
    ))
}
