// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

mod failover;
mod faulty_store;

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use async_trait::async_trait;
use domain::{
    intake::RawEvent,
    storage::{FileCatalog, WalWriterRegistry, is_stale_writer_epoch, primary_dataset_type},
    stream::{
        FieldDef, FieldType, Retention, Schema, StreamDefinition, StreamIndexType, StreamType,
    },
};
use futures::StreamExt;
use kernel::{Error, health::Probe, ids::Id, time::TimestampMicros};
use object_store::{ObjectStore, path::Path};
use serde_json::json;

use self::faulty_store::FaultyStore;
use super::*;
use crate::infra::intake::{
    MemoryWalWriters, ObjectWal,
    dataset_resolver::test_support::{StubFileCatalog, test_catalog_and_resolver},
    object_wal::test_support::config,
};

/// 只有一条流的仓库；`gone` 置位后这条流对读取表现为已被删除。
struct OneStreamRepository {
    stream: StreamDefinition,
    gone: Arc<AtomicBool>,
}

#[async_trait]
impl StreamRepository for OneStreamRepository {
    async fn create(&self, definition: StreamDefinition) -> Result<StreamDefinition> {
        Ok(definition)
    }

    async fn update_schema(&self, _id: &Id, _schema: Schema) -> Result<()> {
        Ok(())
    }

    async fn get(
        &self,
        organization_id: &Id,
        name: &str,
        stream_type: StreamType,
    ) -> Result<StreamDefinition> {
        if !self.gone.load(Ordering::SeqCst)
            && self.stream.org_id == *organization_id
            && self.stream.name == name
            && self.stream.stream_type == stream_type
        {
            Ok(self.stream.clone())
        } else {
            Err(Error::not_found(format!("stream {name}")))
        }
    }

    async fn list(&self, organization_id: &Id) -> Result<Vec<StreamDefinition>> {
        Ok((self.stream.org_id == *organization_id)
            .then(|| self.stream.clone())
            .into_iter()
            .collect())
    }

    async fn delete(&self, _id: &Id) -> Result<()> {
        Ok(())
    }
}

fn stream_definition() -> StreamDefinition {
    StreamDefinition {
        id: Id::new(),
        org_id: Id::from_string("org-a"),
        name: "app".into(),
        stream_type: StreamType::LOGS,
        schema: Schema {
            fields: vec![FieldDef {
                name: "level".into(),
                data_type: FieldType::Utf8,
                nullable: false,
                index_type: StreamIndexType::None,
                encrypted: false,
            }],
        },
        retention: Some(Retention { days: 7 }),
        created_at: TimestampMicros::now(),
        updated_at: TimestampMicros::now(),
    }
}

fn batch(stream: &StreamDefinition, start_micros: i64, rows: usize) -> IntakeBatch {
    IntakeBatch {
        batch_id: Id::new(),
        org_id: stream.org_id.clone(),
        stream: stream.name.clone(),
        stream_type: stream.stream_type,
        events: (0..rows)
            .map(|offset| RawEvent {
                timestamp: TimestampMicros(start_micros + offset as i64),
                fields: [("level".to_string(), json!("info"))].into_iter().collect(),
            })
            .collect(),
        received_at: TimestampMicros::now(),
    }
}

struct WorkerFixture {
    worker: Option<Arc<IntakeWorker>>,
    catalog: Arc<StubFileCatalog>,
    resolver: Arc<DatasetResolver>,
    buffer: Arc<BufferPool>,
    /// One object store for the WAL and the segments, as in a deployment.
    store: Arc<FaultyStore>,
    registry: Arc<MemoryWalWriters>,
    streams: Arc<dyn StreamRepository>,
    stream: StreamDefinition,
    stream_gone: Arc<AtomicBool>,
    settings: IntakeSettings,
    /// What nodes started from now on hold before a takeover publishes.
    takeover_byte_cap: usize,
}

async fn fixture(settings: IntakeSettings) -> WorkerFixture {
    fixture_with_budget(settings, usize::MAX).await
}

async fn fixture_with_budget(
    settings: IntakeSettings,
    memory_budget_bytes: usize,
) -> WorkerFixture {
    let stream = stream_definition();
    let (catalog, resolver) = test_catalog_and_resolver();
    let registry = Arc::new(MemoryWalWriters::default());
    catalog.fence_with(registry.clone());
    let stream_gone = Arc::new(AtomicBool::new(false));
    let store = Arc::new(FaultyStore::default());
    let buffer = Arc::new(BufferPool::with_memory_limit_bytes(memory_budget_bytes));

    let streams: Arc<dyn StreamRepository> = Arc::new(OneStreamRepository {
        stream: stream.clone(),
        gone: stream_gone.clone(),
    });

    let mut fixture = WorkerFixture {
        worker: None,
        catalog,
        resolver,
        buffer: buffer.clone(),
        store,
        registry,
        streams,
        stream,
        stream_gone,
        settings,
        takeover_byte_cap: DEFAULT_TAKEOVER_BYTE_CAP,
    };

    fixture.worker = Some(fixture.node_with(buffer, Duration::from_secs(30)).await);
    fixture
}

