// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `parquet_file_meta_dump` Postgres + object_store happy-path 冒烟。
//!
//! 覆盖 change `parquet-file-meta-dump-columnar`：
//! - PgParquetFileMetaDumpRepository 主路径（upsert / find_by_time_range / mark_deleted / insert_rewrite）
//! - ParquetFileMetaDumpService advisory lock 行为：并发 dump 同一 partition 第二个返 skip-locked
//! - ParquetFileMetaDumpService::delete_by_time_range 三种 case（无 overlap / 整删 / 部分重写）
//!
//! 默认会跳过：testcontainers 需要可用的 docker daemon，设置 `MS_RUN_IT=1` 才真正跑。
//!
//! ```bash
//! MS_RUN_IT=1 cargo test -p molesignal-infra --test it_parquet_file_meta_dump -- --nocapture
//! ```

use std::sync::Arc;

use molesignal::{
    config::{MetaStoreSettings, ParquetFileMetaDumpSettings, PartitionLevel as CfgLevel},
    domain::{
        storage::{
            ParquetFileMeta, ParquetFileMetaDumpRepository, ParquetFileMetaDumpRow,
            ParquetFileMetaDumpStats, PartitionLevel, PhysicalDatasetKind,
        },
        stream::StreamType,
    },
    infra::{
        persistence::{
            MetaStore, repositories::parquet_file_meta::dump::PgParquetFileMetaDumpRepository,
        },
        storage::parquet_file_meta_dump::ParquetFileMetaDumpService,
    },
    shared::{
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};
use object_store::{ObjectStore, memory::InMemory};
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres as PgImage;

fn skip_unless_enabled() -> bool {
    std::env::var("MS_RUN_IT").ok().as_deref() != Some("1")
}

async fn boot() -> (MetaStore, Arc<dyn ObjectStore>) {
    let pg = PgImage::default().start().await.expect("start pg");
    let port = pg.get_host_port_ipv4(5432).await.expect("pg port");
    let host = pg.get_host().await.expect("pg host");
    let dsn = format!("postgres://postgres:postgres@{host}:{port}/postgres");
    let store = MetaStore::connect(&MetaStoreSettings {
        backend: "postgres".into(),
        dsn,
        min_connections: 1,
        max_connections: 5,
    })
    .await
    .expect("connect + migrate");
    let obj: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    // Leak the testcontainers handle so it stays alive for the duration of the test;
    // returning it would force a structural change in callers and the smoke tests
    // only need the surface for ~seconds.
    std::mem::forget(pg);
    (store, obj)
}

fn sample_row(seed: u32, partition_key: &str) -> ParquetFileMetaDumpRow {
    let now = TimestampMicros::now();
    let day = chrono::DateTime::<chrono::Utc>::from_timestamp_micros(now.0)
        .unwrap()
        .format("%Y-%m-%d")
        .to_string();
    ParquetFileMetaDumpRow {
        id: Id::new(),
        org_id: Id::from_string("orgA"),
        stream: "app".into(),
        stream_type: StreamType::Logs,
        dataset_kind: PhysicalDatasetKind::Raw,
        date: day,
        object_key: format!(
            "orgA/_parquet_file_meta_dump/logs/raw/app/{partition_key}.r{seed}.parquet"
        ),
        rows_in_dump: 10 + seed,
        created_at: now,
        partition_level: PartitionLevel::Daily,
        partition_key: partition_key.to_string(),
        deleted: false,
        min_ts_micros: now.0 - 10_000,
        max_ts_micros: now.0 - 1_000,
        size_bytes: 4096,
        updated_at_micros: now.0,
    }
}

fn sample_stats_for(row: &ParquetFileMetaDumpRow) -> ParquetFileMetaDumpStats {
    ParquetFileMetaDumpStats {
        object_key: row.object_key.clone(),
        rows_total: i64::from(row.rows_in_dump),
        files_total: i64::from(row.rows_in_dump),
        time_start_micros: row.min_ts_micros,
        time_end_micros: row.max_ts_micros,
        storage_size_bytes: row.size_bytes,
        updated_at_micros: row.updated_at_micros,
    }
}

#[tokio::test]
async fn repo_upsert_find_mark_deleted_roundtrip() {
    if skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }
    let (meta_store, _obj) = boot().await;
    let repo = PgParquetFileMetaDumpRepository::new(meta_store.pool.clone());

    let row = sample_row(1, "2026-01-15");
    let stats = sample_stats_for(&row);
    repo.upsert_dump(row.clone(), stats).await.expect("upsert");

    let listed = repo
        .find_by_time_range(
            &row.org_id,
            &row.stream,
            row.stream_type,
            row.dataset_kind,
            TimeRange::new(
                TimestampMicros(row.min_ts_micros - 1),
                TimestampMicros(row.max_ts_micros + 1),
            ),
        )
        .await
        .expect("find");
    assert_eq!(listed.len(), 1, "live row must be visible");
    assert_eq!(listed[0].object_key, row.object_key);

    repo.mark_deleted(&row.object_key).await.expect("mark");
    let listed = repo
        .find_by_time_range(
            &row.org_id,
            &row.stream,
            row.stream_type,
            row.dataset_kind,
            TimeRange::new(
                TimestampMicros(row.min_ts_micros - 1),
                TimestampMicros(row.max_ts_micros + 1),
            ),
        )
        .await
        .expect("find again");
    assert!(listed.is_empty(), "deleted row must be hidden");

    let pending = repo.pending_object_deletes(10).await.expect("pending");
    assert!(pending.iter().any(|r| r.object_key == row.object_key));
}

