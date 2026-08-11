// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 多租户隔离端到端（in-mem）：
//!
//! - 两 org（A、B）建同名 stream `app`，分别写 50 / 30 行
//! - orgA SELECT count(*) FROM app → 50
//! - orgB SELECT count(*) FROM app → 30
//! - 不存在的 stream `ghost` → Forbidden

use std::{
    collections::HashMap,
    sync::{Arc, Mutex as StdMutex},
};

use arrow::{
    array::{Int64Array, RecordBatch, TimestampMicrosecondArray},
    datatypes::{DataType, Field, Schema as ArrowSchema, TimeUnit},
};
use async_trait::async_trait;
use molesignal::{
    domain::{
        query::{QueryEngine, QueryLanguage, QueryRequest, StreamHint},
        storage::{ParquetFileMeta, ParquetFileMetaRepository},
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

#[derive(Default)]
struct InMemParquetFileMeta {
    inner: StdMutex<HashMap<String, ParquetFileMeta>>,
}
#[async_trait]
impl ParquetFileMetaRepository for InMemParquetFileMeta {
    async fn insert(&self, file: ParquetFileMeta) -> Result<()> {
        self.inner.lock().unwrap().insert(file.id.0.clone(), file);
        Ok(())
    }
    async fn find(
        &self,
        org: &Id,
        stream: &str,
        st: StreamType,
        range: TimeRange,
    ) -> Result<Vec<ParquetFileMeta>> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .values()
            .filter(|f| {
                !f.deleted
                    && &f.org_id == org
                    && f.stream == stream
                    && f.stream_type == st
                    && f.time_range.end.0 >= range.start.0
                    && f.time_range.start.0 <= range.end.0
            })
            .cloned()
            .collect())
    }
    async fn replace(&self, _: &[Id], _: Vec<ParquetFileMeta>) -> Result<()> {
        Err(Error::internal("noop"))
    }

    async fn mark_deleted(&self, _ids: &[Id]) -> Result<usize> {
        Err(Error::internal("noop"))
    }
}

struct InMemStreams {
    inner: StdMutex<HashMap<(Id, String, StreamType), StreamDefinition>>,
}
#[async_trait]
impl StreamRepository for InMemStreams {
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
        stream_type: StreamType::Logs,
        schema: Schema {
            fields: vec![FieldDef {
                name: "n".into(),
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

fn batch(ts: i64, vals: &[i64]) -> RecordBatch {
    let schema = Arc::new(ArrowSchema::new(vec![
        Field::new(
            "_timestamp",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        ),
        Field::new("n", DataType::Int64, false),
    ]));
    let t =
        TimestampMicrosecondArray::from((0..vals.len() as i64).map(|i| ts + i).collect::<Vec<_>>())
            .with_timezone("UTC");
    let v = Int64Array::from(vals.to_vec());
    RecordBatch::try_new(schema, vec![Arc::new(t), Arc::new(v)]).unwrap()
}

#[tokio::test]
async fn cross_org_isolation_and_forbidden() {
    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
    let writer = ParquetWriter::new(store.clone());
    let parquet_file_meta: Arc<InMemParquetFileMeta> = Arc::new(InMemParquetFileMeta::default());
    let streams: Arc<InMemStreams> = Arc::new(InMemStreams {
        inner: StdMutex::new(HashMap::new()),
    });
    let stream_repo: Arc<dyn StreamRepository> = streams.clone();

    // 两 org 同名 stream
    let stream_a = stream_def("orga");
    let stream_b = stream_def("orgb");
    streams.create(stream_a.clone()).await.unwrap();
    streams.create(stream_b.clone()).await.unwrap();

    // orgA 写 50 行
    let m1 = writer
        .flush(&stream_a, batch(1_000_000, &(0..50).collect::<Vec<_>>()))
        .await
        .unwrap();
    parquet_file_meta.insert(m1).await.unwrap();
    // orgB 写 30 行
    let m2 = writer
        .flush(&stream_b, batch(2_000_000, &(0..30).collect::<Vec<_>>()))
        .await
        .unwrap();
    parquet_file_meta.insert(m2).await.unwrap();

    let engine = DataFusionEngine::new(
        parquet_file_meta.clone() as Arc<dyn ParquetFileMetaRepository>,
        store.clone(),
    )
    .with_streams(stream_repo);

    let mk_req = |org: &str, sql: &str| QueryRequest {
        org_id: Id::from_string(org),
        language: QueryLanguage::Sql,
        statement: sql.into(),
        time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX)),
        stream: Some(StreamHint {
            name: "app".into(),
            stream_type: StreamType::Logs,
        }),
        limit: None,
        federation_clusters: Vec::new(),
    };

    // orgA → 50
    let a = engine
        .execute(mk_req("orga", "SELECT count(*) AS n FROM app"))
        .await
        .unwrap();
    let n: i64 = a.rows[0][0]
        .as_i64()
        .or_else(|| a.rows[0][0].as_u64().map(|u| u as i64))
        .unwrap();
    assert_eq!(n, 50);

    // orgB → 30
    let b = engine
        .execute(mk_req("orgb", "SELECT count(*) AS n FROM app"))
        .await
        .unwrap();
    let n: i64 = b.rows[0][0]
        .as_i64()
        .or_else(|| b.rows[0][0].as_u64().map(|u| u as i64))
        .unwrap();
    assert_eq!(n, 30);

    // CTE 形式仍然隔离
    let cte = engine
        .execute(mk_req(
            "orga",
            "WITH x AS (SELECT * FROM app) SELECT count(*) AS n FROM x",
        ))
        .await
        .unwrap();
    let n: i64 = cte.rows[0][0]
        .as_i64()
        .or_else(|| cte.rows[0][0].as_u64().map(|u| u as i64))
        .unwrap();
    assert_eq!(n, 50);

    // 不存在的 stream → Forbidden
    let mut bad = mk_req("orga", "SELECT * FROM ghost");
    bad.stream = Some(StreamHint {
        name: "ghost".into(),
        stream_type: StreamType::Logs,
    });
    let err = engine.execute(bad).await.unwrap_err();
    assert_eq!(err.http_status_code(), 403, "expected 403 Forbidden");
    assert!(
        err.to_string().contains("stream not found"),
        "got error: {err}"
    );
}
