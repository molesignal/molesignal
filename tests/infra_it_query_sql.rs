// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! SQL 查询端到端冒烟（local object_store + 内存 ParquetFileMetaRepository）。

use std::sync::Arc;

use arrow::array::{Int64Array, RecordBatch, StringArray, TimestampMicrosecondArray};
use async_trait::async_trait;
use molesignal::{
    domain::{
        query::{QueryEngine, QueryLanguage, QueryRequest, StreamHint},
        storage::{ParquetFileMeta, ParquetFileMetaRepository},
        stream::{FieldDef, FieldType, Retention, Schema, StreamDefinition, StreamType},
    },
    infra::{
        search::datafusion_engine::DataFusionEngine,
        storage::{arrow_schema::to_arrow, parquet::writer::ParquetWriter},
    },
    shared::{
        Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};
use object_store::{ObjectStore, local::LocalFileSystem};
use parking_lot::Mutex;

struct MemParquetFileMetaRepo {
    files: Mutex<Vec<ParquetFileMeta>>,
}

impl MemParquetFileMetaRepo {
    fn new() -> Self {
        Self {
            files: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl ParquetFileMetaRepository for MemParquetFileMetaRepo {
    async fn insert(&self, file: ParquetFileMeta) -> Result<()> {
        self.files.lock().push(file);
        Ok(())
    }
    async fn find(
        &self,
        org_id: &Id,
        stream: &str,
        stream_type: StreamType,
        time_range: TimeRange,
    ) -> Result<Vec<ParquetFileMeta>> {
        Ok(self
            .files
            .lock()
            .iter()
            .filter(|f| {
                &f.org_id == org_id
                    && f.stream == stream
                    && f.stream_type == stream_type
                    && !f.deleted
                    && f.time_range.end.0 >= time_range.start.0
                    && f.time_range.start.0 <= time_range.end.0
            })
            .cloned()
            .collect())
    }
    async fn replace(&self, _merged_ids: &[Id], _new_files: Vec<ParquetFileMeta>) -> Result<()> {
        unimplemented!()
    }
    async fn mark_deleted(&self, _ids: &[Id]) -> Result<usize> {
        unimplemented!()
    }
}

fn sample_stream(org: &Id) -> StreamDefinition {
    StreamDefinition {
        id: Id::new(),
        org_id: org.clone(),
        name: "app".into(),
        stream_type: StreamType::Logs,
        schema: Schema {
            fields: vec![
                FieldDef {
                    name: "level".into(),
                    data_type: FieldType::Utf8,
                    nullable: false,
                    indexed: true,
                    encrypted: false,
                    exact: false,
                },
                FieldDef {
                    name: "latency_ms".into(),
                    data_type: FieldType::Int64,
                    nullable: true,
                    indexed: true,
                    encrypted: false,
                    exact: false,
                },
            ],
        },
        retention: Some(Retention { days: 30 }),
        created_at: TimestampMicros::now(),
        updated_at: TimestampMicros::now(),
    }
}

fn sample_batch(stream: &StreamDefinition) -> RecordBatch {
    let schema = to_arrow(&stream.schema);
    let ts =
        TimestampMicrosecondArray::from(vec![1_000_000, 2_000_000, 3_000_000]).with_timezone("UTC");
    let level = StringArray::from(vec!["info", "warn", "error"]);
    let latency = Int64Array::from(vec![Some(10), Some(20), Some(30)]);
    RecordBatch::try_new(
        schema,
        vec![Arc::new(ts), Arc::new(level), Arc::new(latency)],
    )
    .unwrap()
}

#[tokio::test]
async fn count_and_aggregate_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());

    let org = Id::from_string("org-1");
    let stream = sample_stream(&org);
    let writer = ParquetWriter::new(store.clone());

    let meta = writer.flush(&stream, sample_batch(&stream)).await.unwrap();
    let repo = Arc::new(MemParquetFileMetaRepo::new());
    repo.insert(meta).await.unwrap();

    let engine = DataFusionEngine::new(repo.clone(), store.clone());

    // count
    let res = engine
        .execute(QueryRequest {
            org_id: org.clone(),
            language: QueryLanguage::Sql,
            statement: "SELECT COUNT(*) AS cnt FROM app".into(),
            time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(10_000_000)),
            stream: Some(StreamHint {
                name: "app".into(),
                stream_type: StreamType::Logs,
            }),
            limit: None,
            federation_clusters: Vec::new(),
        })
        .await
        .expect("execute count");
    assert_eq!(res.scanned_rows, 3);
    assert_eq!(res.columns, vec!["cnt"]);
    assert_eq!(res.rows.len(), 1);
    assert_eq!(res.rows[0][0], 3);

    // filter + projection
    let res2 =
        engine
            .execute(QueryRequest {
                org_id: org.clone(),
                language: QueryLanguage::Sql,
                statement:
                    "SELECT level, latency_ms FROM app WHERE latency_ms > 15 ORDER BY latency_ms"
                        .into(),
                time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(10_000_000)),
                stream: Some(StreamHint {
                    name: "app".into(),
                    stream_type: StreamType::Logs,
                }),
                limit: None,
                federation_clusters: Vec::new(),
            })
            .await
            .expect("execute filter");
    assert_eq!(res2.columns, vec!["level", "latency_ms"]);
    assert_eq!(res2.rows.len(), 2);
    assert_eq!(res2.rows[0][0], "warn");
    assert_eq!(res2.rows[0][1], 20);
    assert_eq!(res2.rows[1][0], "error");
    assert_eq!(res2.rows[1][1], 30);

    // 时间窗外 → 0 行
    let empty = engine
        .execute(QueryRequest {
            org_id: org.clone(),
            language: QueryLanguage::Sql,
            statement: "SELECT COUNT(*) AS cnt FROM app".into(),
            time_range: TimeRange::new(TimestampMicros(20_000_000), TimestampMicros(30_000_000)),
            stream: Some(StreamHint {
                name: "app".into(),
                stream_type: StreamType::Logs,
            }),
            limit: None,
            federation_clusters: Vec::new(),
        })
        .await
        .expect("execute empty");
    assert_eq!(empty.scanned_rows, 0);
}
