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
        value_sets: segment.column_stats.value_sets,
        address_ranges: segment.column_stats.address_ranges,
        null_counts: segment.column_stats.null_counts,
        indexed_at_micros: segment.column_stats.indexed_at_micros,
        shard: None,
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

/// 每个主 Artifact 是 `(第一行的时间, 各行的 message)`，每行晚 1 ms；各带显式 Tantivy Artifact。
async fn logs_engine_of(
    files: &[(i64, &[&str])],
) -> (tempfile::TempDir, DataFusionEngine, StreamDefinition) {
    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
    let writer = ParquetWriter::new(store.clone());
    let repo: Arc<InMemQueryFile> = Arc::new(InMemQueryFile::default());
    let stream = logs_stream();
    let dataset = indexed_dataset(&stream);

    let mut index_keys = HashMap::new();
    for (start, messages) in files {
        let (meta, (primary, index)) = write_indexed_fixture(
            &writer,
            &stream,
            &dataset,
            build_logs_batch(*start, messages),
        )
        .await;
        repo.insert(meta).await.unwrap();
        index_keys.insert(primary, index);
    }

    let cache: Arc<IndexHandleCache<Arc<IndexHandle>>> =
        Arc::new(IndexHandleCache::new(CacheLayerSettings::new(100, 60)));
    let pruner = Arc::new(TantivyPruner::new(cache, store.clone()));
    let engine = DataFusionEngine::new(repo.clone() as Arc<dyn QueryFileSource>, store.clone())
        .with_streams(Arc::new(OneStreamRepo(stream.clone())) as Arc<dyn StreamRepository>)
        .with_tantivy_pruner(pruner)
        .with_tantivy_index_keys(index_keys);

    (tmp, engine, stream)
}

fn logs_request(stream: &StreamDefinition, sql: &str) -> QueryRequest {
    QueryRequest {
        org_id: stream.org_id.clone(),
        language: QueryLanguage::Sql,
        statement: sql.to_owned(),
        time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX)),
        stream: Some(StreamHint {
            name: "logs".into(),
            stream_type: StreamType::LOGS,
        }),
        limit: None,
        step: None,
        federation_clusters: Vec::new(),
    }
}

#[tokio::test]
async fn match_predicate_prunes_two_files_out_of_three() {
    let (_tmp, engine, stream) = logs_engine_of(&[
        (1_000_000, &["panic at line 1", "ok"]),
        (2_000_000, &["all good", "still good"]),
        (3_000_000, &["info only", "warn only"]),
    ])
    .await;

    // 查 SELECT message WHERE MATCH(message, 'panic')
    let res = engine
        .execute(logs_request(
            &stream,
            "SELECT message FROM logs WHERE MATCH(message, 'panic')",
        ))
        .await
        .expect("query");
    // 仅 1 个 file 被实际扫描 → 含 panic 的 file（2 行，其中 1 行匹配 LIKE '%panic%'）
    // 因此 scanned_rows = 2（只扫了 file A），结果只有 "panic at line 1" 一行
    assert_eq!(
        res.scanned_rows, 2,
        "only file A (2 rows) survived tantivy prune; got scanned_rows={}",
        res.scanned_rows
    );
    assert_eq!(res.rows.len(), 1, "only 1 row matches LIKE '%panic%'");
}

