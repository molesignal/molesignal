// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! IngesterWorker 端到端：WAL append → buffer push → 显式 flush_one →
//! parquet_file_meta 行 + parquet 文件存在 + WAL 段被截断；再重启 worker 验启动 replay。
//!
//! 用 `MS_RUN_IT=1` 守护：默认本地无 docker 跳过。

#![allow(clippy::field_reassign_with_default, dead_code)]

mod common;

use std::{path::PathBuf, sync::Arc};

use common::skip_unless_enabled;
use molesignal::{
    bootstrap::roles::ingester::IngesterWorker,
    config::IngesterSettings,
    domain::{
        ingestion::{IngestBatch, IngestSink, RawEvent},
        storage::{ParquetFileMetaRepository, PhysicalDatasetKind},
        stream::{
            FieldDef, FieldType, Retention, Schema, StreamDefinition, StreamRepository, StreamType,
        },
    },
    infra::{
        caching::ParquetFileMetaCache,
        ingester::{BufferPool, WalPool},
        persistence::{
            MetaStore,
            repositories::{
                parquet_file_meta::PgParquetFileMetaRepository, streams::PgStreamRepository,
            },
        },
        segment_wal::{FsyncPolicy, StaticTermSource, TermSource},
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
    worker: Arc<IngesterWorker>,
    parquet_file_meta: Arc<PgParquetFileMetaRepository>,
    streams: Arc<PgStreamRepository>,
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
    let parquet_file_meta = Arc::new(PgParquetFileMetaRepository::new(pool.clone()));

    // 建 streams 行
    let stream = StreamDefinition {
        id: Id::new(),
        org_id: Id::from_string("orga"),
        name: "app".into(),
        stream_type: StreamType::Logs,
        schema: Schema {
            fields: vec![FieldDef {
                name: "level".into(),
                data_type: FieldType::Utf8,
                nullable: false,
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
        4 * 1024, // 4 KiB 小段，触发多次 rotate
        FsyncPolicy::none_default(),
        Arc::new(StaticTermSource(1)) as Arc<dyn TermSource>,
    ));
    let buffer = Arc::new(BufferPool::new());
    let parquet_writer = Arc::new(ParquetWriter::new(store));
    let cache = Arc::new(ParquetFileMetaCache::new(
        molesignal::config::CacheLayerSettings::new(1_000, 60),
    ));
    let probe = Arc::new(Probe::new());
    let settings = IngesterSettings::default();

    let worker = Arc::new(IngesterWorker::new(
        wal_pool.clone(),
        buffer.clone(),
        streams.clone() as Arc<dyn StreamRepository>,
        parquet_file_meta.clone() as Arc<dyn ParquetFileMetaRepository>,
        parquet_writer,
        Some(cache),
        probe,
        settings,
    ));

    Fixture {
        worker,
        parquet_file_meta,
        streams,
        wal_pool,
        buffer,
        object_root,
        wal_root,
        stream,
        _pg: pg,
    }
}

fn batch_of(stream: &StreamDefinition, base_ts: i64, n: usize) -> IngestBatch {
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
    IngestBatch {
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
        f.worker.write(b).await.expect("ingest");
    }

    // 显式 flush（避免依赖 background tick）
    let key = (
        f.stream.org_id.clone(),
        f.stream.stream_type,
        f.stream.name.clone(),
        PhysicalDatasetKind::Raw,
    );
    f.worker.flush_one(&key).await.expect("flush_one");

    // 验：parquet_file_meta 行存在
    let files = f
        .parquet_file_meta
        .find(
            &f.stream.org_id,
            &f.stream.name,
            f.stream.stream_type,
            TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX)),
        )
        .await
        .unwrap();
    assert_eq!(files.len(), 1, "expect a single parquet from flush");
    assert_eq!(files[0].rows, 5000, "all 5000 rows landed");

    // 验：parquet 文件落到 object_store 路径
    let parquet_path: PathBuf = f.object_root.path().join(&files[0].object_key);
    assert!(parquet_path.exists(), "parquet missing at {parquet_path:?}");

    // 验：WAL sealed segments 已被截断（活跃段保留）
    let wal_dir = f.wal_root.path().join("orga").join("logs").join("app");
    let segs_now = molesignal::infra::segment_wal::SegmentWal::segment_paths_sorted(&wal_dir)
        .expect("list segs");
    assert!(
        segs_now.len() <= 1,
        "expect only active segment left after truncate, got {} segments",
        segs_now.len()
    );

    // === 第二阶段：drop worker → 写残余 WAL → 新建 worker → replay ===
    drop(f.worker);
    // 直接往 WAL 追写一批（不经过 buffer），模拟"flush 前进程崩溃"
    let extra_batch = batch_of(&f.stream, 1_700_500_000_000_000, 200);
    let payload = serde_json::to_vec(&extra_batch).unwrap();
    f.wal_pool.append(&key, payload, 9999).await.unwrap();

    // 新 worker：用全新 BufferPool（worker 之间不共享 buffer）
    let buffer2 = Arc::new(BufferPool::new());
    let object_cfg = molesignal::config::ObjectStoreSettings {
        backend: "local".into(),
        root: f.object_root.path().to_string_lossy().into(),
        ..Default::default()
    };
    let store2 = object::build(&object_cfg).unwrap();
    let parquet_writer2 = Arc::new(ParquetWriter::new(store2));
    let worker2 = Arc::new(IngesterWorker::new(
        f.wal_pool.clone(),
        buffer2.clone(),
        f.streams.clone() as Arc<dyn StreamRepository>,
        f.parquet_file_meta.clone() as Arc<dyn ParquetFileMetaRepository>,
        parquet_writer2,
        None,
        Arc::new(Probe::new()),
        IngesterSettings::default(),
    ));
    worker2.recover_and_replay().await.expect("replay");

    // 再次 flush_one：把 replay 进 buffer 的 200 行落 parquet
    worker2
        .flush_one(&key)
        .await
        .expect("flush_one after replay");

    let files2 = f
        .parquet_file_meta
        .find(
            &f.stream.org_id,
            &f.stream.name,
            f.stream.stream_type,
            TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX)),
        )
        .await
        .unwrap();
    assert_eq!(files2.len(), 2, "expect 2 parquets after replay flush");
    let total_rows: u64 = files2.iter().map(|m| m.rows).sum();
    assert_eq!(total_rows, 5000 + 200, "total rows after replay");
}