#[tokio::test]
async fn repo_insert_rewrite_swaps_live_seat() {
    if skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }
    let (meta_store, _obj) = boot().await;
    let repo = PgParquetFileMetaDumpRepository::new(meta_store.pool.clone());

    let old = sample_row(1, "2026-01-15");
    let old_stats = sample_stats_for(&old);
    repo.upsert_dump(old.clone(), old_stats)
        .await
        .expect("seed");

    let mut new = sample_row(2, "2026-01-15");
    new.id = Id::new();
    new.object_key = "orgA/_parquet_file_meta_dump/logs/raw/app/2026-01-15.r2.parquet".to_string();
    new.rows_in_dump = 4;
    let new_stats = sample_stats_for(&new);
    repo.insert_rewrite(&old.object_key, new.clone(), new_stats)
        .await
        .expect("rewrite");

    let listed = repo
        .find_by_time_range(
            &new.org_id,
            &new.stream,
            new.stream_type,
            new.dataset_kind,
            TimeRange::new(
                TimestampMicros(new.min_ts_micros - 1),
                TimestampMicros(new.max_ts_micros + 1),
            ),
        )
        .await
        .expect("find");
    assert_eq!(listed.len(), 1, "only the new live row");
    assert_eq!(listed[0].object_key, new.object_key);
}

#[tokio::test]
async fn service_dump_one_partition_acquires_lock_and_writes_columnar() {
    if skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }
    let (meta_store, obj) = boot().await;
    // seed one cold parquet_file_meta row.
    let org_id = Id::from_string("orgA");
    let stream_name = "app";
    let stream_type = StreamType::Logs;
    let now_us = TimestampMicros::now().0;
    let cold_us = now_us - 40 * 24 * 3600 * 1_000_000_i64;
    let fm = ParquetFileMeta {
        id: Id::new(),
        org_id: org_id.clone(),
        stream: stream_name.into(),
        stream_type,
        dataset_kind: PhysicalDatasetKind::Raw,
        object_key: format!("orgA/logs/app/2026-01-15/{}.parquet", Id::new().0),
        time_range: TimeRange::new(
            TimestampMicros(cold_us),
            TimestampMicros(cold_us + 30_000_000),
        ),
        rows: 100,
        size_bytes: 1024,
        min_values: serde_json::Map::new(),
        max_values: serde_json::Map::new(),
        deleted: false,
    };
    sqlx::query(
        "INSERT INTO parquet_file_meta
         (id, org_id, stream, stream_type, dataset_kind, object_key,
          time_start_micros, time_end_micros, rows, size_bytes,
          min_values, max_values, deleted)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11::jsonb, $12::jsonb, FALSE)",
    )
    .bind(&fm.id.0)
    .bind(&fm.org_id.0)
    .bind(&fm.stream)
    .bind("logs")
    .bind(fm.dataset_kind.as_str())
    .bind(&fm.object_key)
    .bind(fm.time_range.start.0)
    .bind(fm.time_range.end.0)
    .bind(fm.rows as i64)
    .bind(fm.size_bytes as i64)
    .bind("{}")
    .bind("{}")
    .execute(&meta_store.pool)
    .await
    .expect("seed parquet_file_meta");

    let svc = ParquetFileMetaDumpService::new(
        meta_store.pool.clone(),
        obj.clone(),
        ParquetFileMetaDumpSettings {
            enabled: true,
            cold_after_days: 30,
            interval_secs: 3600,
            max_partitions_per_tick: 10,
            partition_level: CfgLevel::Daily,
        },
    );
    let stats = svc.run_tick().await.expect("run_tick");
    assert_eq!(stats.partitions_processed, 1, "1 partition dumped");
    assert!(stats.rows_dumped >= 1);

    let repo = PgParquetFileMetaDumpRepository::new(meta_store.pool.clone());
    let listed = repo
        .find_by_time_range(
            &org_id,
            stream_name,
            stream_type,
            PhysicalDatasetKind::Raw,
            TimeRange::new(
                TimestampMicros(fm.time_range.start.0 - 1),
                TimestampMicros(fm.time_range.end.0 + 1),
            ),
        )
        .await
        .expect("find dumps");
    assert_eq!(listed.len(), 1, "dump row materialized");
    assert_eq!(listed[0].partition_level, PartitionLevel::Daily);
}

