// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Tantivy 裁剪端到端：写入 3 组显式 Parquet/Tantivy Artifact（其中 1 组含
//! "panic"），跑 SELECT WHERE MATCH(message,'panic') → 验只有 1 个主 Artifact
//! 被实际扫描（其它 2 个被 pruner 剔除）。

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
        storage::{
            ArtifactRole, ArtifactState, DataSegment, DatasetState, IndexPolicy, IndexTypeId,
            PartitionPolicy, PhysicalDataset, PhysicalDatasetId, QueryFile, QueryFileSource,
            StoragePolicy, primary_dataset_type, type_id,
        },
        stream::{
            FieldDef, FieldType, Retention, Schema, StreamDefinition, StreamRepository, StreamType,
        },
    },
    infra::{
        caching::IndexHandleCache,
        query::tantivy_pruner::TantivyPruner,
        search::{datafusion_engine::DataFusionEngine, tantivy_index::IndexHandle},
        storage::parquet::writer::ParquetWriter,
    },
    shared::{
        Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};
use object_store::{ObjectStore, local::LocalFileSystem};

#[derive(Default)]
struct InMemQueryFile {
    inner: StdMutex<HashMap<String, QueryFile>>,
}
impl InMemQueryFile {
    async fn insert(&self, file: QueryFile) -> Result<()> {
        self.inner.lock().unwrap().insert(file.id.0.clone(), file);
        Ok(())
    }
}
#[async_trait]
impl QueryFileSource for InMemQueryFile {
    async fn find(
        &self,
        org: &Id,
        stream: &str,
        st: StreamType,
        range: TimeRange,
    ) -> Result<Vec<QueryFile>> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .values()
            .filter(|f| {
                &f.org_id == org
                    && f.stream == stream
                    && f.stream_type == st
                    && f.time_range.end.0 >= range.start.0
                    && f.time_range.start.0 <= range.end.0
            })
            .cloned()
            .collect())
    }
}

