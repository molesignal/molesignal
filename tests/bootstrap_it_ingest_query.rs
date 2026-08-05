// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! ingest API + query API 端到端冒烟。
//!
//! 两条路径都接通到 parquet_file_meta，query 时 DataFusion engine 把它们 union 到一个
//! MemTable，跨 parquet schema 演化由 `align_batch_to_schema` 抚平。
//!
//! 1. POST `/api/v1/ingest/logs/:stream`（含 schema 演化的新字段 `latency_ms`）
//!    → IngestService → IngesterWorker WAL → batch_max_delay_ms 内自动 flush
//!    出一个 parquet + 插入 parquet_file_meta。
//! 2. 同时走 `ParquetWriter` 手动写一份只有老 schema (`_timestamp + level`) 的
//!    parquet + `ParquetFileMetaRepository::insert`，验证 query 时能正确合并两批数据。
//! 3. POST `/api/v1/query`：`SELECT COUNT(*)` 两条路径产物之和（3 + 3 = 6）。

mod common;

use std::sync::Arc;

use arrow::array::{RecordBatch, StringArray, TimestampMicrosecondArray};
use common::{TestServer, skip_unless_enabled};
use molesignal::{
    domain::{
        storage::ParquetFileMetaRepository,
        stream::{
            FieldDef, FieldType, Retention, Schema, StreamDefinition, StreamRepository, StreamType,
        },
    },
    infra::{
        persistence::repositories::{
            parquet_file_meta::PgParquetFileMetaRepository, streams::PgStreamRepository,
        },
        storage::{arrow_schema::to_arrow, parquet::writer::ParquetWriter},
    },
    shared::{ids::Id, time::TimestampMicros},
};
use object_store::{ObjectStore, local::LocalFileSystem};
use serde_json::json;

async fn seed_stream(s: &TestServer) -> StreamDefinition {
    // 直接经 sqlx PgPool 建 StreamRepository（避免依赖 AppState 暴露原始 repo）
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect(&s.settings.store.meta.dsn)
        .await
        .expect("test pool");
    let streams = PgStreamRepository::new(pool);
    streams
        .create(StreamDefinition {
            id: Id::new(),
            org_id: s.root_org_id.clone(),
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
            retention: Some(Retention { days: 30 }),
            created_at: TimestampMicros::now(),
            updated_at: TimestampMicros::now(),
        })
        .await
        .expect("create stream")
}

