// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Object-storage I/O benchmark for the SQL query path.
//!
//! Writes the same rows once as many small files and once as a few large ones, then runs
//! typical explorer queries through the production read stack (range-block cache, Tantivy
//! pruner with its caches, DataFusion) over a store that counts every request and delays it
//! like a remote object store.
//!
//! `QUERY_IO_BENCH=1 cargo test --test perf_query_io -- --nocapture`
//!
//! Optional: `QUERY_IO_ROWS` (total rows, default 960000), `QUERY_IO_LATENCY_MS` (per request,
//! default 30), `QUERY_IO_MBPS` (per request transfer rate, default 100), `QUERY_IO_LAYOUT`
//! (run only the layout whose name contains it, such as `12 files`), `QUERY_IO_QUERY` (run only
//! the queries whose name contains one of these comma-separated words, such as `keyword,summary`),
//! `QUERY_IO_TRACE` (print every request of the cold runs without a disk cache).
//!
//! The same switch runs a PromQL benchmark over the hourly files of a metrics stream holding the
//! measured metric and `PROMQL_IO_OTHER_METRICS` others (default 3) of the same shape
//! (`PROMQL_IO_HOURS`, default 24): `cargo test --test perf_query_io promql -- --nocapture`.

use std::{
    collections::HashMap,
    fmt,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use arrow::{
    array::{Float64Array, Int64Array, RecordBatch, StringArray, TimestampMicrosecondArray},
    datatypes::{DataType, Field, Schema as ArrowSchema, TimeUnit},
};
use async_trait::async_trait;
use futures::{StreamExt, stream::BoxStream};
use molesignal::{
    config::{
        CacheLayerSettings, ObjectCacheSettings, StreamAggCacheSettings,
        TantivyFooterCacheSettings, TantivyResultCacheSettings,
    },
    domain::{
        metrics::{MemoryMetricDirectory, MetricDirectory, MetricObservation},
        query::{PromqlEngine, QueryEngine, QueryLanguage, QueryRequest, StreamHint},
        storage::{
            ArtifactRole, ArtifactState, DatasetState, IndexPolicy, IndexTypeId, PartitionPolicy,
            PhysicalDataset, PhysicalDatasetId, QueryFile, QueryFileSource, StoragePolicy,
            primary_dataset_type, type_id,
        },
        stream::{
            FieldDef, FieldType, Retention, Schema, StreamDefinition, StreamRepository, StreamType,
        },
    },
    infra::{
        caching::{IndexHandleCache, StreamingAggCache, TantivyFooterCache, TantivyResultCache},
        query::{
            parquet_table::{ParquetMetadataCache, parquet_metadata_cache},
            promql::PromQLEngine,
            tantivy_pruner::TantivyPruner,
        },
        search::{datafusion_engine::DataFusionEngine, tantivy_index::IndexHandle},
        storage::{
            object_reader::ObjectReader,
            parquet::{
                reader::{ParquetReader, ReadOptions},
                writer::ParquetWriter,
            },
        },
    },
    shared::{
        Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};
use object_store::{
    CopyOptions, GetOptions, GetResult, GetResultPayload, ListResult, MultipartUpload, ObjectMeta,
    ObjectStore, PutMultipartOptions, PutOptions, PutPayload, PutResult, memory::InMemory,
    path::Path,
};

const HOUR_MICROS: i64 = 3_600_000_000;
const BASE_MICROS: i64 = 1_767_225_600_000_000;
const RARE_PATH: &str = "/api/bar";

const QUERIES: [(&str, &str); 3] = [
    (
        "count, filtered",
        "SELECT count(*) AS cnt FROM requests WHERE path = '/api/bar'",
    ),
    (
        "histogram, filtered",
        "SELECT date_bin(INTERVAL '1 hour', _timestamp) AS t, count(*) AS cnt FROM requests \
         WHERE path = '/api/bar' GROUP BY t ORDER BY t",
    ),
    (
        "histogram, all rows",
        "SELECT date_bin(INTERVAL '1 hour', _timestamp) AS t, count(*) AS cnt FROM requests \
         GROUP BY t ORDER BY t",
    ),
];

/// The queries above, the newest page of rows, then the explorer's histogram statements in
/// hourly bars and the field analysis' most common values. Each of the last three kinds also runs
/// in a twin with the same answer the way it ran before: the page wrapped in a subquery reads
/// every file at once instead of the newest first (its condition repeated outside, so the files
/// are still pruned by the index), and `AND 1 = 1` keeps a histogram's or a top values
/// statement's answer but not its shape, so it is scanned instead of counted from the index.
fn queries() -> Vec<(String, String)> {
    let mut queries = QUERIES
        .iter()
        .map(|(name, sql)| ((*name).to_owned(), (*sql).to_owned()))
        .collect::<Vec<_>>();

    for (name, filters) in [
        ("latest 100, all rows", ""),
        ("latest 100, filtered", " WHERE path = '/api/bar'"),
    ] {
        // The last column is a number, so the answers of the two runs can be compared.
        let sql = format!(
            "SELECT _timestamp, path, status, message, CAST(_timestamp AS BIGINT) AS t \
             FROM requests{filters} ORDER BY _timestamp DESC LIMIT 100"
        );

        queries.push((
            format!("{name}, every file"),
            format!("SELECT * FROM ({sql}) AS page{filters}"),
        ));

        queries.push((name.to_owned(), sql));
    }

    for (name, errors, filters) in [
        ("bars, all rows", false, ""),
        ("bars by level", true, ""),
        ("bars, filtered", false, " AND \"path\" = '/api/bar'"),
    ] {
        let sql = bars_sql(errors, filters);

        queries.push((
            format!("{name}, scanned"),
            sql.replace(" GROUP BY 1", " AND 1 = 1 GROUP BY 1"),
        ));

        queries.push((name.to_owned(), sql));
    }

    for (name, filters) in [
        ("top paths", ""),
        ("top paths of errors", " AND \"level\" = 'error'"),
    ] {
        let sql = top_paths_sql(filters);

        queries.push((
            format!("{name}, scanned"),
            sql.replace(" GROUP BY 1", " AND 1 = 1 GROUP BY 1"),
        ));

        queries.push((name.to_owned(), sql));
    }

    // Keyword searches as the explorer writes them: MATCH runs as `ILIKE '%term%'`, counted from
    // the full-text index of `message`. `u4321` is one user in five thousand, `delete` is in a
    // quarter of the rows.
    let count = "SELECT count(*) AS cnt FROM requests WHERE MATCH(message, 'u4321')";
    queries.push((
        "count, keyword, scanned".to_owned(),
        format!("{count} AND 1 = 1"),
    ));
    queries.push(("count, keyword".to_owned(), count.to_owned()));

    for (name, word) in [("bars, keyword", "u4321"), ("bars, common word", "delete")] {
        let sql = bars_sql(false, &format!(" AND MATCH(message, '{word}')"));

        queries.push((
            format!("{name}, scanned"),
            sql.replace(" GROUP BY 1", " AND 1 = 1 GROUP BY 1"),
        ));

        queries.push((name.to_owned(), sql));
    }

    // The field analysis' summary of a field: rows, rows with a value, and different values.
    let summary = summary_sql("path");
    queries.push((
        "path summary, scanned".to_owned(),
        format!("{summary} AND 1 = 1"),
    ));
    queries.push(("path summary".to_owned(), summary));

    queries
}

/// The statement the field analysis sends for the summary of a text field, over the whole range.
fn summary_sql(field: &str) -> String {
    let (from, to) = (BASE_MICROS, BASE_MICROS + 1000 * HOUR_MICROS);

    format!(
        "SELECT COUNT(*) AS __fs_total, COUNT(NULLIF(\"{field}\", '')) AS __fs_present, \
         approx_distinct(NULLIF(\"{field}\", '')) AS __fs_distinct FROM \"requests\" \
         WHERE CAST(_timestamp AS BIGINT) >= {from} AND CAST(_timestamp AS BIGINT) < {to}"
    )
}

/// The statement the field analysis sends for the most common paths, over the whole range.
fn top_paths_sql(filters: &str) -> String {
    let (from, to) = (BASE_MICROS, BASE_MICROS + 1000 * HOUR_MICROS);

    format!(
        "SELECT NULLIF(\"path\", '') AS __fs_value, COUNT(*) AS __fs_count FROM \"requests\" \
         WHERE CAST(_timestamp AS BIGINT) >= {from} AND CAST(_timestamp AS BIGINT) < {to}{filters} \
         AND NULLIF(\"path\", '') IS NOT NULL GROUP BY 1 ORDER BY 2 DESC, 1 ASC LIMIT 10"
    )
}

/// The statement the explorer histogram sends, over the whole request range.
fn bars_sql(errors: bool, filters: &str) -> String {
    let (from, to) = (BASE_MICROS, BASE_MICROS + 1000 * HOUR_MICROS);

    let errors = if errors {
        ", SUM(CASE WHEN UPPER(CAST(\"level\" AS VARCHAR)) = 'ERROR' THEN 1 ELSE 0 END) \
         AS __histogram_errors"
    } else {
        ""
    };

    format!(
        "SELECT (CAST(_timestamp AS BIGINT) - {from}) / {HOUR_MICROS} AS __histogram_bucket, \
         COUNT(*) AS __histogram_total{errors} FROM \"requests\" \
         WHERE CAST(_timestamp AS BIGINT) >= {from} AND CAST(_timestamp AS BIGINT) < {to}{filters} \
         GROUP BY 1"
    )
}

/// Counts requests and bytes, and delays each one by a fixed latency plus transfer time.
struct SlowStore {
    inner: Arc<dyn ObjectStore>,
    active: AtomicBool,
    trace: AtomicBool,
    latency: Duration,
    bytes_per_sec: f64,
    gets: AtomicU64,
    heads: AtomicU64,
    bytes: AtomicU64,
}

impl SlowStore {
    fn new(latency: Duration, bytes_per_sec: f64) -> Self {
        Self {
            inner: Arc::new(InMemory::new()),
            active: AtomicBool::new(false),
            trace: AtomicBool::new(false),
            latency,
            bytes_per_sec,
            gets: AtomicU64::new(0),
            heads: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
        }
    }

    fn start(&self, delay: bool) {
        self.gets.store(0, Ordering::SeqCst);
        self.heads.store(0, Ordering::SeqCst);
        self.bytes.store(0, Ordering::SeqCst);
        self.active.store(delay, Ordering::SeqCst);
    }

    fn counts(&self) -> (u64, u64, u64) {
        (
            self.gets.load(Ordering::SeqCst),
            self.heads.load(Ordering::SeqCst),
            self.bytes.load(Ordering::SeqCst),
        )
    }
}

impl fmt::Debug for SlowStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SlowStore")
    }
}

