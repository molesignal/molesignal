// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! PromQL 端到端：写 http_requests_total 系列到本地 parquet → 调 PromQLEngine。
//!
//! 无 docker：用 in-mem ParquetFileMetaRepository + local LocalFileSystem object store。

use std::{
    collections::HashMap,
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};

use arrow::{
    array::{Float64Array, RecordBatch, StringArray, TimestampMicrosecondArray},
    datatypes::{DataType, Field, Schema as ArrowSchema, TimeUnit},
};
use async_trait::async_trait;
use molesignal::{
    config::StreamAggCacheSettings,
    domain::{
        query::{PromqlEngine, QueryLanguage, QueryRequest},
        storage::{ParquetFileMeta, ParquetFileMetaRepository},
        stream::{
            FieldDef, FieldType, Retention, Schema, StreamDefinition, StreamRepository,
            StreamSettings, StreamType,
        },
    },
    infra::{
        caching::StreamingAggCache, query::promql::PromQLEngine,
        storage::parquet::writer::ParquetWriter,
    },
    shared::{
        Error, Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};
use object_store::{ObjectStore, local::LocalFileSystem};

// ---- in-mem ParquetFileMetaRepository ----
#[derive(Default)]
struct InMemParquetFileMeta {
    inner: StdMutex<HashMap<String, ParquetFileMeta>>,
    /// `find` 调用计数：增量缓存测试用它证明 run2 跳过了 parquet 扫描。
    finds: std::sync::atomic::AtomicUsize,
}
impl InMemParquetFileMeta {
    fn find_count(&self) -> usize {
        self.finds.load(std::sync::atomic::Ordering::SeqCst)
    }
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
        self.finds.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
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
        Err(Error::internal("not supported in promql test"))
    }

    async fn mark_deleted(&self, _ids: &[Id]) -> Result<usize> {
        Err(Error::internal("not supported in promql test"))
    }
}

// ---- mock StreamRepository：仅 metrics「http_requests_total」存在，queryable 由 flag 控制 ----
struct FakeStreams {
    queryable: bool,
}
#[async_trait]
impl StreamRepository for FakeStreams {
    async fn create(&self, def: StreamDefinition) -> Result<StreamDefinition> {
        Ok(def)
    }
    async fn update_schema(&self, _id: &Id, _schema: Schema) -> Result<()> {
        Ok(())
    }
    async fn get(
        &self,
        org_id: &Id,
        name: &str,
        stream_type: StreamType,
    ) -> Result<StreamDefinition> {
        if stream_type == StreamType::Metrics && name == "http_requests_total" {
            let mut def = metric_stream();
            def.org_id = org_id.clone();
            Ok(def)
        } else {
            Err(Error::not_found(format!("stream {name}")))
        }
    }
    async fn list(&self, _org_id: &Id) -> Result<Vec<StreamDefinition>> {
        Ok(vec![])
    }
    async fn get_settings(&self, _id: &Id) -> Result<StreamSettings> {
        Ok(StreamSettings {
            queryable: self.queryable,
            ..Default::default()
        })
    }
    async fn delete(&self, _id: &Id) -> Result<()> {
        Ok(())
    }
}