#[tokio::test]
async fn ingest_api_accepts_batch_and_query_api_returns_seeded_rows() {
    if skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }
    let s = TestServer::start().await;

    let stream = seed_stream(&s).await;

    // 1. POST ingest（3 条 + 1 个新字段 `latency_ms` 触发 schema 演化）
    let resp = s
        .client
        .post(format!("{}/api/v1/ingest/logs/app", s.base_url))
        .header(s.auth_header().0, s.auth_header().1)
        .json(&json!([
            { "_timestamp": 1_000_000, "level": "info", "latency_ms": 10 },
            { "_timestamp": 2_000_000, "level": "warn", "latency_ms": 20 },
            { "_timestamp": 3_000_000, "level": "error", "latency_ms": 30 }
        ]))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["accepted"], 3, "all 3 events accepted");
    assert_eq!(body["rejected"], 0);

    // 2a. 等 IngesterWorker 自动 flush 出 parquet_file_meta（wire 默认 batch_max_delay_ms=50，
    //     加上 parquet 写 + PG insert，通常 < 1s；这里给 10s 兜底防 CI 抖动）。
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect(&s.settings.store.meta.dsn)
        .await
        .expect("test pool 2");
    let flushed = {
        let pool = pool.clone();
        common::wait_until_async(10, move || {
            let pool = pool.clone();
            async move {
                let row: (i64,) =
                    sqlx::query_as("SELECT COUNT(*) FROM parquet_file_meta WHERE deleted = FALSE")
                        .fetch_one(&pool)
                        .await
                        .unwrap_or((0,));
                row.0 >= 1
            }
        })
        .await
    };
    assert!(
        flushed,
        "ingester flush did not produce parquet_file_meta within timeout"
    );

    // 2b. 同时手动写一份"老 schema" parquet（只有 _timestamp + level，没有 latency_ms）
    //     + 插 parquet_file_meta，制造 schema 演化场景，验证 DataFusion 端 align_batch_to_schema 抚平。
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(s.object_store_root.path()).unwrap());
    let writer = ParquetWriter::new(store);
    let arrow_schema = to_arrow(&stream.schema);
    let ts =
        TimestampMicrosecondArray::from(vec![1_000_000, 2_000_000, 3_000_000]).with_timezone("UTC");
    let level = StringArray::from(vec!["info", "warn", "error"]);
    let batch =
        RecordBatch::try_new(arrow_schema, vec![Arc::new(ts), Arc::new(level)]).expect("batch");

    let meta = writer.flush(&stream, batch).await.expect("flush parquet");

    PgParquetFileMetaRepository::new(pool)
        .insert(meta)
        .await
        .expect("insert ParquetFileMeta");

    // 3. POST query → 验证 count = 6（ingest path 3 + 手写 parquet 3）
    let resp = s
        .client
        .post(format!("{}/api/v1/query", s.base_url))
        .header(s.auth_header().0, s.auth_header().1)
        .json(&json!({
            "org_id": s.root_org_id.0,
            "language": "sql",
            "statement": "SELECT COUNT(*) AS n FROM app",
            "time_range": { "start": 0, "end": 10_000_000 },
            "stream": { "name": "app", "stream_type": "logs" }
        }))
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let body_text = resp.text().await.unwrap();
    assert_eq!(status, 200, "query status; body={body_text}");
    let body: serde_json::Value = serde_json::from_str(&body_text).unwrap();
    assert_eq!(body["columns"][0], "n");
    assert_eq!(body["rows"][0][0], 6);
    assert_eq!(body["scanned_rows"], 6);
}

/// 撤掉启动期预 seed 后，写入一个不存在的流应由 `IngestService` 用推断 schema
/// 自动建流（schema-on-write），随后照常 flush + 可查。
#[tokio::test]
async fn ingest_auto_creates_missing_stream() {
    if skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }
    let s = TestServer::start().await;

    // 注意：没有 seed_stream —— 流 `auto_created` 此刻并不存在。
    let resp = s
        .client
        .post(format!("{}/api/v1/ingest/logs/auto_created", s.base_url))
        .header(s.auth_header().0, s.auth_header().1)
        .json(&json!([
            { "_timestamp": 1_000_000, "level": "info", "msg": "hello" },
            { "_timestamp": 2_000_000, "level": "warn", "msg": "world" }
        ]))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "ingest to a non-existent stream must auto-create it"
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["accepted"], 2, "both events accepted");
    assert_eq!(body["rejected"], 0);

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect(&s.settings.store.meta.dsn)
        .await
        .expect("test pool");
    let flushed = {
        let pool = pool.clone();
        common::wait_until_async(10, move || {
            let pool = pool.clone();
            async move {
                let row: (i64,) =
                    sqlx::query_as("SELECT COUNT(*) FROM parquet_file_meta WHERE deleted = FALSE")
                        .fetch_one(&pool)
                        .await
                        .unwrap_or((0,));
                row.0 >= 1
            }
        })
        .await
    };
    assert!(
        flushed,
        "auto-created stream never flushed to parquet_file_meta"
    );

    let resp = s
        .client
        .post(format!("{}/api/v1/query", s.base_url))
        .header(s.auth_header().0, s.auth_header().1)
        .json(&json!({
            "org_id": s.root_org_id.0,
            "language": "sql",
            "statement": "SELECT COUNT(*) AS n FROM auto_created",
            "time_range": { "start": 0, "end": 10_000_000 },
            "stream": { "name": "auto_created", "stream_type": "logs" }
        }))
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let body_text = resp.text().await.unwrap();
    assert_eq!(status, 200, "query status; body={body_text}");
    let body: serde_json::Value = serde_json::from_str(&body_text).unwrap();
    assert_eq!(body["rows"][0][0], 2);
}
