// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Compactor 端到端：testcontainer postgres + 真 PgFileCatalog。
//! 写 5 个小 Segment → sweep_one → 验 Catalog 原子替换为 1 个，旧 Artifact 仅进入
//! 延迟 GC 队列且对象仍可供旧快照读取。
//!
//! 用 `MS_RUN_IT=1` 守护：默认本地无 docker 跳过。

#![allow(clippy::field_reassign_with_default, dead_code)]

mod common;

use std::sync::Arc;

use arrow::array::{Int64Array, RecordBatch, TimestampMicrosecondArray};
use common::skip_unless_enabled;
use futures::StreamExt;
use molesignal::{
    config::{CompactorSettings, ObjectCacheSettings},
    domain::{
        iam::{Organization, OrganizationRepository},
        storage::{
            CommitFlush, DatasetSelection, FileCatalog, FlushProvenance, OrganizationScope,
            SequenceRange, WalSequence, WriterEpoch, WriterNodeId, builtin_registry,
        },
        stream::{
            FieldDef, FieldType, Retention, Schema, StreamDefinition, StreamRepository, StreamType,
        },
    },
    infra::{
        persistence::{
            MetaStore,
            repositories::{
                file_catalog::PgFileCatalog, organizations::PgOrganizationRepository,
                streams::PgStreamRepository,
            },
        },
        storage::{
            arrow_schema::to_arrow, compactor::Compactor, manifest::PartitionManifestReader,
            object, object_reader::ObjectReader, parquet::writer::ParquetWriter,
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
        stream_type: StreamType::LOGS,
        schema: Schema {
            fields: vec![FieldDef {
                name: "val".into(),
                data_type: FieldType::Int64,
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

    let stream = sample_stream();
    PgOrganizationRepository::new(pool.clone())
        .create(Organization {
            id: stream.org_id.clone(),
            name: "Compactor".into(),
            slug: "compactor".into(),
            system: false,
            disabled: false,
            created_at: TimestampMicros::now(),
        })
        .await
        .unwrap();
    streams_repo.create(stream.clone()).await.unwrap();
    let catalog = Arc::new(PgFileCatalog::new(pool.clone()));
    let scope = OrganizationScope::new(stream.org_id.clone());
    let specs = builtin_registry()
        .eager_dataset_specs(&stream.stream_type)
        .unwrap();
    let dataset = catalog
        .ensure_datasets(&scope, &stream.id, &specs)
        .await
        .unwrap()
        .remove(0);

    let object_root = tempfile::tempdir().unwrap();
    let object_cfg = molesignal::config::ObjectStoreSettings {
        backend: "local".into(),
        root: object_root.path().to_string_lossy().into(),
        ..Default::default()
    };
    let store = object::build(&object_cfg).unwrap();
    let writer = Arc::new(ParquetWriter::new(store.clone()));
    let object_reader =
        ObjectReader::build(store.clone(), "local", &ObjectCacheSettings::default()).unwrap();

    // 写 5 个小 Segment，并逐个通过 Catalog 原子发布。
    for i in 0..5 {
        let batch = small_batch(1_000_000 + i * 1_000, 20);
        let start = i as u64 * 20 + 1;
        let provenance = FlushProvenance::derive(
            WriterNodeId::new("intake-a"),
            WriterEpoch(1),
            SequenceRange::new(WalSequence(start), WalSequence(start + 19)),
        );
        let segments = writer
            .flush_catalog(&stream, &dataset, &provenance, batch)
            .await
            .unwrap();
        catalog
            .commit_flush(
                &scope,
                CommitFlush {
                    dataset_id: dataset.id.clone(),
                    provenance,
                    segments,
                },
            )
            .await
            .unwrap();
    }
    let selection = || DatasetSelection {
        dataset_ids: vec![dataset.id.clone()],
        time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX)),
        partition_shard: None,
    };
    let before = catalog.snapshot(&scope, selection()).await.unwrap();
    assert_eq!(before.datasets[0].segments.len(), 5);

    // 跑 sweep
    let compactor = Compactor::new(
        catalog.clone() as Arc<dyn FileCatalog>,
        object_reader.clone(),
        Arc::new(PartitionManifestReader::new(object_reader, 8 * 1024 * 1024)),
        writer,
        store.clone(),
        CompactorSettings::default(),
        3600,
    );
    let n = compactor
        .sweep_one(
            &stream,
            TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX)),
        )
        .await
        .unwrap();
    assert_eq!(n, 1, "expect 1 merged group");

    // 只有 replacement 对新快照可见，旧对象尚未物理删除。
    let after = catalog.snapshot(&scope, selection()).await.unwrap();
    assert_eq!(after.datasets[0].segments.len(), 1);
    assert_eq!(
        after.datasets[0].segments[0].row_count, 100,
        "all 5×20 rows in merged"
    );

    // 5 个被替换对象 + 1 个 replacement 都仍在 store；延迟 GC 由独立 worker 执行。
    let mut stream_list = store.list(None);
    let mut count = 0usize;
    while let Some(item) = stream_list.next().await {
        item.unwrap();
        count += 1;
    }
    assert_eq!(count, 6);
    let queued: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM object_gc_queue WHERE org_id = $1 AND reason = 'replaced'",
    )
    .bind(stream.org_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(queued, 5);
}
