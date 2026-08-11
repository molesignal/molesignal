// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Compactor 端到端：testcontainer postgres + 真 PgParquetFileMetaRepository。
//! 写 5 个小 parquet → sweep_one → 验合并为 1 个 + 旧 object 已删 + replace 事务原子。
//!
//! 用 `MS_RUN_IT=1` 守护：默认本地无 docker 跳过。

#![allow(clippy::field_reassign_with_default, dead_code)]

mod common;

use std::sync::Arc;

use arrow::array::{Int64Array, RecordBatch, TimestampMicrosecondArray};
use common::skip_unless_enabled;
use futures::StreamExt;
use molesignal::{
    config::CompactorSettings,
    domain::{
        storage::{ParquetFileMeta, ParquetFileMetaRepository},
        stream::{
            FieldDef, FieldType, Retention, Schema, StreamDefinition, StreamRepository, StreamType,
        },
    },
    infra::{
        persistence::{
            MetaStore,
            repositories::{
                parquet_file_meta::PgParquetFileMetaRepository, streams::PgStreamRepository,
            },
        },
        storage::{
            arrow_schema::to_arrow,
            compactor::Compactor,
            object,
            parquet::{reader::ParquetReader, writer::ParquetWriter},
        },
    },
    shared::{
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};
use object_store::ObjectStore;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres as PgImage;

fn sample_stream() -> StreamDefinition {
    StreamDefinition {
        id: Id::new(),
        org_id: Id::from_string("orga"),
        name: "app".into(),
        stream_type: StreamType::Logs,
        schema: Schema {
            fields: vec![FieldDef {
                name: "val".into(),
                data_type: FieldType::Int64,
                nullable: false,
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

fn small_batch(start_us: i64, n: usize) -> RecordBatch {
    let schema = to_arrow(&sample_stream().schema);
    let ts =
        TimestampMicrosecondArray::from((0..n).map(|i| start_us + i as i64).collect::<Vec<_>>())
            .with_timezone("UTC");
    let val = Int64Array::from((0..n).map(|i| i as i64).collect::<Vec<_>>());
    RecordBatch::try_new(schema, vec![Arc::new(ts), Arc::new(val)]).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_compactor_merges_5_files_into_1_via_pg() {
    if skip_unless_enabled() {
        return;
    }
    let pg = PgImage::default().start().await.expect("pg start");
    let port = pg.get_host_port_ipv4(5432).await.unwrap();
    let host = pg.get_host().await.unwrap();
    let dsn = format!("postgres://postgres:postgres@{host}:{port}/postgres");

    let meta_cfg = molesignal::config::MetaStoreSettings {
        backend: "postgres".into(),
        dsn,
        min_connections: 1,
        max_connections: 4,
    };
    let meta = MetaStore::connect(&meta_cfg).await.unwrap();
    let pool = meta.pool.clone();
    let streams_repo = Arc::new(PgStreamRepository::new(pool.clone()));
    let parquet_file_meta = Arc::new(PgParquetFileMetaRepository::new(pool.clone()));

    let stream = sample_stream();
    streams_repo.create(stream.clone()).await.unwrap();

    let object_root = tempfile::tempdir().unwrap();
    let object_cfg = molesignal::config::ObjectStoreSettings {
        backend: "local".into(),
        root: object_root.path().to_string_lossy().into(),
        ..Default::default()
    };
    let store = object::build(&object_cfg).unwrap();
    let writer = Arc::new(ParquetWriter::new(store.clone()));
    let reader = Arc::new(ParquetReader::new(store.clone()));

    // 写 5 个小文件
    let mut metas: Vec<ParquetFileMeta> = Vec::new();
    for i in 0..5 {
        let batch = small_batch(1_000_000 + i * 1_000, 20);
        let mut m = writer.flush(&stream, batch).await.unwrap();
        m.size_bytes = 1024; // 强制小于 target_mb
        parquet_file_meta.insert(m.clone()).await.unwrap();
        metas.push(m);
    }
    // 验：5 个 active
    let before = parquet_file_meta
        .find(
            &stream.org_id,
            &stream.name,
            stream.stream_type,
            TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX)),
        )
        .await
        .unwrap();
    assert_eq!(before.len(), 5);

    // 跑 sweep
    let compactor = Compactor::new(
        parquet_file_meta.clone() as Arc<dyn ParquetFileMetaRepository>,
        reader,
        writer,
        store.clone(),
        CompactorSettings::default(),
    );
    let n = compactor
        .sweep_one(
            &stream,
            TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX)),
        )
        .await
        .unwrap();
    assert_eq!(n, 1, "expect 1 merged group");

    // 验：1 个 active（5 旧 marked deleted）
    let after = parquet_file_meta
        .find(
            &stream.org_id,
            &stream.name,
            stream.stream_type,
            TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX)),
        )
        .await
        .unwrap();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].rows, 100, "all 5×20 rows in merged");

    // 验：object_store 上 .parquet 对象数 == 1（旧 5 已删除 + 新 1）
    let mut stream_list = store.list(None);
    let mut count = 0usize;
    while let Some(item) = stream_list.next().await {
        let obj = item.unwrap();
        if obj.location.as_ref().ends_with(".parquet") {
            count += 1;
        }
    }
    assert_eq!(count, 1, "5 old objects deleted, 1 new survives");
}