impl fmt::Display for SlowStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SlowStore")
    }
}

#[async_trait]
impl ObjectStore for SlowStore {
    async fn put_opts(
        &self,
        location: &Path,
        payload: PutPayload,
        options: PutOptions,
    ) -> object_store::Result<PutResult> {
        self.inner.put_opts(location, payload, options).await
    }

    async fn put_multipart_opts(
        &self,
        location: &Path,
        options: PutMultipartOptions,
    ) -> object_store::Result<Box<dyn MultipartUpload>> {
        self.inner.put_multipart_opts(location, options).await
    }

    async fn get_opts(
        &self,
        location: &Path,
        options: GetOptions,
    ) -> object_store::Result<GetResult> {
        let head = options.head;
        let result = self.inner.get_opts(location, options).await?;

        if head {
            self.heads.fetch_add(1, Ordering::SeqCst);
        } else {
            self.gets.fetch_add(1, Ordering::SeqCst);
        }

        if head {
            if self.active.load(Ordering::SeqCst) {
                tokio::time::sleep(self.latency).await;
            }

            return Ok(result);
        }

        let meta = result.meta.clone();
        let range = result.range.clone();
        let attributes = result.attributes.clone();
        let bytes = result.bytes().await?;
        self.bytes.fetch_add(bytes.len() as u64, Ordering::SeqCst);

        if self.trace.load(Ordering::SeqCst) {
            let key = location.as_ref();
            let tail = &key[key.len().saturating_sub(24)..];
            eprintln!(
                "GET …{tail} {range:?} of {} ({} bytes)",
                meta.size,
                bytes.len()
            );
        }

        if self.active.load(Ordering::SeqCst) {
            let transfer = Duration::from_secs_f64(bytes.len() as f64 / self.bytes_per_sec);
            tokio::time::sleep(self.latency + transfer).await;
        }

        Ok(GetResult {
            payload: GetResultPayload::Stream(
                futures::stream::once(async move { Ok(bytes) }).boxed(),
            ),
            meta,
            range,
            attributes,
        })
    }

