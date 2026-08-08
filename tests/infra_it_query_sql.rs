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
        stream::{
            FieldDef, FieldType, Retention, Schema, StreamDefinition, StreamRepository, StreamType,
        },
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

/// 内存版 `StreamRepository`：只实现 `get`（门槛校验需要），其余 stub。
struct MemStreamRepository {
    def: StreamDefinition,
}

#[async_trait]
impl StreamRepository for MemStreamRepository {
    async fn create(&self, _def: StreamDefinition) -> Result<StreamDefinition> {
        unreachable!("no create in this test")
    }
    async fn update_schema(&self, _id: &Id, _schema: Schema) -> Result<()> {
        unreachable!("no update_schema in this test")
    }
    async fn get(
        &self,
        _org_id: &Id,
        _name: &str,
        _stream_type: StreamType,
    ) -> Result<StreamDefinition> {
        Ok(self.def.clone())
    }
    async fn list(&self, _org_id: &Id) -> Result<Vec<StreamDefinition>> {
        Ok(vec![self.def.clone()])
    }
    async fn delete(&self, _id: &Id) -> Result<()> {
        unreachable!("no delete in this test")
    }
}

fn stream_with(message_indexed: bool, request_id_indexed: bool) -> StreamDefinition {
    let org = Id::from_string("org-1");
    StreamDefinition {
        id: Id::new(),
        org_id: org.clone(),
        name: "logs".into(),
        stream_type: StreamType::Logs,
        schema: Schema {
            fields: vec![
                FieldDef {
                    name: "message".into(),
                    data_type: FieldType::Utf8,
                    nullable: true,
                    indexed: message_indexed,
                    encrypted: false,
                    exact: false,
                },
                FieldDef {
                    name: "request_id".into(),
                    data_type: FieldType::Utf8,
                    nullable: true,
                    indexed: request_id_indexed,
                    encrypted: false,
                    exact: false,
                },
            ],
        },
        retention: Some(Retention { days: 7 }),
        created_at: TimestampMicros::now(),
        updated_at: TimestampMicros::now(),
    }
}

/// 按给定 stream 的 schema 构建一批测试数据（message 含可检索内容）。
fn batch_for(stream: &StreamDefinition) -> RecordBatch {
    let schema = to_arrow(&stream.schema);
    let ts =
        TimestampMicrosecondArray::from(vec![1_000_000, 2_000_000, 3_000_000]).with_timezone("UTC");
    let message = StringArray::from(vec!["info ok", "warn disk full", "error panic at line 1"]);
    let request_id = StringArray::from(vec!["abc", "def", "ghi"]);
    RecordBatch::try_new(
        schema,
        vec![Arc::new(ts), Arc::new(message), Arc::new(request_id)],
    )
    .unwrap()
}

fn match_text_request(org: &Id, statement: &str) -> QueryRequest {
    QueryRequest {
        org_id: org.clone(),
        language: QueryLanguage::Sql,
        statement: statement.to_string(),
        time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(10_000_000)),
        stream: Some(StreamHint {
            name: "logs".into(),
            stream_type: StreamType::Logs,
        }),
        limit: None,
        federation_clusters: Vec::new(),
    }
}

/// MATCH_TEXT 门槛校验（spec text-match-functions，D2/D3）：未配置全文索引的字段报错，
/// 已配置（`indexed && !exact`）的字段正常执行。
#[tokio::test]
async fn match_text_requires_full_text_indexed_field() {
    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
    let org = Id::from_string("org-1");
    // message 有 full_text 索引，request_id 未建索引。
    let stream = stream_with(true, false);
    let writer = ParquetWriter::new(store.clone());
    let meta = writer.flush(&stream, batch_for(&stream)).await.unwrap();
    let repo = Arc::new(MemParquetFileMetaRepo::new());
    repo.insert(meta).await.unwrap();
    let streams = Arc::new(MemStreamRepository {
        def: stream.clone(),
    });
    let engine = DataFusionEngine::new(repo.clone(), store.clone()).with_streams(streams);

    // 未配置索引字段 → 报错，信息指明未配置全文索引。
    let err = engine
        .execute(match_text_request(
            &org,
            "SELECT message FROM logs WHERE MATCH_TEXT(request_id, 'abc')",
        ))
        .await
        .expect_err("unindexed field must fail validation");
    assert!(
        err.to_string().contains("full-text index"),
        "错误信息须指明未配置全文索引，实得: {err}"
    );

    // 已配置字段 → 正常执行（单 token 退化为 MATCH 语义）。
    let res = engine
        .execute(match_text_request(
            &org,
            "SELECT message FROM logs WHERE MATCH_TEXT(message, 'info')",
        ))
        .await
        .expect("indexed field must execute");
    assert_eq!(res.rows.len(), 1, "含 'info' 的行应被返回: {res:?}");

    // 空 query → rewrite 为 FALSE，恒不返回行（spec：空串恒不匹配）。
    let empty = engine
        .execute(match_text_request(
            &org,
            "SELECT message FROM logs WHERE MATCH_TEXT(message, '')",
        ))
        .await
        .expect("empty match_text executes");
    assert_eq!(empty.rows.len(), 0, "空 query 应恒不返回行");
}

/// MATCH_TEXT 单 token 与 MATCH 语义一致（spec：单 token 查询与 MATCH 返回行集合一致）。
#[tokio::test]
async fn match_text_single_token_matches_plain_match_results() {
    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
    let org = Id::from_string("org-1");
    let stream = stream_with(true, true);
    let writer = ParquetWriter::new(store.clone());
    let meta = writer.flush(&stream, batch_for(&stream)).await.unwrap();
    let repo = Arc::new(MemParquetFileMetaRepo::new());
    repo.insert(meta).await.unwrap();
    let streams = Arc::new(MemStreamRepository {
        def: stream.clone(),
    });
    let engine = DataFusionEngine::new(repo.clone(), store.clone()).with_streams(streams);

    let match_text = engine
        .execute(match_text_request(
            &org,
            "SELECT message FROM logs WHERE MATCH_TEXT(message, 'info')",
        ))
        .await
        .expect("match_text executes");
    let plain = engine
        .execute(match_text_request(
            &org,
            "SELECT message FROM logs WHERE MATCH(message, 'info')",
        ))
        .await
        .expect("match executes");
    assert_eq!(match_text.rows, plain.rows);
}
