// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Writing the candidate log, tracking settled spans, deleting what nothing needs any more.
//!
//! One committer per writer epoch writes one object at a time; appends that arrive meanwhile go
//! into the next object together, so a busy intake writes few large objects and a lone request is
//! not delayed. An append is answered only once its object is stored and the lease still holds.

use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::Arc,
    time::Duration,
};

use domain::storage::WalStreamRow;
use kernel::{Error, Result, ids::Id};
use object_store::{ObjectStore, ObjectStoreExt, PutPayload, path::Path};
use parking_lot::Mutex;
use postgres::cipher::CipherRootKey;
use signals::{tail_sampling::ForceKeep, trace_normalization::CanonicalSpan};
use tokio::{
    sync::{OnceCell, oneshot},
    task::JoinHandle,
};

use super::{
    SpanOrigin, SpanSettlement, candidate_log_id, candidate_log_type, object_path,
    progress::Progress,
    record::{LogRecord, force_keep_code},
};
use crate::infra::intake::object_wal::{ObjectWal, WriterLease, delete_objects, encode_object};

#[derive(Debug, Clone, Copy)]
pub struct TraceLogConfig {
    /// Bytes of appends one object takes at most (one append is never split).
    pub max_group_bytes: usize,
    pub put_timeout: Duration,
    pub put_attempts: u32,
    /// How often settled spans are recorded when no append carries them.
    pub marker_interval: Duration,
}

impl Default for TraceLogConfig {
    fn default() -> Self {
        Self {
            max_group_bytes: 8 * 1024 * 1024,
            put_timeout: Duration::from_secs(5),
            put_attempts: 3,
            marker_interval: Duration::from_secs(1),
        }
    }
}

/// What the committers share.
struct Context {
    store: Arc<dyn ObjectStore>,
    cipher: Option<CipherRootKey>,
    config: TraceLogConfig,
}

pub struct TraceCandidateLog {
    wal: Arc<ObjectWal>,
    system_org: Id,
    context: Arc<Context>,
    /// The current epoch's log, and older ones while their spans settle.
    epochs: Mutex<BTreeMap<u64, Arc<EpochLog>>>,
}

impl TraceCandidateLog {
    pub fn new(wal: Arc<ObjectWal>, system_org: Id, config: TraceLogConfig) -> Self {
        let context = Arc::new(Context {
            store: wal.store().clone(),
            cipher: wal.cipher().cloned(),
            config,
        });

        Self {
            wal,
            system_org,
            context,
            epochs: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn wal(&self) -> &Arc<ObjectWal> {
        &self.wal
    }

    pub(super) fn cipher(&self) -> Option<&CipherRootKey> {
        self.context.cipher.as_ref()
    }

    /// Writes one tenant stream's spans to the log; returns where each one sits, in order. An
    /// error means nothing was acknowledged.
    pub async fn append(
        &self,
        org_id: &str,
        stream: &str,
        force_keep: ForceKeep,
        spans: &[CanonicalSpan],
    ) -> Result<Vec<SpanOrigin>> {
        if spans.is_empty() {
            return Ok(Vec::new());
        }

        let lease = self.wal.lease();

        if !lease.is_valid() {
            return Err(Error::unavailable(
                "this node does not hold its WAL writer lease; retry",
            ));
        }

        let log = self.epoch_log(&lease);
        log.ensure_open(self).await?;

        let record = LogRecord::Candidates {
            org_id: org_id.to_owned(),
            stream: stream.to_owned(),
            force_keep: force_keep_code(force_keep),
            spans: spans.to_vec(),
        }
        .encode()
        .map_err(|error| Error::internal(format!("encode trace candidates: {error}")))?;

        let span_count = u32::try_from(spans.len())
            .map_err(|_| Error::invalid("too many spans in one trace request"))?;

        let (done, answer) = oneshot::channel();

        if log.enqueue(PendingAppend {
            record,
            spans: span_count,
            done,
        }) {
            tokio::spawn(log.clone().run(self.context.clone()));
        }

        let (sequence, record) = answer
            .await
            .map_err(|_| Error::internal("trace candidate log stopped before answering"))??;

        let epoch = lease.epoch().0;

        Ok((0..span_count)
            .map(|span| SpanOrigin {
                epoch,
                sequence,
                record,
                span,
            })
            .collect())
    }

    fn epoch_log(&self, lease: &Arc<WriterLease>) -> Arc<EpochLog> {
        self.epochs
            .lock()
            .entry(lease.epoch().0)
            .or_insert_with(|| Arc::new(EpochLog::new(lease.clone())))
            .clone()
    }

    /// Records the settled spans not recorded yet, now.
    pub async fn write_markers(&self) -> Result<()> {
        let lease = self.wal.lease();
        let log = self.epochs.lock().get(&lease.epoch().0).cloned();

        match log {
            Some(log) => log.write_markers(&self.context).await,
            None => Ok(()),
        }
    }

    /// Records settled spans every `marker_interval` until the log is dropped.
    pub fn spawn_marker_loop(self: &Arc<Self>) -> JoinHandle<()> {
        let log = Arc::downgrade(self);
        let period = self
            .context
            .config
            .marker_interval
            .max(Duration::from_millis(50));

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(period);
            interval.tick().await;

            loop {
                interval.tick().await;

                let Some(log) = log.upgrade() else {
                    return;
                };

                if let Err(error) = log.write_markers().await {
                    tracing::warn!(%error, "recording settled trace spans failed; retried");
                }
            }
        })
    }