    fn delete_stream(
        &self,
        locations: BoxStream<'static, object_store::Result<Path>>,
    ) -> BoxStream<'static, object_store::Result<Path>> {
        self.inner.delete_stream(locations)
    }

    fn list(&self, prefix: Option<&Path>) -> BoxStream<'static, object_store::Result<ObjectMeta>> {
        self.inner.list(prefix)
    }

    async fn list_with_delimiter(&self, prefix: Option<&Path>) -> object_store::Result<ListResult> {
        self.inner.list_with_delimiter(prefix).await
    }

    async fn copy_opts(
        &self,
        from: &Path,
        to: &Path,
        options: CopyOptions,
    ) -> object_store::Result<()> {
        self.inner.copy_opts(from, to, options).await
    }
}

#[derive(Default)]
struct Files {
    inner: StdMutex<Vec<QueryFile>>,
}

#[async_trait]
impl QueryFileSource for Files {
    async fn find(
        &self,
        org: &Id,
        stream: &str,
        stream_type: StreamType,
        range: TimeRange,
    ) -> Result<Vec<QueryFile>> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .iter()
            .filter(|file| {
                &file.org_id == org
                    && file.stream == stream
                    && file.stream_type == stream_type
                    && file.time_range.end.0 >= range.start.0
                    && file.time_range.start.0 <= range.end.0
            })
            .cloned()
            .collect())
    }
}

struct OneStream(StreamDefinition);

#[async_trait]
impl StreamRepository for OneStream {
    async fn create(&self, definition: StreamDefinition) -> Result<StreamDefinition> {
        Ok(definition)
    }

    async fn update_schema(&self, _id: &Id, _schema: Schema) -> Result<()> {
        Ok(())
    }

    async fn get(
        &self,
        _org: &Id,
        _name: &str,
        _stream_type: StreamType,
    ) -> Result<StreamDefinition> {
        Ok(self.0.clone())
    }

    async fn list(&self, _org: &Id) -> Result<Vec<StreamDefinition>> {
        Ok(vec![self.0.clone()])
    }

    async fn delete(&self, _id: &Id) -> Result<()> {
        Ok(())
    }
}

fn field(name: &str, data_type: FieldType, indexed: bool, exact: bool) -> FieldDef {
    FieldDef {
        name: name.into(),
        data_type,
        nullable: false,
        index_type: None,
        indexed,
        encrypted: false,
        exact,
    }
}

