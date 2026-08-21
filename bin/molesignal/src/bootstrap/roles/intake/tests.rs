// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;
use object_store::{ObjectStore, memory::InMemory};
use serde_json::json;
use tempfile::tempdir;

use super::*;
use crate::{
    domain::{
        intake::RawEvent,
        storage::{FileCatalog, primary_dataset_type},
        stream::{FieldDef, FieldType, Retention, Schema, StreamDefinition, StreamType},
    },
    infra::{
        intake::dataset_resolver::test_support::{StubFileCatalog, test_catalog_and_resolver},
        segment_wal::{FsyncPolicy, SegmentWal},
    },
    shared::{Error, ids::Id, time::TimestampMicros},
};

struct OneStreamRepository(StreamDefinition);

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
        if self.0.org_id == *organization_id
            && self.0.name == name
            && self.0.stream_type == stream_type
        {
            Ok(self.0.clone())
        } else {
            Err(Error::not_found(format!("stream {name}")))
        }
    }

    async fn list(&self, organization_id: &Id) -> Result<Vec<StreamDefinition>> {
        Ok((self.0.org_id == *organization_id)
            .then(|| self.0.clone())
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
                index_type: None,
                indexed: false,
                encrypted: false,
                exact: false,
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
    worker: Arc<IntakeWorker>,
    catalog: Arc<StubFileCatalog>,
    resolver: Arc<DatasetResolver>,
    buffer: Arc<BufferPool>,
    store: Arc<InMemory>,
    stream: StreamDefinition,
    _wal_dir: tempfile::TempDir,
}

fn fixture(settings: IntakeSettings) -> WorkerFixture {
    let wal_dir = tempdir().unwrap();
    let stream = stream_definition();
    let wal = Arc::new(WalPool::new(
        wal_dir.path(),
        "node-test",
        64 * 1024,
        FsyncPolicy::none_default(),
    ));
    let buffer = Arc::new(BufferPool::new());
    let (catalog, resolver) = test_catalog_and_resolver();
    let store = Arc::new(InMemory::new());
    let worker = Arc::new(IntakeWorker::new(
        wal,
        buffer.clone(),
        Arc::new(OneStreamRepository(stream.clone())),
        resolver.clone(),
        catalog.clone() as Arc<dyn FileCatalog>,
        Arc::new(ParquetWriter::new(store.clone())),
        Arc::new(Probe::new()),
        settings,
    ));
    WorkerFixture {
        worker,
        catalog,
        resolver,
        buffer,
        store,
        stream,
        _wal_dir: wal_dir,
    }
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
    let fixture = fixture(IntakeSettings::default());
    fixture
        .worker
        .write(batch(&fixture.stream, 1_000_000, 2))
        .await
        .unwrap();
    let key = raw_dataset_key(&fixture).await;
    assert!(fixture.buffer.reserved_bytes() > 0);

    fixture.worker.flush_one(&key).await.unwrap();

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
    let segments = fixture.store.list(None).collect::<Vec<_>>().await;
    assert_eq!(segments.len(), 1);
    let wal_segments = SegmentWal::segment_paths_sorted(
        fixture
            .worker
            .wal
            .root()
            .join(fixture.worker.wal.node_id())
            .join(key.as_str())
            .join("1"),
    )
    .unwrap();
    assert_eq!(
        wal_segments.len(),
        1,
        "only the new empty active WAL segment remains"
    );
}

#[tokio::test]
async fn catalog_failure_keeps_generation_visible_and_retries_before_new_rows() {
    let fixture = fixture(IntakeSettings::default());
    fixture
        .worker
        .write(batch(&fixture.stream, 1_000_000, 2))
        .await
        .unwrap();
    let key = raw_dataset_key(&fixture).await;
    fixture.catalog.fail_next_commit();

    assert!(fixture.worker.flush_one(&key).await.is_err());
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
        fixture.store.list(None).collect::<Vec<_>>().await.len(),
        1,
        "uploaded object is an intentional orphan after catalog failure"
    );

    fixture
        .worker
        .write(batch(&fixture.stream, 2_000_000, 1))
        .await
        .unwrap();
    fixture.worker.flush_one(&key).await.unwrap();
    assert_eq!(fixture.catalog.committed_rows(), vec![2]);
    assert!(
        fixture.buffer.reserved_bytes() > 0,
        "new active row remains buffered"
    );

    fixture.worker.flush_one(&key).await.unwrap();
    assert_eq!(fixture.catalog.committed_rows(), vec![2, 1]);
    assert_eq!(fixture.buffer.reserved_bytes(), 0);
}

#[tokio::test]
async fn due_flush_waits_for_age_threshold() {
    let fixture = fixture(IntakeSettings {
        buffer_max_mb: 1,
        flush_interval_secs: 30,
        ..IntakeSettings::default()
    });
    fixture
        .worker
        .write(batch(&fixture.stream, 1_000_000, 1))
        .await
        .unwrap();
    let key = raw_dataset_key(&fixture).await;
    let now = Instant::now();

    fixture.worker.flush_one_if_due_at(&key, now).await.unwrap();
    assert!(fixture.catalog.committed_rows().is_empty());

    fixture
        .worker
        .flush_one_if_due_at(&key, now + Duration::from_secs(31))
        .await
        .unwrap();
    assert_eq!(fixture.catalog.committed_rows(), vec![1]);
}
