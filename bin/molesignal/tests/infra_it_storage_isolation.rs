// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 存储层前缀隔离端到端（in-mem）：
//!
//! - 正常的 Catalog 只返回本 org 的文件 → 查询照常返回本 org 的行
//! - Catalog 出错、把别的 org 的文件混进候选 → 存储层拒绝读取，查询失败，
//!   而不是读到别的 org 的数据
//!
//! 与 `infra_it_multitenant` 互补：那里验证 Catalog 正确时的隔离结果，这里验证 Catalog
//! 不可信时存储层自己的边界。

mod common;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex as StdMutex},
};

use arrow::{
    array::{Int64Array, RecordBatch, TimestampMicrosecondArray},
    datatypes::{DataType, Field, Schema as ArrowSchema, TimeUnit},
};
use async_trait::async_trait;
use common::write_parquet_fixture;
use molesignal::{
    domain::{
        query::{QueryEngine, QueryLanguage, QueryRequest, StreamHint},
        storage::{QueryFile, QueryFileSource},
        stream::{
            FieldDef, FieldType, Retention, Schema, StreamDefinition, StreamRepository, StreamType,
        },
    },
    infra::{search::datafusion_engine::DataFusionEngine, storage::parquet::writer::ParquetWriter},
    shared::{
        Error, Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};
use object_store::{ObjectStore, local::LocalFileSystem};

/// `leaky = true` 模拟 Catalog 缺陷：忽略请求的 org，把所有 org 的文件都当候选返回。
struct FakeCatalog {
    leaky: bool,
    files: StdMutex<Vec<QueryFile>>,
}

#[async_trait]
impl QueryFileSource for FakeCatalog {
    async fn find(
        &self,
        org: &Id,
        stream: &str,
        st: StreamType,
        range: TimeRange,
    ) -> Result<Vec<QueryFile>> {
        Ok(self
            .files
            .lock()
            .unwrap()
            .iter()
            .filter(|f| {
                (self.leaky || &f.org_id == org)
                    && f.stream == stream
                    && f.stream_type == st
                    && f.time_range.end.0 >= range.start.0
                    && f.time_range.start.0 <= range.end.0
            })
            .cloned()
            .collect())
    }
}

struct Streams {
    inner: StdMutex<HashMap<(Id, String, StreamType), StreamDefinition>>,
}

#[async_trait]
impl StreamRepository for Streams {
    async fn create(&self, def: StreamDefinition) -> Result<StreamDefinition> {
        self.inner.lock().unwrap().insert(
            (def.org_id.clone(), def.name.clone(), def.stream_type),
            def.clone(),
        );
        Ok(def)
    }
    async fn update_schema(&self, _id: &Id, _s: Schema) -> Result<()> {
        Ok(())
    }
    async fn get(&self, org: &Id, name: &str, st: StreamType) -> Result<StreamDefinition> {
        self.inner
            .lock()
            .unwrap()
            .get(&(org.clone(), name.to_string(), st))
            .cloned()
            .ok_or_else(|| Error::not_found(format!("stream {name}")))
    }
    async fn list(&self, _org: &Id) -> Result<Vec<StreamDefinition>> {
        Ok(self.inner.lock().unwrap().values().cloned().collect())
    }
    async fn delete(&self, _id: &Id) -> Result<()> {
        Ok(())
    }
}

fn stream_def(org: &str) -> StreamDefinition {
    StreamDefinition {
        id: Id::new(),
        org_id: Id::from_string(org),
        name: "app".into(),
        stream_type: StreamType::LOGS,
        schema: Schema {
            fields: vec![FieldDef {
                name: "n".into(),
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

fn batch(ts: i64, rows: i64) -> RecordBatch {
    let schema = Arc::new(ArrowSchema::new(vec![
        Field::new(
            "_timestamp",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        ),
        Field::new("n", DataType::Int64, false),
    ]));
    let t = TimestampMicrosecondArray::from((0..rows).map(|i| ts + i).collect::<Vec<_>>())
        .with_timezone("UTC");
    let v = Int64Array::from((0..rows).collect::<Vec<_>>());
    RecordBatch::try_new(schema, vec![Arc::new(t), Arc::new(v)]).unwrap()
}

fn count_request(org: &str) -> QueryRequest {
    QueryRequest {
        org_id: Id::from_string(org),
        language: QueryLanguage::Sql,
        statement: "SELECT count(*) AS n FROM app".into(),
        time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX)),
        stream: Some(StreamHint {
            name: "app".into(),
            stream_type: StreamType::LOGS,
        }),
        limit: None,
        federation_clusters: Vec::new(),
    }
}

/// 两个 org 各写一个文件（org-a 50 行、org-b 30 行），返回引擎。
async fn build_engine(leaky: bool) -> (DataFusionEngine, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
    let writer = ParquetWriter::new(store.clone());

    let streams = Arc::new(Streams {
        inner: StdMutex::new(HashMap::new()),
    });
    let stream_a = stream_def("orga");
    let stream_b = stream_def("orgb");
    streams.create(stream_a.clone()).await.unwrap();
    streams.create(stream_b.clone()).await.unwrap();

    let file_a = write_parquet_fixture(&writer, &stream_a, batch(1_000_000, 50)).await;
    let file_b = write_parquet_fixture(&writer, &stream_b, batch(2_000_000, 30)).await;

    let catalog = Arc::new(FakeCatalog {
        leaky,
        files: StdMutex::new(vec![file_a, file_b]),
    });

    let engine = DataFusionEngine::new(catalog as Arc<dyn QueryFileSource>, store)
        .with_streams(streams as Arc<dyn StreamRepository>);

    (engine, tmp)
}

#[tokio::test]
async fn own_prefix_reads_still_work() {
    let (engine, _tmp) = build_engine(false).await;

    let result = engine.execute(count_request("orga")).await.unwrap();

    let n = result.rows[0][0]
        .as_i64()
        .or_else(|| result.rows[0][0].as_u64().map(|u| u as i64))
        .unwrap();
    assert_eq!(n, 50);
}

#[tokio::test]
async fn foreign_candidates_are_refused_at_the_storage_layer() {
    // Catalog 把 org-b 的文件也当作 org-a 的候选返回。
    let (engine, _tmp) = build_engine(true).await;

    let error = engine
        .execute(count_request("orga"))
        .await
        .expect_err("a file under another organization's prefix must not be readable");

    assert!(
        error.to_string().contains("storage prefix"),
        "unexpected error: {error}"
    );
}