fn requests_stream() -> StreamDefinition {
    StreamDefinition {
        id: Id::new(),
        org_id: Id::from_string("bench"),
        name: "requests".into(),
        stream_type: StreamType::LOGS,
        schema: Schema {
            fields: vec![
                field("path", FieldType::Utf8, true, true),
                field("method", FieldType::Utf8, true, true),
                field("level", FieldType::Utf8, true, true),
                field("status", FieldType::Int64, false, false),
                field("duration_ms", FieldType::Float64, false, false),
                field("message", FieldType::Utf8, true, false),
            ],
        },
        retention: Some(Retention { days: 30 }),
        created_at: TimestampMicros::now(),
        updated_at: TimestampMicros::now(),
    }
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

/// xorshift64*: deterministic rows without a random-number dependency.
struct Rows(u64);

impl Rows {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
}

/// One file's rows, all inside the hour that starts at `hour_start`.
fn batch(rows: &mut Rows, hour_start: i64, count: usize) -> RecordBatch {
    let methods = ["GET", "POST", "PUT", "DELETE"];
    let mut timestamps = Vec::with_capacity(count);
    let mut paths = Vec::with_capacity(count);
    let mut verbs = Vec::with_capacity(count);
    let mut levels = Vec::with_capacity(count);
    let mut statuses = Vec::with_capacity(count);
    let mut durations = Vec::with_capacity(count);
    let mut messages = Vec::with_capacity(count);

    for _ in 0..count {
        let roll = rows.next();
        let path = if roll % 1000 < 2 {
            RARE_PATH.to_owned()
        } else {
            let rank = (roll >> 10) % 100;
            format!("/api/p{}", rank * rank / 100)
        };

        let verb = methods[((roll >> 20) % 4) as usize];
        let status = match (roll >> 24) % 100 {
            0..=1 => 500,
            2..=6 => 404,
            _ => 200,
        };

        let duration = ((roll >> 32) % 5000) as f64 / 10.0;

        messages.push(format!(
            "{verb} {path} {status} {duration}ms request_id={:016x} user=u{}",
            rows.next(),
            (roll >> 40) % 5000
        ));

        timestamps.push(hour_start + ((roll >> 8) % HOUR_MICROS as u64) as i64);
        paths.push(path);
        verbs.push(verb);
        levels.push(match status {
            500 => "error",
            404 => "warn",
            _ => "info",
        });
        statuses.push(status);
        durations.push(duration);
    }

    let schema = Arc::new(ArrowSchema::new(vec![
        Field::new(
            "_timestamp",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        ),
        Field::new("path", DataType::Utf8, false),
        Field::new("method", DataType::Utf8, false),
        Field::new("level", DataType::Utf8, false),
        Field::new("status", DataType::Int64, false),
        Field::new("duration_ms", DataType::Float64, false),
        Field::new("message", DataType::Utf8, false),
    ]));

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(TimestampMicrosecondArray::from(timestamps).with_timezone("UTC")),
            Arc::new(StringArray::from(paths)),
            Arc::new(StringArray::from(verbs)),
            Arc::new(StringArray::from(levels)),
            Arc::new(Int64Array::from(statuses)),
            Arc::new(Float64Array::from(durations)),
            Arc::new(StringArray::from(messages)),
        ],
    )
    .unwrap()
}

struct Layout {
    name: String,
    store: Arc<SlowStore>,
    stream: StreamDefinition,
    files: Arc<Files>,
    index_keys: HashMap<String, String>,
    segments: Vec<molesignal::domain::storage::DataSegment>,
    bytes: u64,
}

async fn write_layout(
    name: &str,
    file_count: usize,
    rows_per_file: usize,
    store: Arc<SlowStore>,
) -> Layout {
    let stream = requests_stream();
    let dataset = indexed_dataset(&stream);
    let writer = ParquetWriter::new(store.clone() as Arc<dyn ObjectStore>);
    let files = Arc::new(Files::default());
    let mut index_keys = HashMap::new();
    let mut segments = Vec::new();
    let mut rows = Rows(0x9e37_79b9_7f4a_7c15);
    let mut bytes = 0;

    for hour in 0..file_count {
        let hour_start = BASE_MICROS + hour as i64 * HOUR_MICROS;

        let mut written = writer
            .write_compaction_catalog(
                &stream,
                &dataset,
                batch(&mut rows, hour_start, rows_per_file),
            )
            .await
            .unwrap();

        assert_eq!(written.len(), 1, "one hour of rows is one segment");
        let segment = written.remove(0);

        let index = segment
            .auxiliaries
            .iter()
            .find(|artifact| {
                artifact.role == ArtifactRole::Index
                    && artifact.state == ArtifactState::Ready
                    && artifact.artifact_type.as_str() == type_id::builtin::ARTIFACT_TANTIVY
            })
            .expect("segment has a Tantivy Artifact");

        bytes += segment.primary.object.size_bytes + index.object.size_bytes;
        index_keys.insert(
            segment.primary.object.key.as_str().to_owned(),
            index.object.key.as_str().to_owned(),
        );

        files.inner.lock().unwrap().push(QueryFile {
            id: segment.id.0.clone(),
            org_id: stream.org_id.clone(),
            stream: stream.name.clone(),
            stream_type: stream.stream_type,
            dataset_type: primary_dataset_type(stream.stream_type).unwrap(),
            object_key: segment.primary.object.key.as_str().to_owned(),
            checksum: Some(segment.primary.object.checksum.clone()),
            etag: segment.primary.object.etag.clone(),
            time_range: segment.time_range,
            rows: segment.row_count,
            size_bytes: segment.primary.object.size_bytes,
            min_values: segment.column_stats.min_values.clone(),
            max_values: segment.column_stats.max_values.clone(),
            value_sets: segment.column_stats.value_sets.clone(),
            address_ranges: segment.column_stats.address_ranges.clone(),
            null_counts: segment.column_stats.null_counts.clone(),
            indexed_at_micros: segment.column_stats.indexed_at_micros,
            shard: None,
        });

        segments.push(segment);
    }

    Layout {
        name: name.to_owned(),
        store,
        stream,
        files,
        index_keys,
        segments,
        bytes,
    }
}

/// The query-side caches that live across queries in one process.
struct ProcessCaches {
    handles: Arc<IndexHandleCache<Arc<IndexHandle>>>,
    footers: Arc<TantivyFooterCache>,
    results: Arc<TantivyResultCache>,
    parquet: Option<ParquetMetadataCache>,
}