#[tokio::test]
async fn service_delete_by_time_range_drops_full_overlap_dump() {
    if skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }
    let (meta_store, obj) = boot().await;
    // 1) ingest one cold row → dump
    let org_id = Id::from_string("orgA");
    let now_us = TimestampMicros::now().0;
    let cold_us = now_us - 40 * 24 * 3600 * 1_000_000_i64;
    sqlx::query(
        "INSERT INTO parquet_file_meta
         (id, org_id, stream, stream_type, dataset_kind, object_key,
          time_start_micros, time_end_micros, rows, size_bytes,
          min_values, max_values, deleted)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11::jsonb, $12::jsonb, FALSE)",
    )
    .bind(Id::new().0)
    .bind(&org_id.0)
    .bind("app")
    .bind("logs")
    .bind("raw")
    .bind(format!("orgA/logs/app/2026-01-15/{}.parquet", Id::new().0))
    .bind(cold_us)
    .bind(cold_us + 1_000_000)
    .bind(50_i64)
    .bind(2_000_i64)
    .bind("{}")
    .bind("{}")
    .execute(&meta_store.pool)
    .await
    .expect("seed");
    let svc = ParquetFileMetaDumpService::new(
        meta_store.pool.clone(),
        obj.clone(),
        ParquetFileMetaDumpSettings {
            enabled: true,
            cold_after_days: 30,
            interval_secs: 3600,
            max_partitions_per_tick: 10,
            partition_level: CfgLevel::Daily,
        },
    );
    svc.run_tick().await.expect("dump");

    // 2) delete the entire cold range
    let stats = svc
        .delete_by_time_range(
            &org_id,
            "app",
            StreamType::Logs,
            PhysicalDatasetKind::Raw,
            TimeRange::new(
                TimestampMicros(cold_us - 1),
                TimestampMicros(cold_us + 2_000_000),
            ),
        )
        .await
        .expect("delete");
    assert_eq!(
        stats.partitions_dropped, 1,
        "fully-overlapping dump dropped"
    );
    assert_eq!(stats.partitions_rewritten, 0);
    assert!(stats.rows_removed >= 1);

    // 3) repo lookup must hide the live row
    let repo = PgParquetFileMetaDumpRepository::new(meta_store.pool.clone());
    let listed = repo
        .find_by_time_range(
            &org_id,
            "app",
            StreamType::Logs,
            PhysicalDatasetKind::Raw,
            TimeRange::new(
                TimestampMicros(cold_us - 1),
                TimestampMicros(cold_us + 2_000_000),
            ),
        )
        .await
        .expect("find");
    assert!(listed.is_empty(), "no live dump after full drop");
}

#[tokio::test]
async fn service_object_delete_sweep_purges_object_and_row() {
    // change `parquet-file-meta-dump-columnar` task 8.x：sweep 消费 pending_object_deletes，
    // 删 object_store 上的旧 parquet 并硬删行。
    if skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }
    use object_store::{ObjectStoreExt as _, PutPayload, path::Path};

    let (meta_store, obj) = boot().await;
    let repo = PgParquetFileMetaDumpRepository::new(meta_store.pool.clone());

    // dump 行 + 对应对象。
    let row = sample_row(7, "2026-02-20");
    repo.upsert_dump(row.clone(), sample_stats_for(&row))
        .await
        .expect("upsert");
    obj.put(
        &Path::from(row.object_key.clone()),
        PutPayload::from_static(b"dump"),
    )
    .await
    .expect("put object");

    // mark_deleted → 进入 pending。
    repo.mark_deleted(&row.object_key).await.expect("mark");
    assert!(
        repo.pending_object_deletes(10)
            .await
            .unwrap()
            .iter()
            .any(|r| r.object_key == row.object_key),
        "row must be pending before sweep"
    );

    // sweep。
    let svc = ParquetFileMetaDumpService::new(
        meta_store.pool.clone(),
        obj.clone(),
        ParquetFileMetaDumpSettings {
            enabled: true,
            cold_after_days: 30,
            interval_secs: 3600,
            max_partitions_per_tick: 10,
            partition_level: CfgLevel::Daily,
        },
    );
    let purged = svc.run_object_delete_sweep(10).await.expect("sweep");
    assert_eq!(purged, 1, "one object purged");

    // 对象已删 + 行已硬删。
    assert!(
        matches!(
            obj.head(&Path::from(row.object_key.clone())).await,
            Err(object_store::Error::NotFound { .. })
        ),
        "object must be gone after sweep"
    );
    assert!(
        repo.pending_object_deletes(10).await.unwrap().is_empty(),
        "row must be hard-deleted after sweep"
    );
}