impl WorkerFixture {
    /// The node every test starts with.
    fn worker(&self) -> &Arc<IntakeWorker> {
        self.worker.as_ref().expect("fixture starts a node")
    }

    /// Another node over the same object store, registry and catalog: a new WAL writer epoch and
    /// memory of its own. A node keeps nothing on disk, so this is also a restarted node.
    async fn node(&self) -> Arc<IntakeWorker> {
        self.node_with(Arc::new(BufferPool::new()), Duration::from_secs(30))
            .await
    }

    async fn node_with(&self, buffer: Arc<BufferPool>, lease: Duration) -> Arc<IntakeWorker> {
        let wal = ObjectWal::start(
            self.store.clone(),
            self.registry.clone(),
            "node-test",
            config(lease),
            None,
        )
        .await
        .unwrap();

        Arc::new(
            IntakeWorker::new(
                Arc::new(wal),
                buffer,
                self.streams.clone(),
                self.resolver.clone(),
                self.catalog.clone() as Arc<dyn FileCatalog>,
                Arc::new(ParquetWriter::new(self.store.clone())),
                Arc::new(Probe::new()),
                self.settings.clone(),
            )
            .with_takeover_byte_cap(self.takeover_byte_cap),
        )
    }
}

/// How many objects are under `prefix`: `artifacts/` for segments, `wal/` for the WAL.
async fn objects(store: &FaultyStore, prefix: &str) -> usize {
    store
        .list(Some(&Path::from(prefix)))
        .collect::<Vec<_>>()
        .await
        .len()
}

async fn raw_dataset_key(fixture: &WorkerFixture) -> PhysicalDatasetId {
    fixture
        .resolver
        .resolve(
            &fixture.stream,
            primary_dataset_type(fixture.stream.stream_type).unwrap(),
        )
        .await
        .unwrap()
        .dataset
        .id
        .clone()
}

#[tokio::test]
async fn successful_flush_atomically_commits_and_retires_generation() {
    let fixture = fixture(IntakeSettings::default()).await;
    fixture
        .worker()
        .write(batch(&fixture.stream, 1_000_000, 2))
        .await
        .unwrap();
    let key = raw_dataset_key(&fixture).await;
    assert!(fixture.buffer.reserved_bytes() > 0);

    fixture.worker().flush_one(&key).await.unwrap();

    assert_eq!(fixture.catalog.committed_rows(), vec![2]);
    assert_eq!(fixture.buffer.reserved_bytes(), 0);
    assert!(
        fixture
            .buffer
            .get(&key)
            .unwrap()
            .records()
            .lock()
            .await
            .is_empty()
    );
    assert_eq!(objects(&fixture.store, "artifacts/").await, 1);
    assert_eq!(
        objects(&fixture.store, "wal/").await,
        0,
        "the committed flush deleted the WAL objects it covers"
    );
}

#[tokio::test]
async fn catalog_failure_keeps_generation_visible_and_retries_before_new_rows() {
    let fixture = fixture(IntakeSettings::default()).await;
    fixture
        .worker()
        .write(batch(&fixture.stream, 1_000_000, 2))
        .await
        .unwrap();
    let key = raw_dataset_key(&fixture).await;
    fixture.catalog.fail_next_commit();

    assert!(fixture.worker().flush_one(&key).await.is_err());
    let visible = fixture.buffer.snapshot_dataset(&key).await.unwrap();
    assert_eq!(
        visible
            .iter()
            .map(|batch| batch.batch.num_rows())
            .sum::<usize>(),
        2
    );
    assert_eq!(fixture.catalog.committed_rows(), Vec::<u64>::new());
    assert_eq!(
        objects(&fixture.store, "artifacts/").await,
        1,
        "uploaded object is an intentional orphan after catalog failure"
    );
    assert_eq!(
        objects(&fixture.store, "wal/").await,
        1,
        "the WAL object stays until a flush covering it commits"
    );

    fixture
        .worker()
        .write(batch(&fixture.stream, 2_000_000, 1))
        .await
        .unwrap();
    fixture.worker().flush_one(&key).await.unwrap();
    assert_eq!(fixture.catalog.committed_rows(), vec![2]);
    assert!(
        fixture.buffer.reserved_bytes() > 0,
        "new active row remains buffered"
    );

    fixture.worker().flush_one(&key).await.unwrap();
    assert_eq!(fixture.catalog.committed_rows(), vec![2, 1]);
    assert_eq!(fixture.buffer.reserved_bytes(), 0);
}

#[tokio::test]
async fn due_flush_waits_for_age_threshold() {
    let fixture = fixture(IntakeSettings {
        buffer_max_size: 1,
        flush_interval_secs: 30,
        ..IntakeSettings::default()
    })
    .await;
    fixture
        .worker()
        .write(batch(&fixture.stream, 1_000_000, 1))
        .await
        .unwrap();
    let key = raw_dataset_key(&fixture).await;
    let now = Instant::now();

    fixture
        .worker()
        .flush_one_if_due_at(&key, now)
        .await
        .unwrap();
    assert!(fixture.catalog.committed_rows().is_empty());

    fixture
        .worker()
        .flush_one_if_due_at(&key, now + Duration::from_secs(31))
        .await
        .unwrap();
    assert_eq!(fixture.catalog.committed_rows(), vec![1]);
}