impl ProcessCaches {
    fn new() -> Self {
        Self {
            handles: Arc::new(IndexHandleCache::new(CacheLayerSettings::new(10_000, 600))),
            footers: Arc::new(TantivyFooterCache::new(&TantivyFooterCacheSettings {
                capacity: 100_000,
                ttl_secs: 3600,
            })),
            results: Arc::new(TantivyResultCache::new(&TantivyResultCacheSettings {
                capacity: 1_000_000,
                ttl_secs: 600,
            })),
            parquet: parquet_metadata_cache(256 * 1024 * 1024),
        }
    }
}

fn engine(
    layout: &Layout,
    read_store: Arc<dyn ObjectStore>,
    caches: &ProcessCaches,
) -> DataFusionEngine {
    let pruner = TantivyPruner::new(caches.handles.clone(), read_store.clone())
        .with_result_cache(caches.results.clone())
        .with_footer_cache(caches.footers.clone());

    DataFusionEngine::new(layout.files.clone() as Arc<dyn QueryFileSource>, read_store)
        .with_streams(Arc::new(OneStream(layout.stream.clone())) as Arc<dyn StreamRepository>)
        .with_tantivy_pruner(Arc::new(pruner))
        .with_tantivy_index_keys(layout.index_keys.clone())
        .with_parquet_metadata_cache(caches.parquet.clone())
}

fn read_store(layout: &Layout, cache_root: Option<&std::path::Path>) -> Arc<dyn ObjectStore> {
    let origin = layout.store.clone() as Arc<dyn ObjectStore>;

    let Some(root) = cache_root else {
        return origin;
    };

    let settings = ObjectCacheSettings {
        root: root.to_path_buf(),
        ..ObjectCacheSettings::default()
    };

    let reader = ObjectReader::build(origin, "s3", &settings).unwrap();

    for segment in &layout.segments {
        reader
            .register_segment(&layout.stream.org_id, segment)
            .unwrap();
    }

    reader.store()
}

struct Sample {
    gets: u64,
    heads: u64,
    bytes: u64,
    millis: u128,
    rows: usize,
    answer: String,
}

async fn run(layout: &Layout, engine: &DataFusionEngine, sql: &str, delay: bool) -> Sample {
    run_traced(layout, engine, sql, delay, false).await
}

async fn run_traced(
    layout: &Layout,
    engine: &DataFusionEngine,
    sql: &str,
    delay: bool,
    trace: bool,
) -> Sample {
    let request = QueryRequest {
        org_id: layout.stream.org_id.clone(),
        language: QueryLanguage::Sql,
        statement: sql.to_owned(),
        time_range: TimeRange::new(
            TimestampMicros(BASE_MICROS),
            TimestampMicros(BASE_MICROS + 1000 * HOUR_MICROS),
        ),
        stream: Some(StreamHint {
            name: layout.stream.name.clone(),
            stream_type: layout.stream.stream_type,
        }),
        limit: None,
        step: None,
        federation_clusters: Vec::new(),
    };

    layout.store.start(delay);
    layout.store.trace.store(trace, Ordering::SeqCst);
    let started = Instant::now();
    let result = engine.execute(request).await.expect("query");
    let millis = started.elapsed().as_millis();
    let (gets, heads, bytes) = layout.store.counts();
    layout.store.trace.store(false, Ordering::SeqCst);
    layout.store.start(false);

    // Many rows are told apart by the sum of their last column, when it is a number.
    let last_sum = result
        .rows
        .iter()
        .map(|row| row.last().and_then(serde_json::Value::as_i64))
        .sum::<Option<i64>>();

    let answer = match (result.rows.first().and_then(|row| row.last()), last_sum) {
        (Some(value), _) if result.rows.len() == 1 => value.to_string(),
        (_, Some(sum)) => format!("{} rows, sum {sum}", result.rows.len()),
        _ => format!("{} rows", result.rows.len()),
    };

    Sample {
        gets,
        heads,
        bytes,
        millis,
        rows: result.rows.len(),
        answer,
    }
}

fn mib(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / (1024.0 * 1024.0))
}