/// 关键词搜索的计数与直方图：`MATCH` 改写成的 `ILIKE '%词%'` 由分词索引数出，词落在更长的词里
/// 也算，一行都不扫描；答案与 DataFusion 扫描得到的相同（`AND 1 = 1` 不改变结果，却让语句不再是
/// 能由索引回答的形态）。词可能落在没进词典的长词里时，那个文件照常扫描。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn keyword_counts_come_from_the_index() {
    let (_tmp, engine, stream) = logs_engine_of(&[
        (1_000_000, &["worker panicked", "ok"]),
        (2_000_000, &["Panic in main", "panic again"]),
        (3_000_000, &["all good", "fine"]),
        (
            4_000_000,
            &[
                "digest 9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
                "plain",
            ],
        ),
    ])
    .await;

    for (term, expected, scanned_rows) in [("panic", 3, 0), ("d081", 1, 2), ("zzz", 0, 0)] {
        let sql = format!("SELECT count(*) AS n FROM logs WHERE MATCH(message, '{term}')");

        let res = engine
            .execute(logs_request(&stream, &sql))
            .await
            .expect("query");

        assert_eq!(single_count(&res), expected, "{sql}");
        assert_eq!(res.scanned_rows, scanned_rows, "{sql}");
    }

    let sql = "SELECT (CAST(_timestamp AS BIGINT) - 0) / 1000000 AS __histogram_bucket, \
               COUNT(*) AS __histogram_total FROM \"logs\" \
               WHERE CAST(_timestamp AS BIGINT) >= 0 AND CAST(_timestamp AS BIGINT) < 5000000 \
               AND MATCH(message, 'panic') GROUP BY 1";

    let counted = engine
        .execute(logs_request(&stream, sql))
        .await
        .expect("query");

    let scanned = engine
        .execute(logs_request(
            &stream,
            &sql.replace(" GROUP BY 1", " AND 1 = 1 GROUP BY 1"),
        ))
        .await
        .expect("query");

    assert_eq!(sorted_bars(&counted), vec![vec![1, 1], vec![2, 2]]);
    assert_eq!(sorted_bars(&scanned), sorted_bars(&counted));
    assert_eq!(counted.scanned_rows, 0);
    assert!(scanned.scanned_rows > 0);
}

/// MATCH 按子串执行：`panic` 也命中只写着 `panicked` 的行，含它的文件不能被裁掉；词典里没有、
/// 长词里也没有的文件照常裁掉。大写的词同样按子串、不分大小写地裁剪。
#[tokio::test]
async fn match_finds_the_term_inside_longer_words() {
    let (_tmp, engine, stream) = logs_engine_of(&[
        (1_000_000, &["worker panicked at main.rs", "ok"]),
        (2_000_000, &["all good", "still good"]),
    ])
    .await;

    for term in ["panic", "PANIC"] {
        let res = engine
            .execute(logs_request(
                &stream,
                &format!("SELECT message FROM logs WHERE MATCH(message, '{term}')"),
            ))
            .await
            .expect("query");

        assert_eq!(res.rows.len(), 1, "{term}");
        assert_eq!(
            res.scanned_rows, 2,
            "{term}: the file of good news is pruned"
        );
    }
}

/// 单流 StreamRepository：`get` 恒返回同一个定义，`get_settings` 走 default
/// （queryable=true）。DataFusionEngine 靠它拿 schema 判断字段的索引类型；没有它时
/// 引擎不生成任何裁剪谓词。
struct OneStreamRepo(StreamDefinition);