#[tokio::test]
async fn deleted_stream_discards_the_buffer_and_returns_its_memory() {
    let fixture = fixture(IntakeSettings::default()).await;
    fixture
        .worker()
        .write(batch(&fixture.stream, 1_000_000, 3))
        .await
        .unwrap();
    let key = raw_dataset_key(&fixture).await;
    assert!(fixture.buffer.reserved_bytes() > 0);

    fixture.stream_gone.store(true, Ordering::SeqCst);
    fixture.worker().flush_one(&key).await.unwrap();

    assert_eq!(
        fixture.buffer.reserved_bytes(),
        0,
        "budget must be returned"
    );
    assert!(fixture.catalog.committed_rows().is_empty());
    assert!(
        fixture
            .buffer
            .snapshot_dataset(&key)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        objects(&fixture.store, "wal/").await,
        0,
        "discarded rows must not stay in the WAL"
    );
    // 之后的 flush 是安静的空操作，不会反复报错。
    fixture.worker().flush_one(&key).await.unwrap();
}

#[tokio::test]
async fn dataset_missing_from_the_catalog_discards_the_buffer() {
    let fixture = fixture(IntakeSettings::default()).await;
    fixture
        .worker()
        .write(batch(&fixture.stream, 1_000_000, 2))
        .await
        .unwrap();
    let key = raw_dataset_key(&fixture).await;

    fixture.catalog.forget_datasets();
    fixture.worker().flush_one(&key).await.unwrap();

    assert_eq!(fixture.buffer.reserved_bytes(), 0);
    assert!(fixture.catalog.committed_rows().is_empty());
    assert!(
        fixture
            .buffer
            .snapshot_dataset(&key)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_fenced_writer_drops_its_rows_keeps_their_wal_and_writes_on_under_a_new_epoch() {
    let fixture = fixture(IntakeSettings::default()).await;
    fixture
        .worker()
        .write(batch(&fixture.stream, 1_000_000, 2))
        .await
        .unwrap();
    let key = raw_dataset_key(&fixture).await;
    let fenced = fixture.worker().wal().lease().epoch();
    // Another node fenced this writer: its lease had lapsed (a long pause, a partition).
    fixture.registry.expire(fenced);
    fixture.registry.fence_expired().await.unwrap();

    let error = fixture.worker().flush_one(&key).await.unwrap_err();

    assert!(is_stale_writer_epoch(&error), "{error}");
    assert_eq!(
        fixture.buffer.reserved_bytes(),
        0,
        "budget must be returned"
    );
    assert!(
        fixture
            .buffer
            .snapshot_dataset(&key)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(fixture.catalog.committed_rows().is_empty());
    assert_eq!(
        objects(&fixture.store, "wal/").await,
        1,
        "fenced rows stay in the WAL for the node that claims it"
    );

    // The next renewal finds the fence and takes a new epoch; writes go on under it.
    fixture.worker().wal().renew().await;
    assert!(fixture.worker().wal().lease().epoch() > fenced);
    fixture
        .worker()
        .write(batch(&fixture.stream, 2_000_000, 1))
        .await
        .unwrap();
    fixture.worker().flush_one(&key).await.unwrap();
    assert_eq!(fixture.catalog.committed_rows(), vec![1]);
}

/// 一个永远发布不出去的数据集不能耗尽共享的内存预算，否则所有数据集的写入都会被限流。
#[tokio::test]
async fn discarding_a_stuck_dataset_lets_new_writes_through_again() {
    let fixture = fixture_with_budget(IntakeSettings::default(), 4096).await;
    let key = {
        fixture
            .worker()
            .write(batch(&fixture.stream, 1_000_000, 1))
            .await
            .unwrap();
        raw_dataset_key(&fixture).await
    };
    let mut accepted = 1;
    loop {
        match fixture
            .worker()
            .write(batch(&fixture.stream, 1_000_000 + accepted, 1))
            .await
        {
            Ok(_) => accepted += 1,
            Err(Error::ResourceExhausted(_)) => break,
            Err(other) => panic!("unexpected write error: {other}"),
        }
    }
    assert!(
        accepted > 1,
        "budget must have admitted several batches first"
    );

    // 流被删除，buffer 里的数据永远发布不出去。
    fixture.stream_gone.store(true, Ordering::SeqCst);
    fixture.worker().flush_one(&key).await.unwrap();
    assert_eq!(fixture.buffer.reserved_bytes(), 0);

    // 流重建后，新的写入重新被接受。
    fixture.stream_gone.store(false, Ordering::SeqCst);
    fixture
        .worker()
        .write(batch(&fixture.stream, 9_000_000, 1))
        .await
        .expect("budget was returned");
}