fn report(layout: &Layout, scenario: &str, query: &str, sample: &Sample) {
    println!(
        "| {} | {} | {} | {} | {} | {} | {} | {} |",
        layout.name,
        scenario,
        query,
        sample.gets,
        sample.heads,
        mib(sample.bytes),
        sample.millis,
        sample.answer,
    );

    assert!(sample.rows > 0, "{query} returned no rows");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn query_io_by_file_layout() {
    if std::env::var("QUERY_IO_BENCH").is_err() {
        eprintln!("query I/O benchmark skipped: QUERY_IO_BENCH is not set");
        return;
    }

    let env = |name: &str, default: u64| {
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(default)
    };

    let total_rows = env("QUERY_IO_ROWS", 960_000) as usize;
    let only_layout = std::env::var("QUERY_IO_LAYOUT").ok();
    let trace = std::env::var("QUERY_IO_TRACE").is_ok();
    let latency = Duration::from_millis(env("QUERY_IO_LATENCY_MS", 30));
    let bytes_per_sec = env("QUERY_IO_MBPS", 100) as f64 * 1024.0 * 1024.0;

    let mut layouts = Vec::new();

    for (name, file_count) in [("96 files", 96), ("12 files", 12)] {
        if only_layout
            .as_deref()
            .is_some_and(|only| !name.contains(only))
        {
            continue;
        }

        let store = Arc::new(SlowStore::new(latency, bytes_per_sec));
        let started = Instant::now();
        let layout = write_layout(name, file_count, total_rows / file_count, store).await;

        eprintln!(
            "{name}: {} MiB of Parquet and Tantivy written in {:.1}s",
            mib(layout.bytes),
            started.elapsed().as_secs_f64()
        );

        layouts.push(layout);
    }

    println!(
        "\nrows {total_rows}, latency {} ms, {} MiB/s per request\n",
        latency.as_millis(),
        bytes_per_sec / (1024.0 * 1024.0)
    );
    println!("| layout | scenario | query | GET | HEAD | MiB read | ms | answer |");
    println!("|---|---|---|---|---|---|---|---|");

    let only_queries = std::env::var("QUERY_IO_QUERY").ok();

    let queries = queries()
        .into_iter()
        .filter(|(name, _)| {
            only_queries
                .as_deref()
                .is_none_or(|only| only.split(',').any(|word| name.contains(word.trim())))
        })
        .collect::<Vec<_>>();

    for layout in &layouts {
        // No latency: what the query costs without the network.
        for (query, sql) in &queries {
            let caches = ProcessCaches::new();
            let engine = engine(layout, read_store(layout, None), &caches);
            let sample = run(layout, &engine, sql, false).await;
            report(layout, "cpu only", query, &sample);
        }

        // Every read goes to the remote store; each query starts with empty process caches.
        for (query, sql) in &queries {
            let caches = ProcessCaches::new();
            let engine = engine(layout, read_store(layout, None), &caches);

            if trace {
                eprintln!("-- {} / {query}", layout.name);
            }

            let sample = run_traced(layout, &engine, sql, true, trace).await;
            report(layout, "no disk cache, cold", query, &sample);
        }

        // Same engine and caches for the second round: process caches warm, disk cache off.
        let caches = ProcessCaches::new();
        let warm_engine = engine(layout, read_store(layout, None), &caches);

        for (_, sql) in &queries {
            run(layout, &warm_engine, sql, false).await;
        }

        for (query, sql) in &queries {
            let sample = run(layout, &warm_engine, sql, true).await;
            report(layout, "no disk cache, process caches warm", query, &sample);
        }

        // The production default: remote reads through the range-block disk cache.
        for (query, sql) in &queries {
            let root = tempfile::tempdir().unwrap();
            let caches = ProcessCaches::new();
            let engine = engine(layout, read_store(layout, Some(root.path())), &caches);
            let sample = run(layout, &engine, sql, true).await;
            report(layout, "disk cache, cold", query, &sample);
        }

        let root = tempfile::tempdir().unwrap();
        let caches = ProcessCaches::new();
        let warm_engine = engine(layout, read_store(layout, Some(root.path())), &caches);

        for (_, sql) in &queries {
            run(layout, &warm_engine, sql, false).await;
        }

        for (query, sql) in &queries {
            let sample = run(layout, &warm_engine, sql, true).await;
            report(layout, "disk cache, warm", query, &sample);
        }
    }
}

const METRIC: &str = "http_requests_total";
/// The metrics stream: one source holding `METRIC` and the other metrics.
const SOURCE: &str = "bench";
const SERVICES: usize = 8;
const INSTANCES: usize = 25;
const METHODS: [&str; 4] = ["GET", "POST", "PUT", "DELETE"];
const CODES: [&str; 2] = ["200", "500"];
const MINUTE_MICROS: i64 = 60_000_000;

fn metrics_stream() -> StreamDefinition {
    StreamDefinition {
        id: Id::new(),
        org_id: Id::from_string("bench"),
        name: SOURCE.into(),
        stream_type: StreamType::METRICS,
        schema: Schema {
            fields: vec![
                field("value", FieldType::Float64, false, false),
                field("metric_name", FieldType::Utf8, false, true),
                field("service", FieldType::Utf8, false, false),
                field("instance", FieldType::Utf8, false, false),
                field("method", FieldType::Utf8, false, false),
                field("code", FieldType::Utf8, false, false),
            ],
        },
        retention: Some(Retention { days: 30 }),
        created_at: TimestampMicros::now(),
        updated_at: TimestampMicros::now(),
    }
}

/// The metrics of the stream: the measured one and the others.
fn metric_names(others: usize) -> Vec<String> {
    std::iter::once(METRIC.to_string())
        .chain((0..others).map(|other| format!("other_{other}_total")))
        .collect()
}

/// One hour of counters sampled every minute for each of the 1600 series of every metric.
fn metric_batch(hour: usize, hour_start: i64, metrics: &[String]) -> RecordBatch {
    let mut timestamps = Vec::new();
    let mut names = Vec::new();
    let mut values = Vec::new();
    let mut services = Vec::new();
    let mut instances = Vec::new();
    let mut methods = Vec::new();
    let mut codes = Vec::new();

    for metric in metrics {
        for minute in 0..60 {
            let at = hour_start + minute as i64 * MINUTE_MICROS;

            for service in 0..SERVICES {
                for instance in 0..INSTANCES {
                    for (method_index, method) in METHODS.iter().enumerate() {
                        for code in CODES {
                            let series =
                                (service * INSTANCES + instance) * METHODS.len() + method_index;

                            timestamps.push(at);
                            names.push(metric.as_str());
                            values.push((hour * 60 + minute) as f64 * (1 + series % 7) as f64);
                            services.push(format!("svc-{service}"));
                            instances.push(format!("10.0.{service}.{instance}:9090"));
                            methods.push(*method);
                            codes.push(code);
                        }
                    }
                }
            }
        }
    }

    let schema = Arc::new(ArrowSchema::new(vec![
        Field::new(
            "_timestamp",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        ),
        Field::new("value", DataType::Float64, false),
        Field::new("metric_name", DataType::Utf8, false),
        Field::new("service", DataType::Utf8, false),
        Field::new("instance", DataType::Utf8, false),
        Field::new("method", DataType::Utf8, false),
        Field::new("code", DataType::Utf8, false),
    ]));

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(TimestampMicrosecondArray::from(timestamps).with_timezone("UTC")),
            Arc::new(Float64Array::from(values)),
            Arc::new(StringArray::from(names)),
            Arc::new(StringArray::from(services)),
            Arc::new(StringArray::from(instances)),
            Arc::new(StringArray::from(methods)),
            Arc::new(StringArray::from(codes)),
        ],
    )
    .unwrap()
}

