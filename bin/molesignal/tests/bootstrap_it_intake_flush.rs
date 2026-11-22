// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! IntakeWorker end to end on a real Postgres: a write is acknowledged once its WAL object is in
//! object storage → flush publishes a segment and deletes the objects → a node dies after
//! acknowledging and before flushing, with nothing of it left → another node takes its WAL over
//! and publishes every acknowledged row exactly once, and the dead node, should it come back,
//! is refused.
//!
//! Guarded by `MS_RUN_IT=1` (a container, or `MS_IT_POSTGRES_DSN`).

#![allow(clippy::field_reassign_with_default, dead_code)]

mod common;

use std::{path::PathBuf, sync::Arc};

use common::skip_unless_enabled;
use domain::{
    cluster::PeerRole,
    iam::{Organization, OrganizationRepository},
    intake::{IntakeBatch, IntakeSink, RawEvent},
    storage::{
        DatasetSelection, FileCatalog, OrganizationScope, PhysicalDatasetId, builtin_registry,
        is_stale_writer_epoch, primary_dataset_type,
    },
    stream::{
        FieldDef, FieldType, Retention, Schema, StreamDefinition, StreamIndexType,
        StreamRepository, StreamType,
    },
};
use futures::TryStreamExt;
use kernel::{
    health::Probe,
    ids::Id,
    time::{TimeRange, TimestampMicros},
};
use molesignal::{
    app::cluster::WorkOwnership,
    bootstrap::roles::intake::IntakeWorker,
    infra::{
        intake::{BufferPool, DatasetResolver, ObjectWal, ObjectWalConfig},
        storage::{object, parquet::writer::ParquetWriter},
    },
};
use object_store::{ObjectStore, path::Path};
use postgres::{
    MetaStore,
    repositories::{
        file_catalog::PgFileCatalog, organizations::PgOrganizationRepository,
        streams::PgStreamRepository, wal_writers::PgWalWriterRegistry,
    },
};
use serde_json::json;
use settings::{IntakeSettings, WalSettings};
use testcontainers_modules::postgres::Postgres as PgImage;

struct Fixture {
    pool: sqlx::PgPool,
    streams: Arc<PgStreamRepository>,
    catalog: Arc<PgFileCatalog>,
    resolver: Arc<DatasetResolver>,
    store: Arc<dyn ObjectStore>,
    object_root: tempfile::TempDir,
    stream: StreamDefinition,
    _pg: Option<testcontainers::ContainerAsync<PgImage>>,
}

async fn fixture() -> Fixture {
    let (dsn, pg) = common::test_database().await;
    let object_root = tempfile::tempdir().unwrap();

    let meta_cfg = settings::MetaDatabaseSettings {
        dsn,
        min_connections: 1,
        max_connections: 4,
    };
    let meta = MetaStore::connect(&meta_cfg).await.unwrap();
    let pool = meta.pool.clone();
    let streams = Arc::new(PgStreamRepository::new(pool.clone()));
    let catalog = Arc::new(PgFileCatalog::new(pool.clone()));

    PgOrganizationRepository::new(pool.clone())
        .create(Organization {
            id: Id::from_string("orga"),
            name: "Org A".into(),
            slug: "orga".into(),
            system: false,
            disabled: false,
            created_at: TimestampMicros::now(),
        })
        .await
        .unwrap();

    let stream = StreamDefinition {
        id: Id::new(),
        org_id: Id::from_string("orga"),
        name: "app".into(),
        stream_type: StreamType::LOGS,
        schema: Schema {
            fields: vec![FieldDef {
                name: "level".into(),
                data_type: FieldType::Utf8,
                nullable: false,
                index_type: StreamIndexType::FullText,
                encrypted: false,
            }],
        },
        retention: Some(Retention { days: 7 }),
        created_at: TimestampMicros::now(),
        updated_at: TimestampMicros::now(),
    };
    streams.create(stream.clone()).await.unwrap();

    let object_cfg = settings::ObjectStoreSettings {
        backend: "local".into(),
        root: object_root.path().to_string_lossy().into(),
        ..Default::default()
    };
    let store = object::build(
        &object_cfg,
        &object::credentials::ResolvedCredentials::empty(),
    )
    .unwrap();

    let resolver = Arc::new(DatasetResolver::new(
        catalog.clone() as Arc<dyn FileCatalog>,
        Arc::new(builtin_registry()),
    ));

    Fixture {
        pool,
        streams,
        catalog,
        resolver,
        store,
        object_root,
        stream,
        _pg: pg,
    }
}

/// A node: its own WAL writer (a new epoch) and memory buffers over the shared Postgres and
/// object store. Nothing of it is on a disk.
async fn node(f: &Fixture, node_id: &str) -> Arc<IntakeWorker> {
    let wal = ObjectWal::start(
        f.store.clone(),
        Arc::new(PgWalWriterRegistry::new(f.pool.clone())),
        node_id,
        ObjectWalConfig::from_settings(&WalSettings::default()),
        None,
    )
    .await
    .expect("register WAL writer");

    Arc::new(IntakeWorker::new(
        Arc::new(wal),
        Arc::new(BufferPool::new()),
        f.streams.clone() as Arc<dyn StreamRepository>,
        f.resolver.clone(),
        f.catalog.clone() as Arc<dyn FileCatalog>,
        Arc::new(ParquetWriter::new(f.store.clone())),
        Arc::new(Probe::new()),
        IntakeSettings::default(),
    ))
}