#[async_trait]
impl StreamRepository for OneStreamRepo {
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

const TRACE_A: &str = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
const TRACE_B: &str = "b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2";
const TRACE_C: &str = "c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3";

/// 3 个主 Artifact 各含两行同一 trace_id（A、B、C），带显式 Tantivy Artifact。
async fn traces_engine() -> (tempfile::TempDir, DataFusionEngine, StreamDefinition) {
    traces_engine_of(&[
        (1_000_000, [TRACE_A, TRACE_A]),
        (2_000_000, [TRACE_B, TRACE_B]),
        (3_000_000, [TRACE_C, TRACE_C]),
    ])
    .await
}

/// 每个主 Artifact 是 `(第一行的时间, 各行的 trace_id)`，每行晚 1 ms；各带显式 Tantivy
/// Artifact。
async fn traces_engine_of<'a>(
    files: &[(i64, impl AsRef<[&'a str]>)],
) -> (tempfile::TempDir, DataFusionEngine, StreamDefinition) {
    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
    let writer = ParquetWriter::new(store.clone());
    let repo: Arc<InMemQueryFile> = Arc::new(InMemQueryFile::default());
    let stream = traces_stream();
    let dataset = indexed_dataset(&stream);

    let mut index_keys = HashMap::new();
    for (start, tids) in files {
        let (meta, (primary, index)) = write_indexed_fixture(
            &writer,
            &stream,
            &dataset,
            build_traces_batch(*start, tids.as_ref()),
        )
        .await;
        repo.insert(meta).await.unwrap();
        index_keys.insert(primary, index);
    }

    let cache: Arc<IndexHandleCache<Arc<IndexHandle>>> =
        Arc::new(IndexHandleCache::new(CacheLayerSettings::new(100, 60)));
    let pruner = Arc::new(TantivyPruner::new(cache, store.clone()));
    let engine = DataFusionEngine::new(repo.clone() as Arc<dyn QueryFileSource>, store.clone())
        .with_streams(Arc::new(OneStreamRepo(stream.clone())) as Arc<dyn StreamRepository>)
        .with_tantivy_pruner(pruner)
        .with_tantivy_index_keys(index_keys);

    (tmp, engine, stream)
}

fn traces_request(stream: &StreamDefinition, sql: &str, window: TimeRange) -> QueryRequest {
    QueryRequest {
        org_id: stream.org_id.clone(),
        language: QueryLanguage::Sql,
        statement: sql.to_owned(),
        time_range: window,
        stream: Some(StreamHint {
            name: "traces".into(),
            stream_type: StreamType::TRACES,
        }),
        limit: None,
        step: None,
        federation_clusters: Vec::new(),
    }
}

fn whole_range() -> TimeRange {
    TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX))
}

fn single_count(result: &molesignal::domain::query::QueryResult) -> i64 {
    assert_eq!(result.rows.len(), 1, "a count returns one row");
    let value = &result.rows[0][0];
    value
        .as_i64()
        .or_else(|| value.as_u64().map(|u| u as i64))
        .unwrap()
}

/// `WHERE trace_id = '<A>'` 对 exact-indexed 字段触发 tantivy 等值裁剪：3 个主
/// Artifact 各含不同 trace_id，只有含目标值的对象被扫描。
#[tokio::test]
async fn equality_predicate_on_exact_field_prunes_files() {
    let (_tmp, engine, stream) = traces_engine().await;

    let res = engine
        .execute(traces_request(
            &stream,
            &format!("SELECT trace_id FROM traces WHERE trace_id = '{TRACE_A}'"),
            whole_range(),
        ))
        .await
        .expect("query");

    // 只有 File A（2 行）被扫描；B/C 被等值裁剪剔除。
    assert_eq!(
        res.scanned_rows, 2,
        "only file A survived equality prune; got scanned_rows={}",
        res.scanned_rows
    );
    assert_eq!(res.rows.len(), 2, "both rows in file A carry trace_id = a");
}

/// 只统计行数的语句：整体落在时间窗内的文件用整值索引的命中数或 Catalog 行数回答，
/// 一行都不扫描。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn counts_of_files_inside_the_window_come_from_the_index() {
    let (_tmp, engine, stream) = traces_engine().await;

    for (sql, expected) in [
        (
            format!("SELECT count(*) AS n FROM traces WHERE trace_id = '{TRACE_A}'"),
            2,
        ),
        ("SELECT count(*) AS n FROM traces".to_owned(), 6),
        (
            format!(
                "SELECT count(1) FROM traces WHERE trace_id = '{TRACE_A}' AND trace_id = '{TRACE_B}'"
            ),
            0,
        ),
    ] {
        let res = engine
            .execute(traces_request(&stream, &sql, whole_range()))
            .await
            .expect("query");

        assert_eq!(single_count(&res), expected, "{sql}");
        assert_eq!(res.scanned_rows, 0, "{sql}");
    }
}