struct Metrics {
    stream: StreamDefinition,
    files: Arc<Files>,
    directory: Arc<MemoryMetricDirectory>,
}

async fn write_metrics(store: Arc<SlowStore>, hours: usize, others: usize) -> Metrics {
    let stream = metrics_stream();
    let names = metric_names(others);

    let dataset = PhysicalDataset {
        index_policy: IndexPolicy {
            indexers: Vec::new(),
        },
        ..indexed_dataset(&stream)
    };

    let writer = ParquetWriter::new(store as Arc<dyn ObjectStore>);
    let files = Arc::new(Files::default());

    for hour in 0..hours {
        let hour_start = BASE_MICROS + hour as i64 * HOUR_MICROS;

        let mut written = writer
            .write_compaction_catalog(&stream, &dataset, metric_batch(hour, hour_start, &names))
            .await
            .unwrap();

        assert_eq!(written.len(), 1, "one hour of samples is one segment");
        let segment = written.remove(0);

        files.inner.lock().unwrap().push(QueryFile {
            id: segment.id.0.clone(),
            org_id: stream.org_id.clone(),
            stream: stream.name.clone(),
            stream_type: stream.stream_type,
            dataset_type: primary_dataset_type(stream.stream_type).unwrap(),
            object_key: segment.primary.object.key.as_str().to_owned(),
            checksum: Some(segment.primary.object.checksum.clone()),
            etag: segment.primary.object.etag.clone(),
            time_range: segment.time_range,
            rows: segment.row_count,
            size_bytes: segment.primary.object.size_bytes,
            min_values: segment.column_stats.min_values.clone(),
            max_values: segment.column_stats.max_values.clone(),
            value_sets: segment.column_stats.value_sets.clone(),
            address_ranges: segment.column_stats.address_ranges.clone(),
            null_counts: segment.column_stats.null_counts.clone(),
            indexed_at_micros: segment.column_stats.indexed_at_micros,
            shard: None,
        });
    }

    let directory = Arc::new(MemoryMetricDirectory::default());

    let observations = names
        .iter()
        .map(|name| MetricObservation {
            metric_name: name.clone(),
            metric_kind: None,
            labels: ["code", "instance", "method", "service"]
                .map(str::to_string)
                .to_vec(),
            first_seen: TimestampMicros(BASE_MICROS),
            last_seen: TimestampMicros(BASE_MICROS + (hours as i64 + 1) * HOUR_MICROS),
        })
        .collect::<Vec<_>>();

    directory.record(&stream, &observations).await.unwrap();

    Metrics {
        stream,
        files,
        directory,
    }
}

fn promql_engine(
    metrics: &Metrics,
    store: &Arc<SlowStore>,
    cache: Option<Arc<StreamingAggCache>>,
) -> PromQLEngine {
    let engine = PromQLEngine::new(
        metrics.files.clone() as Arc<dyn QueryFileSource>,
        store.clone() as Arc<dyn ObjectStore>,
    )
    .with_streams(Arc::new(OneStream(metrics.stream.clone())) as Arc<dyn StreamRepository>)
    .with_metric_directory(metrics.directory.clone());

    match cache {
        Some(cache) => engine.with_streaming_cache(cache, Duration::from_secs(300)),
        None => engine,
    }
}

/// A range query over `[from, to]` with a point every five minutes of a day.
async fn run_promql(
    metrics: &Metrics,
    store: &Arc<SlowStore>,
    engine: &PromQLEngine,
    query: &str,
    (from, to): (i64, i64),
    delay: bool,
) -> Sample {
    let request = QueryRequest {
        org_id: metrics.stream.org_id.clone(),
        language: QueryLanguage::Promql,
        statement: query.to_owned(),
        time_range: TimeRange::new(TimestampMicros(from), TimestampMicros(to)),
        stream: None,
        limit: Some(((to - from) / (5 * MINUTE_MICROS)).max(2) as usize),
        step: Some(5 * MINUTE_MICROS),
        federation_clusters: Vec::new(),
    };

    store.start(delay);
    let started = Instant::now();
    let result = engine.execute(request).await.expect("promql query");
    let millis = started.elapsed().as_millis();
    let (gets, heads, bytes) = store.counts();
    store.start(false);

    Sample {
        gets,
        heads,
        bytes,
        millis,
        rows: result.rows.len(),
        answer: format!("{} points", result.rows.len()),
    }
}