fn logs_stream() -> StreamDefinition {
    StreamDefinition {
        id: Id::new(),
        org_id: Id::from_string("orga"),
        name: "logs".into(),
        stream_type: StreamType::LOGS,
        schema: Schema {
            fields: vec![FieldDef {
                name: "message".into(),
                data_type: FieldType::Utf8,
                nullable: false,
                index_type: None,
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

fn indexed_dataset(stream: &StreamDefinition) -> PhysicalDataset {
    PhysicalDataset {
        id: PhysicalDatasetId::generate(),
        organization_id: stream.org_id.clone(),
        logical_stream_id: stream.id.clone(),
        dataset_type: primary_dataset_type(stream.stream_type).unwrap(),
        dataset_type_version: 1,
        partition_policy: PartitionPolicy::default(),
        storage_policy: StoragePolicy::default(),
        index_policy: IndexPolicy {
            indexers: vec![IndexTypeId::builtin(type_id::builtin::INDEX_TANTIVY)],
        },
        catalog_version: 0,
        state: DatasetState::Active,
        created_at_micros: 0,
        updated_at_micros: 0,
    }
}

async fn write_indexed_fixture(
    writer: &ParquetWriter,
    stream: &StreamDefinition,
    dataset: &PhysicalDataset,
    batch: RecordBatch,
) -> (QueryFile, (String, String)) {
    let mut segments = writer
        .write_compaction_catalog(stream, dataset, batch)
        .await
        .unwrap();
    assert_eq!(segments.len(), 1);
    let segment = segments.remove(0);
    let index = explicit_tantivy_key(&segment);
    let primary_key = segment.primary.object.key.as_str().to_owned();
    let meta = QueryFile {
        id: segment.id.0,
        org_id: stream.org_id.clone(),
        stream: stream.name.clone(),
        stream_type: stream.stream_type,
        dataset_type: primary_dataset_type(stream.stream_type).unwrap(),
        object_key: primary_key.clone(),
        checksum: Some(segment.primary.object.checksum),
        etag: segment.primary.object.etag,
        time_range: segment.time_range,
        rows: segment.row_count,
        size_bytes: segment.primary.object.size_bytes,
        min_values: segment.column_stats.min_values,
        max_values: segment.column_stats.max_values,
    };
    (meta, (primary_key, index))
}

fn explicit_tantivy_key(segment: &DataSegment) -> String {
    segment
        .auxiliaries
        .iter()
        .find(|artifact| {
            artifact.role == ArtifactRole::Index
                && artifact.state == ArtifactState::Ready
                && artifact.artifact_type.as_str() == type_id::builtin::ARTIFACT_TANTIVY
                && artifact.source_artifact_id.as_ref() == Some(&segment.primary.id)
        })
        .expect("fixture has an explicit Tantivy Artifact")
        .object
        .key
        .as_str()
        .to_owned()
}

#[tokio::test]
async fn match_predicate_prunes_two_files_out_of_three() {
    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
    let writer = ParquetWriter::new(store.clone());
    let repo: Arc<InMemQueryFile> = Arc::new(InMemQueryFile::default());
    let stream = logs_stream();
    let dataset = indexed_dataset(&stream);

    let mut index_keys = HashMap::new();
    for batch in [
        build_logs_batch(1_000_000, &["panic at line 1", "ok"]),
        build_logs_batch(2_000_000, &["all good", "still good"]),
        build_logs_batch(3_000_000, &["info only", "warn only"]),
    ] {
        let (meta, (primary, index)) =
            write_indexed_fixture(&writer, &stream, &dataset, batch).await;
        repo.insert(meta).await.unwrap();
        index_keys.insert(primary, index);
    }

    let cache: Arc<IndexHandleCache<Arc<IndexHandle>>> =
        Arc::new(IndexHandleCache::new(CacheLayerSettings::new(100, 60)));
    let pruner = Arc::new(TantivyPruner::new(cache, store.clone()));
    let engine = DataFusionEngine::new(repo.clone() as Arc<dyn QueryFileSource>, store.clone())
        .with_tantivy_pruner(pruner)
        .with_tantivy_index_keys(index_keys);

    // 查 SELECT count(*) WHERE MATCH(message, 'panic')
    let req = QueryRequest {
        org_id: stream.org_id.clone(),
        language: QueryLanguage::Sql,
        statement: "SELECT count(*) AS n FROM logs WHERE MATCH(message, 'panic')".into(),
        time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX)),
        stream: Some(StreamHint {
            name: "logs".into(),
            stream_type: StreamType::LOGS,
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
        stream_type: StreamType::TRACES,
        schema: Schema {
            fields: vec![FieldDef {
                name: "trace_id".into(),
                data_type: FieldType::Utf8,
                nullable: false,
                index_type: None,
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

/// `WHERE trace_id = '<A>'` 对 exact-indexed 字段触发 tantivy 等值裁剪：3 个主
/// Artifact 各含不同 trace_id，只有含目标值的对象被扫描。
#[tokio::test]
async fn equality_predicate_on_exact_field_prunes_files() {
    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
    let writer = ParquetWriter::new(store.clone());
    let repo: Arc<InMemQueryFile> = Arc::new(InMemQueryFile::default());
    let stream = traces_stream();
    let dataset = indexed_dataset(&stream);

    let a = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
    let b = "b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2";
    let c = "c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3";
    let mut index_keys = HashMap::new();
    for (start, tids) in [
        (1_000_000, [a, a]),
        (2_000_000, [b, b]),
        (3_000_000, [c, c]),
    ] {
        let (meta, (primary, index)) =
            write_indexed_fixture(&writer, &stream, &dataset, build_traces_batch(start, &tids))
                .await;
        repo.insert(meta).await.unwrap();
        index_keys.insert(primary, index);
    }

    let cache: Arc<IndexHandleCache<Arc<IndexHandle>>> =
        Arc::new(IndexHandleCache::new(CacheLayerSettings::new(100, 60)));
    let pruner = Arc::new(TantivyPruner::new(cache, store.clone()));
    let engine = DataFusionEngine::new(repo.clone() as Arc<dyn QueryFileSource>, store.clone())
        .with_streams(Arc::new(OneTracesRepo(stream.clone())) as Arc<dyn StreamRepository>)
        .with_tantivy_pruner(pruner)
        .with_tantivy_index_keys(index_keys);

    let req = QueryRequest {
        org_id: stream.org_id.clone(),
        language: QueryLanguage::Sql,
        statement: format!("SELECT count(*) AS n FROM traces WHERE trace_id = '{a}'"),
        time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX)),
        stream: Some(StreamHint {
            name: "traces".into(),
            stream_type: StreamType::TRACES,
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