fn metric_stream() -> StreamDefinition {
    StreamDefinition {
        id: Id::new(),
        org_id: Id::from_string("orga"),
        name: "http_requests_total".into(),
        stream_type: StreamType::Metrics,
        schema: Schema {
            fields: vec![
                FieldDef {
                    name: "value".into(),
                    data_type: FieldType::Float64,
                    nullable: false,
                    indexed: false,
                    encrypted: false,
                    exact: false,
                },
                FieldDef {
                    name: "method".into(),
                    data_type: FieldType::Utf8,
                    nullable: false,
                    indexed: true,
                    encrypted: false,
                    exact: false,
                },
                FieldDef {
                    name: "code".into(),
                    data_type: FieldType::Utf8,
                    nullable: false,
                    indexed: true,
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

/// 构造 RecordBatch：(ts, value, method, code)；100 个样本 / 60s。
fn build_batch(start_us: i64, n: usize, method: &str, code: &str) -> RecordBatch {
    let schema = Arc::new(ArrowSchema::new(vec![
        Field::new(
            "_timestamp",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        ),
        Field::new("value", DataType::Float64, false),
        Field::new("method", DataType::Utf8, false),
        Field::new("code", DataType::Utf8, false),
    ]));
    let step = 60_000_000 / n as i64; // 60s / n samples
    let ts = TimestampMicrosecondArray::from(
        (0..n)
            .map(|i| start_us + i as i64 * step)
            .collect::<Vec<_>>(),
    )
    .with_timezone("UTC");
    // counter 累加：value(i) = i + offset_for_method
    let vals = Float64Array::from((0..n).map(|i| i as f64).collect::<Vec<_>>());
    let methods = StringArray::from(vec![method; n]);
    let codes = StringArray::from(vec![code; n]);
    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(ts),
            Arc::new(vals),
            Arc::new(methods),
            Arc::new(codes),
        ],
    )
    .unwrap()
}

#[tokio::test]
async fn rate_and_sum_by_method() {
    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
    let writer = ParquetWriter::new(store.clone());
    let repo: Arc<InMemParquetFileMeta> = Arc::new(InMemParquetFileMeta::default());
    let stream = metric_stream();

    // 在 [t0, t0+60s] 内为 method=GET 写 60 个样本（counter 0..59）+ method=POST 写 60 个（counter 0..59）
    let t0: i64 = 1_700_000_000_000_000;
    for (method, code) in &[("GET", "200"), ("POST", "200")] {
        let batch = build_batch(t0, 60, method, code);
        let meta = writer.flush(&stream, batch).await.unwrap();
        repo.insert(meta).await.unwrap();
    }

    let engine = PromQLEngine::new(
        repo.clone() as Arc<dyn ParquetFileMetaRepository>,
        store.clone(),
    );

    // 1) rate(http_requests_total[5m]) — 每秒变化率
    //    序列每个 60s 内 0..59，rate = (59 - 0)/60 ≈ 0.983
    let req = QueryRequest {
        org_id: stream.org_id.clone(),
        language: QueryLanguage::Promql,
        statement: "rate(http_requests_total[5m])".into(),
        time_range: TimeRange::new(TimestampMicros(t0), TimestampMicros(t0 + 60_000_000)),
        stream: None,
        limit: None,
        federation_clusters: Vec::new(),
    };
    let res = engine.execute(req).await.expect("rate query");
    assert_eq!(res.rows.len(), 2, "two series: GET + POST");

    // 2) sum by(method)(rate(...)) — 按 method 聚合（每条 series 仍是 0.983，因为 method=GET/POST 各一条）
    let req = QueryRequest {
        org_id: stream.org_id.clone(),
        language: QueryLanguage::Promql,
        statement: "sum by(method)(rate(http_requests_total[5m]))".into(),
        time_range: TimeRange::new(TimestampMicros(t0), TimestampMicros(t0 + 60_000_000)),
        stream: None,
        limit: None,
        federation_clusters: Vec::new(),
    };
    let res = engine.execute(req).await.expect("sum by query");
    assert_eq!(res.rows.len(), 2, "two groups: method=GET / method=POST");

    // 3) holt_winters — 等差 counter 的双指数平滑收敛到末端值附近
    let req = QueryRequest {
        org_id: stream.org_id.clone(),
        language: QueryLanguage::Promql,
        statement: "holt_winters(http_requests_total[1h], 0.5, 0.5)".into(),
        time_range: TimeRange::new(TimestampMicros(t0), TimestampMicros(t0 + 60_000_000)),
        stream: None,
        limit: None,
        federation_clusters: Vec::new(),
    };
    let res = engine.execute(req).await.expect("holt_winters query");
    assert_eq!(res.rows.len(), 2, "two series: GET + POST");

    // 4) native-histogram 系列（classic bucket 模型下 N/A）仍不支持 → Err
    let req = QueryRequest {
        org_id: stream.org_id.clone(),
        language: QueryLanguage::Promql,
        statement: "histogram_count(http_requests_total)".into(),
        time_range: TimeRange::new(TimestampMicros(t0), TimestampMicros(t0 + 60_000_000)),
        stream: None,
        limit: None,
        federation_clusters: Vec::new(),
    };
    let err = engine.execute(req).await.unwrap_err();
    assert!(
        err.to_string().contains("not yet supported"),
        "expected unsupported error, got {err}"
    );
}

/// 性能冒烟（手动跑）：百万级密集样本上的 range 查询要在秒级内完成。
///
/// ```bash
/// cargo test --release -p molesignal-infra --test it_promql -- --ignored perf_smoke
/// ```
///
/// step 降采样修复前，该路径对窗口内每个原始样本各出一个点且重扫整条
/// series（O(n²)），BENCHMARKS 实测 ~3M 行窗口 >30s 超时。
#[tokio::test]
#[ignore = "perf smoke; run manually in release"]
async fn perf_smoke_range_query_over_dense_samples() {
    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
    let writer = ParquetWriter::new(store.clone());
    let repo: Arc<InMemParquetFileMeta> = Arc::new(InMemParquetFileMeta::default());
    let stream = metric_stream();

    // 20 个文件 × 60_000 样本 = 1.2M 样本，覆盖 [t0, t0+20min)
    let t0: i64 = 1_700_000_000_000_000;
    for i in 0..20 {
        let batch = build_batch(t0 + i * 60_000_000, 60_000, "GET", "200");
        let meta = writer.flush(&stream, batch).await.unwrap();
        repo.insert(meta).await.unwrap();
    }

    let engine = PromQLEngine::new(
        repo.clone() as Arc<dyn ParquetFileMetaRepository>,
        store.clone(),
    );
    let req = QueryRequest {
        org_id: stream.org_id.clone(),
        language: QueryLanguage::Promql,
        statement: "rate(http_requests_total[5m])".into(),
        time_range: TimeRange::new(TimestampMicros(t0), TimestampMicros(t0 + 1_200_000_000)),
        stream: None,
        limit: Some(1_000),
        federation_clusters: Vec::new(),
    };
    let started = std::time::Instant::now();
    let res = engine.execute(req).await.expect("range query");
    let took = started.elapsed();
    assert!(!res.rows.is_empty() && res.rows.len() <= 1_000);
    assert!(
        took < std::time::Duration::from_secs(10),
        "range over 1.2M dense samples took {took:?}"
    );
    eprintln!(
        "perf_smoke: 1.2M samples, {} rows, took {took:?}",
        res.rows.len()
    );
}

/// Range 查询（limit>1）：按 step 出点，输出规模受 step 与 limit 约束，
/// 与原始采样密度（每秒 1 个样本）无关。
#[tokio::test]
async fn rate_range_query_is_step_bounded() {
    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
    let writer = ParquetWriter::new(store.clone());
    let repo: Arc<InMemParquetFileMeta> = Arc::new(InMemParquetFileMeta::default());
    let stream = metric_stream();

    let t0: i64 = 1_700_000_000_000_000;
    for (method, code) in &[("GET", "200"), ("POST", "200")] {
        let batch = build_batch(t0, 60, method, code);
        let meta = writer.flush(&stream, batch).await.unwrap();
        repo.insert(meta).await.unwrap();
    }

    let engine = PromQLEngine::new(
        repo.clone() as Arc<dyn ParquetFileMetaRepository>,
        store.clone(),
    );
    let req = QueryRequest {
        org_id: stream.org_id.clone(),
        language: QueryLanguage::Promql,
        statement: "rate(http_requests_total[5m])".into(),
        time_range: TimeRange::new(TimestampMicros(t0), TimestampMicros(t0 + 60_000_000)),
        stream: None,
        limit: Some(100),
        federation_clusters: Vec::new(),
    };
    let res = engine.execute(req).await.expect("range query");
    assert!(
        !res.rows.is_empty() && res.rows.len() <= 100,
        "expected (0, 100] rows, got {}",
        res.rows.len()
    );
    assert_eq!(res.columns[0], "_timestamp");
    assert_eq!(res.columns[1], "value");
}

/// 单文件、给定步长的线性 counter（value = 自 `data_start` 起的秒数），覆盖
/// `[data_start, data_end]`。各窗口 rate ≈ 1/s，便于断言两条求值路径完全一致。
fn build_counter_batch(
    data_start: i64,
    data_end: i64,
    sample_step_us: i64,
    method: &str,
    code: &str,
) -> RecordBatch {
    let schema = Arc::new(ArrowSchema::new(vec![
        Field::new(
            "_timestamp",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        ),
        Field::new("value", DataType::Float64, false),
        Field::new("method", DataType::Utf8, false),
        Field::new("code", DataType::Utf8, false),
    ]));
    let mut ts = Vec::new();
    let mut vals = Vec::new();
    let mut t = data_start;
    while t <= data_end {
        ts.push(t);
        vals.push((t - data_start) as f64 / 1_000_000.0);
        t += sample_step_us;
    }
    let n = ts.len();
    let ts_arr = TimestampMicrosecondArray::from(ts).with_timezone("UTC");
    let val_arr = Float64Array::from(vals);
    let methods = StringArray::from(vec![method; n]);
    let codes = StringArray::from(vec![code; n]);
    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(ts_arr),
            Arc::new(val_arr),
            Arc::new(methods),
            Arc::new(codes),
        ],
    )
    .unwrap()
}

/// `ParquetWriter` 强制单个文件不得跨 UTC 小时边界（tantivy 按小时映射 `.ttv` sidecar），
/// 因此把 `[data_start, data_end]` 的采样按小时切段、逐段落独立文件，保持序列连续无间隙。
#[allow(clippy::too_many_arguments)]
async fn flush_counter_range(
    writer: &ParquetWriter,
    stream: &StreamDefinition,
    repo: &Arc<InMemParquetFileMeta>,
    data_start: i64,
    data_end: i64,
    sample_step_us: i64,
    method: &str,
    code: &str,
) {
    const HOUR_US: i64 = 3_600_000_000;
    let mut segment_start = data_start;
    while segment_start <= data_end {
        let hour_start = (segment_start / HOUR_US) * HOUR_US;
        let segment_end = (hour_start + HOUR_US - 1).min(data_end);
        let batch = build_counter_batch(segment_start, segment_end, sample_step_us, method, code);
        let meta = writer.flush(stream, batch).await.unwrap();
        repo.insert(meta).await.unwrap();
        segment_start = segment_end + sample_step_us;
    }
}

/// range 窗口聚合增量缓存：同一 range 查询连续两次（仪表盘刷新），
/// 第二次稳定桶全部命中缓存、跳过 parquet 扫描，且结果与无缓存路径逐行一致。
///
/// 数据时间戳取 2023（远早于真实 now），故水位 = `max(parquet_file_meta.end)`（数据驱动），
/// 查询窗口内全部桶都「已封存」→ 行为确定，不依赖 wall-clock。
#[tokio::test]
async fn streaming_agg_cache_reuses_stable_buckets_across_refresh() {
    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
    let writer = ParquetWriter::new(store.clone());
    let repo: Arc<InMemParquetFileMeta> = Arc::new(InMemParquetFileMeta::default());
    let stream = metric_stream();

    // step = span / limit = 60s；start 对齐到 step 网格 → 无缓存（start 锚定）与
    // 缓存（grid 对齐）两路步点重合，可逐行比对。
    let step_us: i64 = 60_000_000;
    let span_us: i64 = 60 * step_us; // 1h
    let start: i64 = (1_700_000_000_000_000 / step_us) * step_us; // 对齐
    let end: i64 = start + span_us;
    let range_us: i64 = 300_000_000; // [5m] 窗口

    // 数据覆盖 [start - 5m, end + 1m]，15s 一个样本：每个 [5m] 窗口都有足够样本；
    // 数据右端 > 查询右端 → 查询窗口内全部桶 <= 水位（稳定）。
    for (method, code) in &[("GET", "200"), ("POST", "200")] {
        flush_counter_range(
            &writer,
            &stream,
            &repo,
            start - range_us,
            end + step_us,
            15_000_000,
            method,
            code,
        )
        .await;
    }

    let mk_req = || QueryRequest {
        org_id: stream.org_id.clone(),
        language: QueryLanguage::Promql,
        statement: "rate(http_requests_total[5m])".into(),
        time_range: TimeRange::new(TimestampMicros(start), TimestampMicros(end)),
        stream: None,
        limit: Some(60),
        federation_clusters: Vec::new(),
    };

    // 基准：无缓存引擎。
    let nocache = PromQLEngine::new(
        repo.clone() as Arc<dyn ParquetFileMetaRepository>,
        store.clone(),
    );
    let res_nocache = nocache.execute(mk_req()).await.expect("nocache query");
    assert!(!res_nocache.rows.is_empty());

    let base_finds = repo.find_count();

    // 启用增量缓存的引擎（safe_lookback 300s；数据在 2023，水位仍由数据封顶）。
    let cache = Arc::new(StreamingAggCache::new(&StreamAggCacheSettings {
        capacity: 64,
        ttl_secs: 300,
        safe_lookback_secs: 300,
        max_series_per_query: 0,
    }));
    let cached = PromQLEngine::new(
        repo.clone() as Arc<dyn ParquetFileMetaRepository>,
        store.clone(),
    )
    .with_streaming_cache(cache.clone(), Duration::from_secs(300));

    // 刷新 1（冷）：稳定桶全部重算 + 封存。
    let res1 = cached.execute(mk_req()).await.expect("cached run1");
    let (h1, m1) = cache.stats();
    let finds1 = repo.find_count() - base_finds;

    // 刷新 2（暖，同窗口）：稳定桶全部命中缓存、零重算、零扫描。
    let res2 = cached.execute(mk_req()).await.expect("cached run2");
    let (h2, m2) = cache.stats();
    let finds2 = repo.find_count() - base_finds - finds1;

    // 冷查：无可服务缓存，封存了 >0 个稳定桶点。
    assert_eq!(h1, 0, "cold run serves nothing from cache");
    assert!(m1 > 0, "cold run seals stable buckets (misses={m1})");
    // 暖查：恰好命中冷查封存的全部桶点，且没有任何稳定桶被重算。
    assert_eq!(h2 - h1, m1, "run2 serves exactly what run1 sealed");
    assert_eq!(m2, m1, "run2 recomputes no stable bucket");
    // 扫描缩减：run2 比 run1 少 parquet_file_meta find —— load 被跳过，仅剩水位探测。
    assert!(
        finds2 < finds1,
        "run2 must issue fewer file finds (scan skipped): {finds2} vs {finds1}"
    );
    // 水位探测对 metrics 的 raw + rollup 两个数据集各做一次 find，恰好 2 次；多出的
    // find 意味着又发生了矩阵加载，缓存未生效。
    assert_eq!(
        finds2, 2,
        "run2 only probes ingest watermark (raw + rollup), no matrix load: {finds2}"
    );
    // 正确性：缓存路径与无缓存路径逐行一致（start 对齐 → 步点重合）。
    assert_eq!(res1.columns, res_nocache.columns);
    assert_eq!(res1.rows, res_nocache.rows, "cold cached == no-cache");
    assert_eq!(res2.rows, res_nocache.rows, "warm cached == no-cache");
}

/// 仪表盘刷新的真实形态：固定宽度窗口整体前移一个 step（start/end 同时 +step）。
/// 断言第二次刷新「只算新增的那个桶」、其余重叠桶全部命中缓存，且与无缓存路径一致。
#[tokio::test]
async fn streaming_agg_cache_slide_recomputes_only_new_bucket() {
    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
    let writer = ParquetWriter::new(store.clone());
    let repo: Arc<InMemParquetFileMeta> = Arc::new(InMemParquetFileMeta::default());
    let stream = metric_stream();

    let step_us: i64 = 60_000_000;
    let span_us: i64 = 60 * step_us; // 固定 1h 窗宽
    let start: i64 = (1_700_000_000_000_000 / step_us) * step_us;
    let end: i64 = start + span_us;
    let range_us: i64 = 300_000_000; // [5m]

    // 数据覆盖到 end + 2*step（> 滑动后查询右端 end+step）→ 滑动后新桶仍 <= 水位（稳定）。
    for (method, code) in &[("GET", "200"), ("POST", "200")] {
        flush_counter_range(
            &writer,
            &stream,
            &repo,
            start - range_us,
            end + 2 * step_us,
            15_000_000,
            method,
            code,
        )
        .await;
    }

    let mk_req = |s: i64, e: i64| QueryRequest {
        org_id: stream.org_id.clone(),
        language: QueryLanguage::Promql,
        statement: "rate(http_requests_total[5m])".into(),
        time_range: TimeRange::new(TimestampMicros(s), TimestampMicros(e)),
        stream: None,
        limit: Some(60),
        federation_clusters: Vec::new(),
    };

    let cache = Arc::new(StreamingAggCache::new(&StreamAggCacheSettings {
        capacity: 64,
        ..Default::default()
    }));
    let cached = PromQLEngine::new(
        repo.clone() as Arc<dyn ParquetFileMetaRepository>,
        store.clone(),
    )
    .with_streaming_cache(cache.clone(), Duration::from_secs(300));

    // 刷新 1：窗口 [start, end]。
    let _ = cached.execute(mk_req(start, end)).await.expect("run1");
    let (_, m1) = cache.stats();

    // 刷新 2：窗口整体前移一个 step → [start+step, end+step]，span/step 不变（同指纹）。
    let res2 = cached
        .execute(mk_req(start + step_us, end + step_us))
        .await
        .expect("run2 slide");
    let (h2, m2) = cache.stats();

    // 只新增一个桶（end+step）：每条 series 重算一次 → misses 增量 = series 数。
    let series_count = 2;
    assert_eq!(
        m2 - m1,
        series_count,
        "slide recomputes only the one new bucket per series (Δmiss={})",
        m2 - m1
    );
    // 其余重叠桶全部命中缓存。
    assert!(h2 > 0, "overlapping buckets served from cache (hits={h2})");

    // 与无缓存路径逐行一致（start+step 仍对齐 step 网格）。
    let nocache = PromQLEngine::new(
        repo.clone() as Arc<dyn ParquetFileMetaRepository>,
        store.clone(),
    );
    let res_nocache = nocache
        .execute(mk_req(start + step_us, end + step_us))
        .await
        .expect("nocache slide");
    assert_eq!(res2.rows, res_nocache.rows, "slid cached == no-cache");
}

/// queryable 闸门（PromQL 路径）：装配 StreamRepository 后，命中的 metric stream 若被标记
/// 不可查询则整条查询返回 Forbidden；标记为可查询时照常求值。
#[tokio::test]
async fn non_queryable_metric_stream_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
    let writer = ParquetWriter::new(store.clone());
    let repo: Arc<InMemParquetFileMeta> = Arc::new(InMemParquetFileMeta::default());
    let stream = metric_stream();

    let t0: i64 = 1_700_000_000_000_000;
    let meta = writer
        .flush(&stream, build_batch(t0, 60, "GET", "200"))
        .await
        .unwrap();
    repo.insert(meta).await.unwrap();

    let mk_req = || QueryRequest {
        org_id: stream.org_id.clone(),
        language: QueryLanguage::Promql,
        statement: "rate(http_requests_total[5m])".into(),
        time_range: TimeRange::new(TimestampMicros(t0), TimestampMicros(t0 + 60_000_000)),
        stream: None,
        limit: None,
        federation_clusters: Vec::new(),
    };

    // queryable = false → Forbidden「not queryable」（与「不存在」区分）。
    let blocked = PromQLEngine::new(
        repo.clone() as Arc<dyn ParquetFileMetaRepository>,
        store.clone(),
    )
    .with_streams(Arc::new(FakeStreams { queryable: false }) as Arc<dyn StreamRepository>);
    let err = blocked
        .execute(mk_req())
        .await
        .expect_err("non-queryable metric must be rejected");
    match err {
        Error::Forbidden(msg) => assert!(msg.contains("not queryable"), "unexpected: {msg}"),
        other => panic!("expected Forbidden, got {other:?}"),
    }

    // queryable = true → 照常求值。
    let allowed = PromQLEngine::new(
        repo.clone() as Arc<dyn ParquetFileMetaRepository>,
        store.clone(),
    )
    .with_streams(Arc::new(FakeStreams { queryable: true }) as Arc<dyn StreamRepository>);
    let res = allowed
        .execute(mk_req())
        .await
        .expect("queryable metric runs");
    assert_eq!(res.rows.len(), 1, "one GET series");
}