/// How the samples would load without the page index: every file in turn, every row of the
/// window decoded, the rows of the other metrics included, matchers applied afterwards in memory.
/// The I/O and decoding alone, without building series.
async fn load_every_file(
    metrics: &Metrics,
    store: &Arc<SlowStore>,
    (from, to): (i64, i64),
    delay: bool,
) -> Sample {
    let files = metrics.files.inner.lock().unwrap().clone();
    let object_store = store.clone() as Arc<dyn ObjectStore>;
    let reader = ParquetReader::new(object_store.clone());
    let columns = [
        "_timestamp",
        "value",
        "metric_name",
        "service",
        "instance",
        "method",
        "code",
    ];

    store.start(delay);
    let started = Instant::now();
    let mut rows = 0;

    for file in files
        .iter()
        .filter(|file| file.time_range.end.0 >= from && file.time_range.start.0 <= to)
    {
        let options = ReadOptions::new()
            .with_time_range(from, to)
            .with_known_size(file.size_bytes)
            .with_columns(&columns);

        let batches = reader
            .read_from_store(object_store.clone(), &file.object_key, options)
            .await
            .unwrap();

        rows += batches.iter().map(RecordBatch::num_rows).sum::<usize>();
    }

    let millis = started.elapsed().as_millis();
    let (gets, heads, bytes) = store.counts();
    store.start(false);

    Sample {
        gets,
        heads,
        bytes,
        millis,
        rows,
        answer: format!("{rows} rows decoded"),
    }
}

fn report_promql(scenario: &str, query: &str, sample: &Sample) {
    println!(
        "| {scenario} | {query} | {} | {} | {} | {} | {} |",
        sample.gets,
        sample.heads,
        mib(sample.bytes),
        sample.millis,
        sample.answer,
    );

    assert!(sample.rows > 0, "{query} returned nothing");
}

/// PromQL over the hourly files of a metrics stream holding the measured metric, 1600 series,
/// beside other metrics of the same shape. A selector reads only the pages of its metric, picks its
/// rows before the samples are decoded, and reads the files eight at a time; `every row` reads
/// every file in turn and decodes every row of the window, the other metrics' included (its I/O
/// and decoding only). A dashboard refresh, the window five minutes later, reads only the newest
/// window once the range cache holds the closed ones.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn promql_io_by_selector() {
    if std::env::var("QUERY_IO_BENCH").is_err() {
        eprintln!("PromQL I/O benchmark skipped: QUERY_IO_BENCH is not set");
        return;
    }

    let env = |name: &str, default: u64| {
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(default)
    };

    let hours = env("PROMQL_IO_HOURS", 24) as usize;
    let others = env("PROMQL_IO_OTHER_METRICS", 3) as usize;
    let latency = Duration::from_millis(env("QUERY_IO_LATENCY_MS", 30));
    let bytes_per_sec = env("QUERY_IO_MBPS", 100) as f64 * 1024.0 * 1024.0;
    let store = Arc::new(SlowStore::new(latency, bytes_per_sec));

    let started = Instant::now();
    let metrics = write_metrics(store.clone(), hours, others).await;

    eprintln!(
        "{hours} hourly metric files of {} metrics written in {:.1}s",
        others + 1,
        started.elapsed().as_secs_f64()
    );

    let window = (BASE_MICROS, BASE_MICROS + hours as i64 * HOUR_MICROS);
    let refresh = (window.0 + 5 * MINUTE_MICROS, window.1 + 5 * MINUTE_MICROS);

    let queries = [
        (
            "one service's GETs",
            "sum(rate(http_requests_total{service=\"svc-3\",method=\"GET\"}[5m]))",
        ),
        (
            "one series",
            "rate(http_requests_total{service=\"svc-3\",instance=\"10.0.3.7:9090\",method=\"GET\",code=\"200\"}[5m])",
        ),
        ("every series", "sum(rate(http_requests_total[5m]))"),
    ];

    println!(
        "\n{hours} files, {} metrics of {} series, latency {} ms, {} MiB/s per request\n",
        others + 1,
        SERVICES * INSTANCES * METHODS.len() * CODES.len(),
        latency.as_millis(),
        bytes_per_sec / (1024.0 * 1024.0)
    );
    println!("| scenario | query | GET | HEAD | MiB read | ms | answer |");
    println!("|---|---|---|---|---|---|---|");

    for (scenario, delay) in [("cpu only", false), ("no disk cache, cold", true)] {
        let sample = load_every_file(&metrics, &store, window, delay).await;
        report_promql(scenario, "every row, any selector", &sample);

        for (name, query) in queries {
            let engine = promql_engine(&metrics, &store, None);
            let sample = run_promql(&metrics, &store, &engine, query, window, delay).await;
            report_promql(scenario, name, &sample);
        }
    }

    // A dashboard refresh: the same panel five minutes later, process caches warm.
    for (name, query) in queries {
        for cached in [false, true] {
            let cache = cached
                .then(|| Arc::new(StreamingAggCache::new(&StreamAggCacheSettings::default())));
            let engine = promql_engine(&metrics, &store, cache);

            run_promql(&metrics, &store, &engine, query, window, false).await;
            let sample = run_promql(&metrics, &store, &engine, query, refresh, true).await;

            let scenario = if cached {
                "refresh, range cache"
            } else {
                "refresh, no range cache"
            };

            report_promql(scenario, name, &sample);
        }
    }
}