    /// Spans of the current epoch not settled yet.
    pub fn unsettled(&self) -> u64 {
        let lease = self.wal.lease();

        self.epochs
            .lock()
            .get(&lease.epoch().0)
            .map_or(0, |log| log.unsettled())
    }

    /// The last step before the WAL writer is released (drain): records what settled, and when
    /// every span is settled deletes the objects and closes the log's row. Otherwise the row
    /// stays, and another node replays the unsettled spans once the writer is released.
    pub async fn release(&self) -> Result<()> {
        let lease = self.wal.lease();
        let Some(log) = self.epochs.lock().get(&lease.epoch().0).cloned() else {
            return Ok(());
        };

        if log.opened.get().is_none() {
            return Ok(());
        }

        log.write_markers(&self.context).await?;
        let unsettled = log.unsettled();

        if unsettled > 0 {
            tracing::warn!(
                unsettled,
                writer_epoch = %lease.epoch(),
                "trace candidate log released with unsettled spans; another node replays them"
            );
            return Ok(());
        }

        let doomed = log.progress.lock().take_all();
        let failed = delete_objects(&self.context.store, doomed).await;

        if !failed.is_empty() {
            let count = failed.len();
            log.progress.lock().restore(failed);
            return Err(Error::unavailable(format!(
                "{count} trace candidate log objects could not be deleted"
            )));
        }

        self.wal
            .registry()
            .close_stream(
                &self.system_org,
                &candidate_log_id(),
                lease.epoch(),
                lease.epoch(),
            )
            .await?;

        Ok(())
    }
}

impl SpanSettlement for TraceCandidateLog {
    fn settle(&self, origins: &[SpanOrigin]) {
        let mut by_epoch: HashMap<u64, Vec<SpanOrigin>> = HashMap::new();

        for origin in origins {
            by_epoch.entry(origin.epoch).or_default().push(*origin);
        }

        let current = self.wal.lease().epoch().0;

        for (epoch, origins) in by_epoch {
            let Some(log) = self.epochs.lock().get(&epoch).cloned() else {
                continue;
            };

            log.settle(&origins, &self.context);

            if epoch != current && log.unsettled() == 0 {
                self.epochs.lock().remove(&epoch);
            }
        }
    }
}

struct PendingAppend {
    record: Vec<u8>,
    spans: u32,
    done: oneshot::Sender<Result<(u64, u32)>>,
}

