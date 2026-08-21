// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! IntakeWorker 端到端：WAL append → buffer push → 显式 flush_one →
//! FileCatalog Segment/Artifact + parquet 文件存在 + WAL 段被截断；再重启 worker 验 replay。
//!
//! 用 `MS_RUN_IT=1` 守护：默认本地无 docker 跳过。

#![allow(clippy::field_reassign_with_default, dead_code)]

mod common;

use std::{path::PathBuf, sync::Arc};

use common::skip_unless_enabled;
use molesignal::{
    bootstrap::roles::intake::IntakeWorker,
    config::IntakeSettings,
    domain::{
        iam::{Organization, OrganizationRepository},
        intake::{IntakeBatch, IntakeSink, RawEvent},
        storage::{
            DatasetSelection, FileCatalog, OrganizationScope, builtin_registry,
            primary_dataset_type,
        },
        stream::{
            FieldDef, FieldType, Retention, Schema, StreamDefinition, StreamRepository, StreamType,
        },
    },
    infra::{
        intake::{BufferPool, DatasetResolver, WalPool},
        persistence::{
            MetaStore,
            repositories::{
                file_catalog::PgFileCatalog, organizations::PgOrganizationRepository,
                streams::PgStreamRepository,
            },
        },
        segment_wal::FsyncPolicy,
        storage::{object, parquet::writer::ParquetWriter},
    },
    shared::{
        health::Probe,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};
use serde_json::json;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres as PgImage;

struct Fixture {
    worker: Arc<IntakeWorker>,
    streams: Arc<PgStreamRepository>,
    catalog: Arc<PgFileCatalog>,
    resolver: Arc<DatasetResolver>,
    wal_pool: Arc<WalPool>,
    buffer: Arc<BufferPool>,
    object_root: tempfile::TempDir,
    wal_root: tempfile::TempDir,
    stream: StreamDefinition,
    _pg: testcontainers::ContainerAsync<PgImage>,
}

async fn fixture() -> Fixture {
    let pg = PgImage::default().start().await.expect("pg start");
    let port = pg.get_host_port_ipv4(5432).await.unwrap();
    let host = pg.get_host().await.unwrap();
    let dsn = format!("postgres://postgres:postgres@{host}:{port}/postgres");

    let object_root = tempfile::tempdir().unwrap();
    let wal_root = tempfile::tempdir().unwrap();

    let meta_cfg = molesignal::config::MetaStoreSettings {
        backend: "postgres".into(),
        dsn,
        min_connections: 1,
        max_connections: 4,
    };
    let meta = MetaStore::connect(&meta_cfg).await.unwrap();
    let pool = meta.pool.clone();
    let streams = Arc::new(PgStreamRepository::new(pool.clone()));
    let catalog = Arc::new(PgFileCatalog::new(pool.clone()));

    // 建 org + streams 行
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
                index_type: None,
                indexed: true,
                encrypted: false,
                exact: false,
            }],
        },
        retention: Some(Retention { days: 7 }),
        created_at: TimestampMicros::now(),
        updated_at: TimestampMicros::now(),
    };
    streams.create(stream.clone()).await.unwrap();

    let object_cfg = molesignal::config::ObjectStoreSettings {
        backend: "local".into(),
        root: object_root.path().to_string_lossy().into(),
        ..Default::default()
    };
    let store = object::build(&object_cfg).unwrap();

    let wal_pool = Arc::new(WalPool::new(
        wal_root.path(),
        "node-test",
        4 * 1024, // 4 KiB 小段，触发多次 rotate
        FsyncPolicy::none_default(),
    ));
    let buffer = Arc::new(BufferPool::new());
    let parquet_writer = Arc::new(ParquetWriter::new(store));
    let probe = Arc::new(Probe::new());
    let settings = IntakeSettings::default();

    let resolver = Arc::new(DatasetResolver::new(
        catalog.clone() as Arc<dyn FileCatalog>,
        Arc::new(builtin_registry()),
    ));
    let worker = Arc::new(IntakeWorker::new(
        wal_pool.clone(),
        buffer.clone(),
        streams.clone() as Arc<dyn StreamRepository>,
        resolver.clone(),
        catalog.clone() as Arc<dyn FileCatalog>,
        parquet_writer,
        probe,
        settings,
    ));

    Fixture {
        worker,
        streams,
        catalog,
        resolver,
        wal_pool,
        buffer,
        object_root,
        wal_root,
        stream,
        _pg: pg,
    }
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
async fn it_5000_rows_flush_then_replay() {
    if skip_unless_enabled() {
        return;
    }
    let f = fixture().await;

    // 写 5000 行，按 100 行/batch 切 50 batch（覆盖多次 WAL append + buffer 累积）
    for chunk in 0..50 {
        let b = batch_of(
            &f.stream,
            1_700_000_000_000_000 + chunk as i64 * 1_000_000,
            100,
        );
        f.worker.write(b).await.expect("intake");
    }

    // 显式 flush（避免依赖 background tick）
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
    let key = dataset_id.clone();
    f.worker.flush_one(&key).await.expect("flush_one");

    // 验：Catalog 一次快照返回已发布 Segment + Primary Artifact。
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
    assert_eq!(segments.len(), 1, "expect a single segment from flush");
    assert_eq!(segments[0].row_count, 5000, "all 5000 rows landed");

    // 验：parquet 文件落到 object_store 路径
    let parquet_path: PathBuf = f
        .object_root
        .path()
        .join(segments[0].primary.object.key.as_str());
    assert!(parquet_path.exists(), "parquet missing at {parquet_path:?}");

    // 验：WAL sealed segments 已被截断（活跃段保留）。目录按 dataset id / epoch 组织。
    let listed_dataset_id = {
        let datasets = f
            .catalog
            .list_datasets(
                &OrganizationScope::new(f.stream.org_id.clone()),
                &f.stream.id,
            )
            .await
            .expect("list datasets");
        assert_eq!(datasets.len(), 1, "raw dataset provisioned on first write");
        datasets[0].id.clone()
    };
    assert_eq!(listed_dataset_id, dataset_id);
    let wal_dir = f
        .wal_root
        .path()
        .join("node-test")
        .join(dataset_id.as_str())
        .join("1");
    let segs_now = molesignal::infra::segment_wal::SegmentWal::segment_paths_sorted(&wal_dir)
        .expect("list segs");
    assert!(
        segs_now.len() <= 1,
        "expect only active segment left after truncate, got {} segments",
        segs_now.len()
    );

    // === 第二阶段：drop worker → 写残余 WAL → 新建 worker（新 WalPool 模拟重启）→ replay ===
    drop(f.worker);
    // 直接往 WAL 追写一批（不经过 buffer），模拟"flush 前进程崩溃"
    let extra_batch = batch_of(&f.stream, 1_700_500_000_000_000, 200);
    let payload = serde_json::to_vec(&extra_batch).unwrap();
    let resolved = f
        .resolver
        .resolve(
            &f.stream,
            primary_dataset_type(f.stream.stream_type).unwrap(),
        )
        .await
        .expect("resolve dataset");
    f.wal_pool
        .append(&resolved.wal_identity(), payload)
        .await
        .unwrap();

    // 新 worker：全新 BufferPool + 全新 WalPool（重启后 pools 状态清零，恢复走目录扫描）
    let buffer2 = Arc::new(BufferPool::new());
    let wal_pool2 = Arc::new(WalPool::new(
        f.wal_root.path(),
        "node-test",
        4 * 1024,
        FsyncPolicy::none_default(),
    ));
    let object_cfg = molesignal::config::ObjectStoreSettings {
        backend: "local".into(),
        root: f.object_root.path().to_string_lossy().into(),
        ..Default::default()
    };
    let store2 = object::build(&object_cfg).unwrap();
    let parquet_writer2 = Arc::new(ParquetWriter::new(store2));
    let worker2 = Arc::new(IntakeWorker::new(
        wal_pool2,
        buffer2.clone(),
        f.streams.clone() as Arc<dyn StreamRepository>,
        f.resolver.clone(),
        f.catalog.clone() as Arc<dyn FileCatalog>,
        parquet_writer2,
        Arc::new(Probe::new()),
        IntakeSettings::default(),
    ));
    worker2.recover_and_replay().await.expect("replay");

    // replay 成功落盘后旧 epoch 目录被整目录清理。
    assert!(
        !wal_dir.exists(),
        "old epoch dir must be purged after successful replay flush"
    );

    // replay 已强制发布；再次 flush_one 应为 no-op。
    worker2
        .flush_one(&key)
        .await
        .expect("flush_one after replay");

    let snapshot2 = f
        .catalog
        .snapshot(
            &OrganizationScope::new(f.stream.org_id.clone()),
            DatasetSelection {
                dataset_ids: vec![dataset_id],
                time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX)),
                partition_shard: None,
            },
        )
        .await
        .unwrap();
    let segments2 = &snapshot2.datasets[0].segments;
    assert_eq!(segments2.len(), 2, "expect 2 segments after replay flush");
    let total_rows: u64 = segments2.iter().map(|segment| segment.row_count).sum();
    assert_eq!(total_rows, 5000 + 200, "total rows after replay");
}