/// 时间窗只覆盖文件的一部分时，该文件照常扫描：File A 的两行在 1_000_000 与 1_001_000，
/// 窗口从 1_000_500 开始，只算第二行。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn counts_scan_the_files_the_window_cuts() {
    let (_tmp, engine, stream) = traces_engine().await;
    let window = TimeRange::new(TimestampMicros(1_000_500), TimestampMicros(i64::MAX));

    let res = engine
        .execute(traces_request(
            &stream,
            &format!("SELECT count(*) AS n FROM traces WHERE trace_id = '{TRACE_A}'"),
            window,
        ))
        .await
        .expect("query");

    assert_eq!(single_count(&res), 1);
    assert_eq!(res.scanned_rows, 2, "file A is scanned, B and C are pruned");

    let res = engine
        .execute(traces_request(
            &stream,
            "SELECT count(*) AS n FROM traces",
            window,
        ))
        .await
        .expect("query");

    assert_eq!(single_count(&res), 5);
    assert_eq!(res.scanned_rows, 2, "only file A is scanned");
}

/// 其它形态的计数（这里是 `OR`）照常扫描，结果不变。
#[tokio::test]
async fn other_counts_scan_as_before() {
    let (_tmp, engine, stream) = traces_engine().await;

    let res = engine
        .execute(traces_request(
            &stream,
            &format!(
                "SELECT count(*) AS n FROM traces WHERE trace_id = '{TRACE_A}' OR trace_id = '{TRACE_B}'"
            ),
            whole_range(),
        ))
        .await
        .expect("query");

    assert_eq!(single_count(&res), 4);
    assert_eq!(res.scanned_rows, 6);
}

/// explorer 直方图的语句：柱宽 1 s、从 0 开始；`errors` 时按 trace_id 是否为 `ERROR`
/// 另计错误行。
fn histogram_sql((from, to): (i64, i64), errors: bool, filters: &str) -> String {
    let errors = if errors {
        ", SUM(CASE WHEN UPPER(CAST(\"trace_id\" AS VARCHAR)) = 'ERROR' THEN 1 ELSE 0 END) AS __histogram_errors"
    } else {
        ""
    };

    format!(
        "SELECT (CAST(_timestamp AS BIGINT) - 0) / 1000000 AS __histogram_bucket, \
         COUNT(*) AS __histogram_total{errors} \
         FROM \"traces\" \
         WHERE CAST(_timestamp AS BIGINT) >= {from} AND CAST(_timestamp AS BIGINT) < {to}{filters} \
         GROUP BY 1"
    )
}

fn sorted_bars(result: &molesignal::domain::query::QueryResult) -> Vec<Vec<i64>> {
    let mut bars = result
        .rows
        .iter()
        .map(|row| row.iter().map(|value| value.as_i64().unwrap()).collect())
        .collect::<Vec<Vec<i64>>>();

    bars.sort();
    bars
}

/// 直方图的柱子从索引里每行的时间数出，一行都不扫描；时间窗切开的文件也一样。答案与
/// DataFusion 扫描得到的相同：`AND 1 = 1` 不改变结果，却让语句不再是能由索引回答的形态。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn histogram_bars_come_from_the_index() {
    let (_tmp, engine, stream) = traces_engine_of(&[
        (1_000_000, ["error", "ok"]),
        (2_000_000, ["ERROR", "Error"]),
        (3_000_000, ["warn", "ok"]),
    ])
    .await;

    for (window, errors, filters, expected) in [
        (
            (0, 4_000_000),
            true,
            "",
            vec![vec![1, 2, 1], vec![2, 2, 2], vec![3, 2, 0]],
        ),
        (
            (1_000_500, 4_000_000),
            true,
            "",
            vec![vec![1, 1, 0], vec![2, 2, 2], vec![3, 2, 0]],
        ),
        (
            (0, 4_000_000),
            false,
            " AND \"trace_id\" = 'ok'",
            vec![vec![1, 1], vec![3, 1]],
        ),
    ] {
        let sql = histogram_sql(window, errors, filters);
        let range = TimeRange::new(TimestampMicros(window.0), TimestampMicros(window.1));

        let counted = engine
            .execute(traces_request(&stream, &sql, range))
            .await
            .expect("query");

        let scanned = engine
            .execute(traces_request(
                &stream,
                &sql.replace(" GROUP BY 1", " AND 1 = 1 GROUP BY 1"),
                range,
            ))
            .await
            .expect("query");

        assert_eq!(sorted_bars(&counted), expected, "{sql}");
        assert_eq!(sorted_bars(&scanned), expected, "{sql}");
        assert_eq!(counted.scanned_rows, 0, "{sql}");
        assert!(scanned.scanned_rows > 0, "{sql}");
    }
}

