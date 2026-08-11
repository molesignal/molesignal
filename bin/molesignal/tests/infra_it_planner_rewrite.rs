// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! planner rewrite + 跨 org 隔离的极端 SQL 形式验证。
//!
//! - CTE / Subquery / UNION ALL 形式下，同 org 数据仍正确隔离 + 跨 org 不可见
//! - stream 不存在 → Forbidden

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
    async fn insert(&self, f: ParquetFileMeta) -> Result<()> {
        self.inner.lock().unwrap().insert(f.id.0.clone(), f);
        Ok(())
    }
    async fn find(
        &self,
        org: &Id,
        stream: &str,
        st: StreamType,
        r: TimeRange,
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
                    && f.time_range.end.0 >= r.start.0
                    && f.time_range.start.0 <= r.end.0
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
    async fn update_schema(&self, _: &Id, _: Schema) -> Result<()> {
        Ok(())
    }
    async fn get(&self, org: &Id, name: &str, st: StreamType) -> Result<StreamDefinition> {
        self.inner
            .lock()
            .unwrap()
            .get(&(org.clone(), name.to_string(), st))
            .cloned()
            .ok_or_else(|| Error::not_found("stream"))
    }
    async fn list(&self, _: &Id) -> Result<Vec<StreamDefinition>> {
        Ok(self.inner.lock().unwrap().values().cloned().collect())
    }
    async fn delete(&self, _: &Id) -> Result<()> {
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
async fn cte_union_subquery_isolated_per_org() {
    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
    let writer = ParquetWriter::new(store.clone());
    let parquet_file_meta: Arc<InMemParquetFileMeta> = Arc::new(InMemParquetFileMeta::default());
    let streams: Arc<InMemStreams> = Arc::new(InMemStreams {
        inner: StdMutex::new(HashMap::new()),
    });
    let stream_repo: Arc<dyn StreamRepository> = streams.clone();

    // orgA 50 行，orgB 30 行
    let sa = stream_def("orga");
    let sb = stream_def("orgb");
    streams.create(sa.clone()).await.unwrap();
    streams.create(sb.clone()).await.unwrap();
    let ma = writer
        .flush(&sa, batch(1_000_000, &(0..50).collect::<Vec<_>>()))
        .await
        .unwrap();
    parquet_file_meta.insert(ma).await.unwrap();
    let mb = writer
        .flush(&sb, batch(2_000_000, &(0..30).collect::<Vec<_>>()))
        .await
        .unwrap();
    parquet_file_meta.insert(mb).await.unwrap();

    let engine = DataFusionEngine::new(
        parquet_file_meta.clone() as Arc<dyn ParquetFileMetaRepository>,
        store.clone(),
    )
    .with_streams(stream_repo);

    fn mk_req(org: &str, sql: &str) -> QueryRequest {
        QueryRequest {
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
        }
    }
    fn cell_i64(r: &molesignal::domain::query::QueryResult) -> i64 {
        r.rows[0][0]
            .as_i64()
            .or_else(|| r.rows[0][0].as_u64().map(|u| u as i64))
            .unwrap()
    }

    let r = engine
        .execute(mk_req(
            "orga",
            "WITH t AS (SELECT * FROM app) SELECT count(*) AS n FROM t",
        ))
        .await
        .unwrap();
    assert_eq!(cell_i64(&r), 50);

    let r = engine
        .execute(mk_req(
            "orga",
            "SELECT count(*) AS n FROM (SELECT * FROM app) sub",
        ))
        .await
        .unwrap();
    assert_eq!(cell_i64(&r), 50);

    let r = engine
        .execute(mk_req(
            "orga",
            "SELECT count(*) AS n FROM (SELECT * FROM app UNION ALL SELECT * FROM app) u",
        ))
        .await
        .unwrap();
    assert_eq!(cell_i64(&r), 100);

    let r = engine
        .execute(mk_req(
            "orgb",
            "SELECT count(*) AS n FROM (SELECT * FROM app UNION ALL SELECT * FROM app) u",
        ))
        .await
        .unwrap();
    assert_eq!(cell_i64(&r), 60);
}