#[derive(Default)]
struct GroupQueue {
    items: VecDeque<PendingAppend>,
    running: bool,
}

/// One writer epoch's candidate log.
struct EpochLog {
    lease: Arc<WriterLease>,
    opened: OnceCell<()>,
    queue: Mutex<GroupQueue>,
    /// Shared with deletions that finish in the background.
    progress: Arc<Mutex<Progress>>,
}

impl EpochLog {
    fn new(lease: Arc<WriterLease>) -> Self {
        Self {
            lease,
            opened: OnceCell::new(),
            queue: Mutex::new(GroupQueue::default()),
            progress: Arc::new(Mutex::new(Progress::default())),
        }
    }

    /// Records the log in the registry before its first object, so a takeover can find it.
    async fn ensure_open(&self, log: &TraceCandidateLog) -> Result<()> {
        self.opened
            .get_or_try_init(|| async {
                log.wal
                    .registry()
                    .open_stream(&WalStreamRow {
                        organization_id: log.system_org.clone(),
                        dataset_id: candidate_log_id(),
                        dataset_type: candidate_log_type(),
                        writer_node_id: self.lease.node_id().clone(),
                        writer_epoch: self.lease.epoch(),
                        publisher_epoch: self.lease.epoch(),
                    })
                    .await
                    .map_err(|error| {
                        Error::unavailable(format!("open trace candidate log: {error}"))
                    })
            })
            .await
            .map(|_| ())
    }

    /// Queues an append; true when no committer runs and the caller must start one.
    fn enqueue(&self, pending: PendingAppend) -> bool {
        let mut queue = self.queue.lock();
        queue.items.push_back(pending);
        !std::mem::replace(&mut queue.running, true)
    }

    fn take_group(&self, max_bytes: usize) -> Option<Vec<PendingAppend>> {
        let mut queue = self.queue.lock();

        if queue.items.is_empty() {
            queue.running = false;
            return None;
        }

        let mut group = Vec::new();
        let mut bytes = 0;

        while let Some(next) = queue.items.front() {
            if !group.is_empty() && bytes + next.record.len() > max_bytes {
                break;
            }

            bytes += next.record.len();
            group.extend(queue.items.pop_front());
        }

        Some(group)
    }

    async fn run(self: Arc<Self>, context: Arc<Context>) {
        while let Some(group) = self.take_group(context.config.max_group_bytes) {
            self.commit(&context, group).await;
        }
    }

    /// Writes one group, with the settled-span markers waiting, as one object.
    async fn commit(&self, context: &Context, group: Vec<PendingAppend>) {
        let (sequence, markers) = {
            let mut progress = self.progress.lock();
            (progress.allocate(), std::mem::take(&mut progress.settled))
        };

        let marker = (!markers.is_empty())
            .then(|| LogRecord::Settled {
                spans: markers.clone(),
            })
            .map(|record| record.encode())
            .transpose();

        let path = object_path(self.lease.epoch().0, sequence);

        let written = match marker {
            Ok(marker) => {
                let records = group
                    .iter()
                    .map(|pending| pending.record.as_slice())
                    .chain(marker.as_deref());

                match encode_object(records, context.cipher.as_ref()) {
                    Ok(body) => put(context, &path, body).await,
                    Err(error) => Err(error),
                }
            }
            Err(error) => Err(error),
        };

        if let Err(error) = written {
            self.progress.lock().settled.extend(markers);
            tracing::warn!(%path, %error, "trace candidate log object could not be stored");
            fail(group, "the trace candidate log could not be stored; retry");
            return;
        }

        let mut progress = self.progress.lock();
        progress.objects.insert(sequence, path);

        // Stored, but the lease lapsed meanwhile: another node may own the epoch already.
        if !self.lease.is_valid() {
            drop(progress);
            fail(group, "this node lost its WAL writer lease; retry");
            return;
        }

        let spans = group.iter().map(|pending| pending.spans).sum::<u32>();

        if spans > 0 {
            progress.outstanding.insert(sequence, spans);
        }

        drop(progress);

        for (record, pending) in group.into_iter().enumerate() {
            let record = u32::try_from(record).unwrap_or(u32::MAX);
            let _ = pending.done.send(Ok((sequence, record)));
        }
    }