fn trace_rows(result: &molesignal::domain::query::QueryResult) -> Vec<(String, i64)> {
    result
        .rows
        .iter()
        .map(|row| {
            (
                row[0].as_str().unwrap().to_owned(),
                row[1].as_i64().unwrap(),
            )
        })
        .collect()
}

/// 大文件里按整值等值找出的少数几行只读它们自己：80 000 行的文件每列有四页，两个命中的行
/// 只在其中两页里，扫描行数就是命中的行数，结果与读整个文件相同（`OR` 让条件不再能从索引
/// 定位行）。命中多到可能落在每一页里时照常读整个文件。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_exact_lookup_in_a_large_file_reads_only_the_matching_rows() {
    const COMMON: &str = "d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4";

    let trace_ids = (0..80_000)
        .map(|row| match row {
            3 | 60_001 => TRACE_A.to_owned(),
            row if row % 800 == 0 => COMMON.to_owned(),
            row => format!("{row:032}"),
        })
        .collect::<Vec<_>>();

    let trace_ids = trace_ids.iter().map(String::as_str).collect::<Vec<_>>();
    let (_tmp, engine, stream) = traces_engine_of(&[(1_000_000, trace_ids)]).await;

    let lookup = |condition: String| {
        format!(
            "SELECT trace_id, CAST(_timestamp AS BIGINT) AS t FROM traces WHERE {condition} ORDER BY t"
        )
    };

    let selected = engine
        .execute(traces_request(
            &stream,
            &lookup(format!("trace_id = '{TRACE_A}'")),
            whole_range(),
        ))
        .await
        .expect("query");

    let scanned = engine
        .execute(traces_request(
            &stream,
            &lookup(format!("trace_id = '{TRACE_A}' OR trace_id = '{TRACE_A}'")),
            whole_range(),
        ))
        .await
        .expect("query");

    let expected = vec![
        (TRACE_A.to_owned(), 1_003_000),
        (TRACE_A.to_owned(), 61_001_000),
    ];

    assert_eq!(trace_rows(&selected), expected);
    assert_eq!(trace_rows(&scanned), expected);
    assert_eq!(selected.scanned_rows, 2);
    assert_eq!(scanned.scanned_rows, 80_000);

    let common = engine
        .execute(traces_request(
            &stream,
            &lookup(format!("trace_id = '{COMMON}'")),
            whole_range(),
        ))
        .await
        .expect("query");

    assert_eq!(common.rows.len(), 100);
    assert_eq!(
        common.scanned_rows, 80_000,
        "100 matches are too many to pick"
    );
}

/// 字段分析“最常见的值”的语句：没有其它条件时，整个落在时间窗里的文件从索引词典数出每个值的
/// 行数，不扫描；窗切开的文件（这里是 File A）与带条件的语句照常扫描。答案与 DataFusion 扫描
/// 得到的相同：`AND 1 = 1` 不改变结果，却让语句不再是能由索引回答的形态。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn top_values_come_from_the_index() {
    let (_tmp, engine, stream) = traces_engine_of(&[
        (1_000_000, ["a", "a"]),
        (2_000_000, ["b", "a"]),
        (3_000_000, ["c", ""]),
    ])
    .await;

    for ((from, to), filters, expected, scanned_rows) in [
        ((0, 4_000_000), "", vec![("a", 3), ("b", 1)], 0),
        ((1_000_500, 4_000_000), "", vec![("a", 2), ("b", 1)], 2),
        ((0, 4_000_000), " AND \"trace_id\" = 'b'", vec![("b", 1)], 2),
    ] {
        let sql = format!(
            "SELECT NULLIF(\"trace_id\", '') AS __fs_value, COUNT(*) AS __fs_count FROM \"traces\" \
             WHERE CAST(_timestamp AS BIGINT) >= {from} AND CAST(_timestamp AS BIGINT) < {to}{filters} \
             AND NULLIF(\"trace_id\", '') IS NOT NULL GROUP BY 1 ORDER BY 2 DESC, 1 ASC LIMIT 2"
        );

        let range = TimeRange::new(TimestampMicros(from), TimestampMicros(to));

        let counted = engine
            .execute(traces_request(&stream, &sql, range))
            .await
            .expect("query");

        let scanned = engine
            .execute(traces_request(
                &stream,
                &sql.replace(" GROUP BY 1", " AND 1 = 1 GROUP BY 1"),
                range,
            ))
            .await
            .expect("query");

        let expected = expected
            .into_iter()
            .map(|(value, count)| (value.to_owned(), count))
            .collect::<Vec<_>>();

        assert_eq!(trace_rows(&counted), expected, "{sql}");
        assert_eq!(trace_rows(&scanned), expected, "{sql}");
        assert_eq!(counted.scanned_rows, scanned_rows, "{sql}");
        assert!(scanned.scanned_rows > 0, "{sql}");
    }
}