async fn wal_objects(f: &Fixture) -> usize {
    f.store
        .list(Some(&Path::from("wal/v1")))
        .try_collect::<Vec<_>>()
        .await
        .unwrap()
        .len()
}

async fn published_rows(f: &Fixture, dataset_id: &PhysicalDatasetId) -> (usize, u64) {
    let snapshot = f
        .catalog
        .snapshot(
            &OrganizationScope::new(f.stream.org_id.clone()),
            DatasetSelection {
                dataset_ids: vec![dataset_id.clone()],
                time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX)),
                partition_shard: None,
            },
        )
        .await
        .unwrap();

    let segments = &snapshot.datasets[0].segments;
    (
        segments.len(),
        segments.iter().map(|segment| segment.row_count).sum(),
    )
}

/// What the takeover node does every few seconds; the lapsed lease is made to lapse at once.
async fn expire_lease(f: &Fixture, worker: &IntakeWorker) {
    sqlx::query(
        "UPDATE wal_writers SET lease_expires_at = now() - interval '1 second'
          WHERE writer_epoch = $1",
    )
    .bind(worker.wal().lease().epoch().0 as i64)
    .execute(&f.pool)
    .await
    .unwrap();
}

fn batch_of(stream: &StreamDefinition, base_ts: i64, n: usize) -> IntakeBatch {
    let events: Vec<RawEvent> = (0..n)
        .map(|i| {
            let mut f = serde_json::Map::new();
            f.insert("level".into(), json!("info"));
            RawEvent {
                timestamp: TimestampMicros(base_ts + i as i64 * 1000),
                fields: f,
            }
        })
        .collect();
    IntakeBatch {
        batch_id: Id::new(),
        org_id: stream.org_id.clone(),
        stream: stream.name.clone(),
        stream_type: stream.stream_type,
        events,
        received_at: TimestampMicros::now(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn acknowledged_rows_survive_losing_the_node_and_are_published_once() {
    if skip_unless_enabled() {
        return;
    }
    let f = fixture().await;
    let a = node(&f, "node-a").await;

    // 5000 rows in 50 acknowledged batches, then a flush.
    for chunk in 0..50 {
        let b = batch_of(
            &f.stream,
            1_700_000_000_000_000 + chunk as i64 * 1_000_000,
            100,
        );
        a.write(b).await.expect("intake");
    }

    assert!(
        wal_objects(&f).await > 0,
        "acknowledged batches are in object storage"
    );

    let dataset_id = f
        .resolver
        .resolve(
            &f.stream,
            primary_dataset_type(f.stream.stream_type).unwrap(),
        )
        .await
        .expect("resolve raw dataset")
        .dataset
        .id
        .clone();
    a.flush_one(&dataset_id).await.expect("flush_one");

    assert_eq!(published_rows(&f, &dataset_id).await, (1, 5000));
    assert_eq!(
        wal_objects(&f).await,
        0,
        "a committed flush deletes its WAL objects"
    );

    let snapshot = f
        .catalog
        .snapshot(
            &OrganizationScope::new(f.stream.org_id.clone()),
            DatasetSelection {
                dataset_ids: vec![dataset_id.clone()],
                time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX)),
                partition_shard: None,
            },
        )
        .await
        .unwrap();
    let parquet_path: PathBuf = f
        .object_root
        .path()
        .join(snapshot.datasets[0].segments[0].primary.object.key.as_str());
    assert!(parquet_path.exists(), "parquet missing at {parquet_path:?}");

    // 200 more rows are acknowledged and never flushed: then the node is gone, disk and all.
    a.write(batch_of(&f.stream, 1_700_500_000_000_000, 200))
        .await
        .expect("acknowledged before the crash");
    assert!(wal_objects(&f).await > 0);
    expire_lease(&f, &a).await;

    // Another node takes the WAL over.
    let b = node(&f, "node-b").await;
    let ownership = WorkOwnership::solo(PeerRole::Intake);
    let round = b.take_over_lapsed(&ownership).await.expect("takeover");
    assert_eq!((round.fenced, round.streams, round.rows), (1, 1, 200));
    assert_eq!(published_rows(&f, &dataset_id).await, (2, 5200));
    assert_eq!(
        wal_objects(&f).await,
        0,
        "a finished takeover deletes the objects"
    );

    let again = b.take_over_lapsed(&ownership).await.expect("second round");
    assert_eq!(
        (again.streams, again.rows),
        (0, 0),
        "nothing is taken over twice"
    );

    // The old node comes back from a pause: its buffered rows are refused, not published twice.
    let refused = a.flush_one(&dataset_id).await.unwrap_err();
    assert!(is_stale_writer_epoch(&refused), "{refused}");
    assert_eq!(published_rows(&f, &dataset_id).await, (2, 5200));

    // Its next renewal finds the writer fenced; it takes a new epoch and goes on writing.
    let fenced_epoch = a.wal().lease().epoch();
    a.wal().renew().await;
    assert!(a.wal().lease().epoch() > fenced_epoch);
    a.write(batch_of(&f.stream, 1_700_600_000_000_000, 1))
        .await
        .expect("write under the new epoch");
    a.flush_one(&dataset_id)
        .await
        .expect("flush under the new epoch");
    assert_eq!(published_rows(&f, &dataset_id).await, (3, 5201));

    let streams_left: Vec<i64> = sqlx::query_scalar("SELECT writer_epoch FROM wal_streams")
        .fetch_all(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        streams_left,
        vec![a.wal().lease().epoch().0 as i64],
        "only the live epoch's stream is open"
    );
}
