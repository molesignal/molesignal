// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Tantivy 裁剪端到端：3 个 parquet（其中 1 个含 "panic"），跑 SELECT WHERE MATCH(message,'panic')
//! → 验只有 1 个 file 被实际扫描（其它 2 个被 pruner 剔除）。

use std::{
    collections::HashMap,
    sync::{Arc, Mutex as StdMutex},
};

use arrow::{
    array::{RecordBatch, StringArray, TimestampMicrosecondArray},
    datatypes::{DataType, Field, Schema as ArrowSchema, TimeUnit},
};
use async_trait::async_trait;
use molesignal::{
    config::CacheLayerSettings,
    domain::{
        query::{QueryEngine, QueryLanguage, QueryRequest, StreamHint},
        storage::{ParquetFileMeta, ParquetFileMetaRepository},
        stream::{
            FieldDef, FieldType, Retention, Schema, StreamDefinition, StreamRepository, StreamType,
        },
    },
    infra::{
        caching::ParquetMetaCache,
        query::tantivy_pruner::TantivyPruner,
        search::{datafusion_engine::DataFusionEngine, tantivy_index::IndexHandle},
        storage::parquet::writer::ParquetWriter,
    },
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
        Err(Error::internal("not supported"))
    }

    async fn mark_deleted(&self, _ids: &[Id]) -> Result<usize> {
        Err(Error::internal("not supported"))
    }
}

fn logs_stream() -> StreamDefinition {
    StreamDefinition {
        id: Id::new(),
        org_id: Id::from_string("orga"),
        name: "logs".into(),
        stream_type: StreamType::Logs,
        schema: Schema {
            fields: vec![FieldDef {
                name: "message".into(),
                data_type: FieldType::Utf8,
                nullable: false,
                indexed: true, // tantivy 索引
                encrypted: false,
                exact: false,
            }],
        },
        retention: Some(Retention { days: 7 }),
        created_at: TimestampMicros::now(),
        updated_at: TimestampMicros::now(),
    }
}

fn build_logs_batch(start_us: i64, messages: &[&str]) -> RecordBatch {
    let schema = Arc::new(ArrowSchema::new(vec![
        Field::new(
            "_timestamp",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        ),
        Field::new("message", DataType::Utf8, false),
    ]));
    let ts = TimestampMicrosecondArray::from(
        (0..messages.len() as i64)
            .map(|i| start_us + i * 1000)
            .collect::<Vec<_>>(),
    )
    .with_timezone("UTC");
    let msgs = StringArray::from(messages.to_vec());
    RecordBatch::try_new(schema, vec![Arc::new(ts), Arc::new(msgs)]).unwrap()
}

#[tokio::test]
async fn match_predicate_prunes_two_files_out_of_three() {
    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
    let writer = ParquetWriter::new(store.clone());
    let repo: Arc<InMemParquetFileMeta> = Arc::new(InMemParquetFileMeta::default());
    let stream = logs_stream();

    // File A: contains "panic"
    let metas = vec![
        writer
            .flush_with_index(
                &stream,
                build_logs_batch(1_000_000, &["panic at line 1", "ok"]),
            )
            .await
            .unwrap(),
        writer
            .flush_with_index(
                &stream,
                build_logs_batch(2_000_000, &["all good", "still good"]),
            )
            .await
            .unwrap(),
        writer
            .flush_with_index(
                &stream,
                build_logs_batch(3_000_000, &["info only", "warn only"]),
            )
            .await
            .unwrap(),
    ];
    for (m, _) in metas {
        repo.insert(m).await.unwrap();
    }

    let cache: Arc<ParquetMetaCache<Arc<IndexHandle>>> =
        Arc::new(ParquetMetaCache::new(CacheLayerSettings::new(100, 60)));
    let pruner = Arc::new(TantivyPruner::new(cache, store.clone()));
    let engine = DataFusionEngine::new(
        repo.clone() as Arc<dyn ParquetFileMetaRepository>,
        store.clone(),
    )
    .with_tantivy_pruner(pruner);

    // 查 SELECT count(*) WHERE MATCH(message, 'panic')
    let req = QueryRequest {
        org_id: stream.org_id.clone(),
        language: QueryLanguage::Sql,
        statement: "SELECT count(*) AS n FROM logs WHERE MATCH(message, 'panic')".into(),
        time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX)),
        stream: Some(StreamHint {
            name: "logs".into(),
            stream_type: StreamType::Logs,
        }),
        limit: None,
        federation_clusters: Vec::new(),
    };
    let res = engine.execute(req).await.expect("query");
    // 仅 1 个 file 被实际扫描 → 含 panic 的 file（2 行，其中 1 行匹配 LIKE '%panic%'）
    // 因此 scanned_rows = 2（只扫了 file A），count(*) = 1（仅 "panic at line 1" 匹配）
    assert_eq!(
        res.scanned_rows, 2,
        "only file A (2 rows) survived tantivy prune; got scanned_rows={}",
        res.scanned_rows
    );
    assert_eq!(res.rows.len(), 1);
    let count_val = &res.rows[0][0];
    let n = count_val
        .as_i64()
        .or_else(|| count_val.as_u64().map(|u| u as i64))
        .unwrap();
    assert_eq!(n, 1, "only 1 row matches LIKE '%panic%'");
}