/// 字段分析的汇总：没有其它条件时，整个落在时间窗里的文件从索引词典数出行数、有值的行数与每个
/// 值，不扫描；窗切开的文件（这里是 File A）照常扫描。答案与 DataFusion 扫描得到的相同。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn field_summaries_come_from_the_index() {
    let (_tmp, engine, stream) = traces_engine_of(&[
        (1_000_000, ["a", "a"]),
        (2_000_000, ["b", "a"]),
        (3_000_000, ["c", ""]),
    ])
    .await;

    for ((from, to), expected, scanned_rows) in [
        ((0, 4_000_000), vec![6, 5, 3], 0),
        ((1_000_500, 4_000_000), vec![5, 4, 3], 2),
    ] {
        let sql = format!(
            "SELECT COUNT(*) AS __fs_total, COUNT(NULLIF(\"trace_id\", '')) AS __fs_present, \
             approx_distinct(NULLIF(\"trace_id\", '')) AS __fs_distinct FROM \"traces\" \
             WHERE CAST(_timestamp AS BIGINT) >= {from} AND CAST(_timestamp AS BIGINT) < {to}"
        );

        let range = TimeRange::new(TimestampMicros(from), TimestampMicros(to));

        let counted = engine
            .execute(traces_request(&stream, &sql, range))
            .await
            .expect("query");

        let scanned = engine
            .execute(traces_request(&stream, &format!("{sql} AND 1 = 1"), range))
            .await
            .expect("query");

        assert_eq!(
            counted.columns,
            ["__fs_total", "__fs_present", "__fs_distinct"],
            "{sql}"
        );
        assert_eq!(sorted_bars(&counted), vec![expected.clone()], "{sql}");
        assert_eq!(sorted_bars(&scanned), vec![expected], "{sql}");
        assert_eq!(counted.scanned_rows, scanned_rows, "{sql}");
        assert!(scanned.scanned_rows > 0, "{sql}");
    }
}

/// 按时间取一页、带整值等值条件：文件到要读时才剪枝，剪掉没有 A 的文件后只读最新的那个含 A 的
/// 文件就凑满了一页。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_page_reads_only_the_newest_matching_file() {
    let (_tmp, engine, stream) = traces_engine_of(&[
        (1_000_000, [TRACE_A, TRACE_A]),
        (2_000_000, [TRACE_B, TRACE_B]),
        (3_000_000, [TRACE_A, TRACE_C]),
    ])
    .await;

    let res = engine
        .execute(traces_request(
            &stream,
            &format!(
                "SELECT trace_id, CAST(_timestamp AS BIGINT) AS t, _timestamp FROM traces \
                 WHERE trace_id = '{TRACE_A}' ORDER BY _timestamp DESC LIMIT 1"
            ),
            whole_range(),
        ))
        .await
        .expect("query");

    assert_eq!(res.rows.len(), 1);
    assert_eq!(res.rows[0][1].as_i64(), Some(3_000_000));
    assert_eq!(
        res.scanned_rows, 2,
        "only the newest file holding the trace is read"
    );
}