    async fn write_markers(&self, context: &Context) -> Result<()> {
        let (sequence, markers) = {
            let mut progress = self.progress.lock();

            if progress.settled.is_empty() {
                return Ok(());
            }

            (progress.allocate(), std::mem::take(&mut progress.settled))
        };

        let path = object_path(self.lease.epoch().0, sequence);

        let written = LogRecord::Settled {
            spans: markers.clone(),
        }
        .encode()
        .and_then(|record| encode_object([record.as_slice()], context.cipher.as_ref()));

        let written = match written {
            Ok(body) => put(context, &path, body).await,
            Err(error) => Err(error),
        };

        if let Err(error) = written {
            self.progress.lock().settled.extend(markers);
            return Err(Error::unavailable(format!(
                "store settled trace spans: {error}"
            )));
        }

        let doomed = {
            let mut progress = self.progress.lock();
            progress.objects.insert(sequence, path);
            self.deletable(&mut progress)
        };

        self.delete(context, doomed).await;
        Ok(())
    }

    fn settle(&self, origins: &[SpanOrigin], context: &Arc<Context>) {
        let doomed = {
            let mut progress = self.progress.lock();

            for origin in origins {
                let Some(left) = progress.outstanding.get_mut(&origin.sequence) else {
                    continue;
                };

                *left = left.saturating_sub(1);

                if *left == 0 {
                    progress.outstanding.remove(&origin.sequence);
                }

                progress
                    .settled
                    .push((origin.sequence, origin.record, origin.span));
            }

            self.deletable(&mut progress)
        };

        if doomed.is_empty() {
            return;
        }

        let store = context.store.clone();
        let progress = self.progress.clone();

        tokio::spawn(async move {
            let failed = delete_objects(&store, doomed).await;

            if !failed.is_empty() {
                progress.lock().restore(failed);
            }
        });
    }

    /// Takes the objects nothing needs any more, and forgets markers for spans whose objects go.
    /// Nothing is taken once the lease lapsed: the objects belong to the node that claims them.
    fn deletable(&self, progress: &mut Progress) -> Vec<(u64, Path)> {
        let low = progress.low_watermark();
        progress.settled.retain(|(sequence, _, _)| *sequence >= low);

        if !self.lease.is_valid() {
            return Vec::new();
        }

        progress.take_deletable()
    }

    async fn delete(&self, context: &Context, doomed: Vec<(u64, Path)>) {
        if doomed.is_empty() {
            return;
        }

        let failed = delete_objects(&context.store, doomed).await;

        if !failed.is_empty() {
            self.progress.lock().restore(failed);
        }
    }

    fn unsettled(&self) -> u64 {
        self.progress
            .lock()
            .outstanding
            .values()
            .map(|left| u64::from(*left))
            .sum()
    }
}

fn fail(group: Vec<PendingAppend>, message: &'static str) {
    for pending in group {
        let _ = pending.done.send(Err(Error::unavailable(message)));
    }
}

async fn put(context: &Context, path: &Path, body: Vec<u8>) -> anyhow::Result<()> {
    let attempts = context.config.put_attempts.max(1);
    let payload = bytes::Bytes::from(body);
    let mut last_error = String::new();

    for attempt in 0..attempts {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(100 << attempt.min(5))).await;
        }

        let write = context.store.put(path, PutPayload::from(payload.clone()));

        match tokio::time::timeout(context.config.put_timeout, write).await {
            Ok(Ok(_)) => return Ok(()),
            Ok(Err(error)) => last_error = error.to_string(),
            Err(_) => last_error = format!("timed out after {:?}", context.config.put_timeout),
        }
    }

    anyhow::bail!("{attempts} attempts failed, last: {last_error}")
}