/// 单流 StreamRepository：`get` 恒返回同一个 traces 定义，`get_settings` 走 default
/// （queryable=true）。用于让 DataFusionEngine 拿到 schema 判断 exact 字段。
struct OneTracesRepo(StreamDefinition);

#[async_trait]
impl StreamRepository for OneTracesRepo {
    async fn create(&self, def: StreamDefinition) -> Result<StreamDefinition> {
        Ok(def)
    }
    async fn update_schema(&self, _id: &Id, _schema: Schema) -> Result<()> {
        Ok(())
    }
    async fn get(&self, _org: &Id, _name: &str, _st: StreamType) -> Result<StreamDefinition> {
        Ok(self.0.clone())
    }
    async fn list(&self, _org: &Id) -> Result<Vec<StreamDefinition>> {
        Ok(vec![self.0.clone()])
    }
    async fn delete(&self, _id: &Id) -> Result<()> {
        Ok(())
    }
}

fn traces_stream() -> StreamDefinition {
    StreamDefinition {
        id: Id::new(),
        org_id: Id::from_string("orga"),
        name: "traces".into(),
        stream_type: StreamType::Traces,
        schema: Schema {
            fields: vec![FieldDef {
                name: "trace_id".into(),
                data_type: FieldType::Utf8,
                nullable: false,
                indexed: true,
                encrypted: false,
                exact: true, // 未分词 STRING 索引 → 等值裁剪
            }],
        },
        retention: Some(Retention { days: 7 }),
        created_at: TimestampMicros::now(),
        updated_at: TimestampMicros::now(),
    }
}

fn build_traces_batch(start_us: i64, trace_ids: &[&str]) -> RecordBatch {
    let schema = Arc::new(ArrowSchema::new(vec![
        Field::new(
            "_timestamp",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        ),
        Field::new("trace_id", DataType::Utf8, false),
    ]));
    let ts = TimestampMicrosecondArray::from(
        (0..trace_ids.len() as i64)
            .map(|i| start_us + i * 1000)
            .collect::<Vec<_>>(),
    )
    .with_timezone("UTC");
    let ids = StringArray::from(trace_ids.to_vec());
    RecordBatch::try_new(schema, vec![Arc::new(ts), Arc::new(ids)]).unwrap()
}

/// `WHERE trace_id = '<A>'` 对 exact-indexed 字段触发 tantivy 等值裁剪：3 个 parquet
/// 各含不同 trace_id，只有含目标值的文件被扫描（其余被 STRING 索引 count_term=0 剔除）。
#[tokio::test]
async fn equality_predicate_on_exact_field_prunes_files() {
    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
    let writer = ParquetWriter::new(store.clone());
    let repo: Arc<InMemParquetFileMeta> = Arc::new(InMemParquetFileMeta::default());
    let stream = traces_stream();

    let a = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
    let b = "b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2";
    let c = "c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3";
    for (start, tids) in [
        (1_000_000, [a, a]),
        (2_000_000, [b, b]),
        (3_000_000, [c, c]),
    ] {
        let (m, _) = writer
            .flush_with_index(&stream, build_traces_batch(start, &tids))
            .await
            .unwrap();
        repo.insert(m).await.unwrap();
    }

    let cache: Arc<ParquetMetaCache<Arc<IndexHandle>>> =
        Arc::new(ParquetMetaCache::new(CacheLayerSettings::new(100, 60)));
    let pruner = Arc::new(TantivyPruner::new(cache, store.clone()));
    let engine = DataFusionEngine::new(
        repo.clone() as Arc<dyn ParquetFileMetaRepository>,
        store.clone(),
    )
    .with_streams(Arc::new(OneTracesRepo(stream.clone())) as Arc<dyn StreamRepository>)
    .with_tantivy_pruner(pruner);

    let req = QueryRequest {
        org_id: stream.org_id.clone(),
        language: QueryLanguage::Sql,
        statement: format!("SELECT count(*) AS n FROM traces WHERE trace_id = '{a}'"),
        time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX)),
        stream: Some(StreamHint {
            name: "traces".into(),
            stream_type: StreamType::Traces,
        }),
        limit: None,
        federation_clusters: Vec::new(),
    };
    let res = engine.execute(req).await.expect("query");
    // 只有 File A（2 行）被扫描；B/C 被等值裁剪剔除。
    assert_eq!(
        res.scanned_rows, 2,
        "only file A survived equality prune; got scanned_rows={}",
        res.scanned_rows
    );
    let n = res.rows[0][0]
        .as_i64()
        .or_else(|| res.rows[0][0].as_u64().map(|u| u as i64))
        .unwrap();
    assert_eq!(n, 2, "both rows in file A carry trace_id = a");
}