#[cfg(test)]
mod tests {
    use futures::TryStreamExt;
    use object_store::ObjectStore;

    use super::*;
    use crate::{
        app::trace::durable::epoch_prefix, infra::intake::object_wal::test_support::memory_wal,
    };

    async fn objects(store: &dyn ObjectStore, epoch: u64) -> usize {
        store
            .list(Some(&epoch_prefix(epoch)))
            .try_collect::<Vec<_>>()
            .await
            .unwrap()
            .len()
    }

    async fn wait_for_objects(store: &dyn ObjectStore, epoch: u64, expected: usize) {
        tokio::time::timeout(Duration::from_secs(3), async {
            while objects(store, epoch).await != expected {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("object count reached in time");
    }

    fn spans(count: usize) -> Vec<CanonicalSpan> {
        signals::trace_fixtures::canonical_http_trace()
            .into_iter()
            .cycle()
            .take(count)
            .collect()
    }

    #[tokio::test]
    async fn an_object_goes_once_every_span_in_it_is_settled() {
        let (wal, store, registry) = memory_wal().await;
        let epoch = wal.lease().epoch().0;
        let log = TraceCandidateLog::new(wal, Id("sys".into()), TraceLogConfig::default());

        let origins = log
            .append("org", "default", ForceKeep::None, &spans(2))
            .await
            .unwrap();

        assert_eq!(origins.len(), 2);
        assert_eq!(objects(store.as_ref(), epoch).await, 1);
        assert_eq!(log.unsettled(), 2);
        assert!(
            registry
                .streams()
                .iter()
                .any(super::super::is_candidate_log)
        );

        log.settle(&origins[..1]);
        assert_eq!(log.unsettled(), 1);
        assert_eq!(
            objects(store.as_ref(), epoch).await,
            1,
            "one span still needs it"
        );

        log.settle(&origins[1..]);
        assert_eq!(log.unsettled(), 0);
        wait_for_objects(store.as_ref(), epoch, 0).await;
    }

    #[tokio::test]
    async fn markers_record_what_settled_while_other_spans_remain() {
        let (wal, store, _registry) = memory_wal().await;
        let epoch = wal.lease().epoch().0;
        let log = TraceCandidateLog::new(wal, Id("sys".into()), TraceLogConfig::default());

        let origins = log
            .append("org", "default", ForceKeep::None, &spans(2))
            .await
            .unwrap();

        log.settle(&origins[..1]);
        log.write_markers().await.unwrap();

        assert_eq!(
            objects(store.as_ref(), epoch).await,
            2,
            "the spans and a marker"
        );
        assert_eq!(log.unsettled(), 1);
    }

    #[tokio::test]
    async fn a_release_with_everything_settled_leaves_nothing_behind() {
        let (wal, store, registry) = memory_wal().await;
        let epoch = wal.lease().epoch().0;
        let log = TraceCandidateLog::new(wal, Id("sys".into()), TraceLogConfig::default());

        let origins = log
            .append("org", "default", ForceKeep::None, &spans(3))
            .await
            .unwrap();

        log.settle(&origins[..2]);
        log.release().await.unwrap();
        assert!(
            registry
                .streams()
                .iter()
                .any(super::super::is_candidate_log),
            "unsettled: kept"
        );

        log.settle(&origins[2..]);
        log.release().await.unwrap();
        wait_for_objects(store.as_ref(), epoch, 0).await;
        assert!(
            !registry
                .streams()
                .iter()
                .any(super::super::is_candidate_log)
        );
    }

    #[tokio::test]
    async fn nothing_is_acknowledged_without_the_lease() {
        let (wal, _store, _registry) = memory_wal().await;
        wal.lease().fence();
        let log = TraceCandidateLog::new(wal, Id("sys".into()), TraceLogConfig::default());

        let refused = log
            .append("org", "default", ForceKeep::None, &spans(1))
            .await;

        assert!(refused.is_err());
    }
}
