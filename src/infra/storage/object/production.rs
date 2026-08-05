// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Production ObjectStore decorator：所有生产对象 I/O 都从这里获得一致的
//! timeout/retry/concurrency、低基数 metrics 与脱敏 Span。对象完整 key 永不进入
//! Span；multipart part/retry 使用有界 Event，不为每个 part 创建子 Span。

use std::{
    fmt,
    future::Future,
    ops::Range,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use bytes::Bytes;
use futures::{FutureExt, StreamExt, stream::BoxStream};
use object_store::{
    CopyOptions, Error as OsError, GetOptions, GetResult, ListResult, MultipartUpload, ObjectMeta,
    ObjectStore, ObjectStoreExt, PutMultipartOptions, PutOptions, PutPayload, PutResult,
    RenameOptions, Result as ObjectStoreResult, UploadPart, path::Path as ObjPath,
};
use prometheus::{HistogramVec, IntCounterVec};
use tokio::sync::Semaphore;
use tracing::{Instrument, field};

use crate::{
    config::ObjectStoreSettings,
    infra::caching::ParquetDiskCache,
    shared::metrics::{register_histogram_vec, register_int_counter_vec},
};

#[derive(Clone)]
pub struct ProductionObjectStore {
    inner: Arc<dyn ObjectStore>,
    backend: &'static str,
    settings: ObjectStoreSettings,
    semaphore: Arc<Semaphore>,
    /// 可选磁盘缓存。`get_or_cache` 命中直读，miss 走 inner.get 再异步落盘。
    disk_cache: Option<Arc<ParquetDiskCache>>,
}

impl ProductionObjectStore {
    pub fn wrap(inner: Arc<dyn ObjectStore>, settings: ObjectStoreSettings) -> Arc<Self> {
        let backend = match settings.backend.as_str() {
            "s3" => "s3",
            "azure" => "azure",
            "gcs" => "gcs",
            "local" => "local",
            _ => "unknown",
        };
        let permits = settings.max_concurrency.max(1) as usize;
        Arc::new(Self {
            inner,
            backend,
            semaphore: Arc::new(Semaphore::new(permits)),
            settings,
            disk_cache: None,
        })
    }

    /// 注入磁盘缓存。`Arc<Self>` 不便链式调用，给一个 builder 形态——
    /// 由 wire 在 wrap 之后调用：`Arc::new(ProductionObjectStore { disk_cache: Some(c), .. (*wrapped).clone() })`。
    pub fn with_disk_cache(mut self, cache: Arc<ParquetDiskCache>) -> Self {
        self.disk_cache = Some(cache);
        self
    }

    pub fn backend(&self) -> &str {
        self.backend
    }

    /// 当前是否挂了 disk cache（wire 注入后为 Some）。
    pub fn disk_cache(&self) -> Option<&Arc<ParquetDiskCache>> {
        self.disk_cache.as_ref()
    }

    fn op_timeout(&self) -> Duration {
        Duration::from_secs(self.settings.op_timeout_secs.max(1) as u64)
    }

    async fn run<T, F, Fut, B>(
        &self,
        operation: &'static str,
        location: Option<&ObjPath>,
        mut call: F,
        success_bytes: B,
    ) -> ObjectStoreResult<T>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = ObjectStoreResult<T>>,
        B: Fn(&T) -> u64,
    {
        let span = operation_span(self.backend, operation, location);
        let started = Instant::now();
        let policy = RetryPolicy::from(&self.settings);
        let result = async {
            let _permit = tokio::time::timeout(self.op_timeout(), self.semaphore.acquire())
                .await
                .map_err(|_| timeout_error(self.backend, "concurrency_wait"))?
                .map_err(|_| timeout_error(self.backend, "concurrency_closed"))?;
            let mut retries = 0_u32;
            loop {
                let attempt = retries + 1;
                let result = tokio::time::timeout(self.op_timeout(), call())
                    .await
                    .unwrap_or_else(|_| Err(timeout_error(self.backend, operation)));
                match result {
                    Ok(value) => {
                        span.record("molesignal.object.retry_count", retries);
                        let bytes = success_bytes(&value);
                        span.record("molesignal.object.bytes", bytes);
                        ops_total()
                            .with_label_values(&[self.backend, operation])
                            .inc();
                        if bytes > 0 {
                            bytes_total()
                                .with_label_values(&[self.backend, operation])
                                .inc_by(bytes);
                        }
                        return Ok(value);
                    }
                    Err(error)
                        if attempt < policy.max_attempts && RetryPolicy::is_retryable(&error) =>
                    {
                        retries = retries.saturating_add(1);
                        let reason = error_reason(&error);
                        tracing::warn!(
                            molesignal.span_event = true,
                            otel.event.name = "object_store.retry",
                            retry.attempt = attempt,
                            error.type = reason,
                        );
                        policy.backoff(attempt).await;
                    }
                    Err(error) => {
                        let reason = error_reason(&error);
                        span.record("molesignal.object.retry_count", retries);
                        span.record("error.type", reason);
                        errors_total()
                            .with_label_values(&[self.backend, operation, reason])
                            .inc();
                        return Err(error);
                    }
                }
            }
        }
        .instrument(span.clone())
        .await;
        op_dur()
            .with_label_values(&[self.backend, operation])
            .observe(started.elapsed().as_secs_f64());
        result
    }

    /// parquet_reader 走这条路径——hit 直接返磁盘 bytes；miss 才去 object_store
    /// 拉，拉回后异步落盘 + 升级 LRU。命中/未命中都更新 `object_store_operations_total{op="get_or_cache"}`
    /// 计数器（外部观测）。
    ///
    /// 注意：本方法不走 `Self::semaphore` / retry——这两层属于真正的 inner.get；
    /// disk_cache 仅是热路径短路。
    pub async fn get_or_cache(&self, path: &ObjPath) -> Result<Bytes, OsError> {
        if let Some(cache) = &self.disk_cache {
            if let Some(b) = cache.get(path.as_ref()).await {
                ops_total()
                    .with_label_values(&[self.backend, "disk_cache_hit"])
                    .inc();
                return Ok(b);
            }
            ops_total()
                .with_label_values(&[self.backend, "disk_cache_miss"])
                .inc();
        }
        let payload = self.get(path).await?;
        let bytes = payload.bytes().await?;
        if let Some(cache) = &self.disk_cache {
            // 异步落盘失败仅 warn，不影响主路径
            let key = path.as_ref().to_string();
            let bytes_clone = bytes.clone();
            let cache_clone = cache.clone();
            tokio::spawn(async move {
                if let Err(e) = cache_clone.insert(&key, bytes_clone).await {
                    tracing::warn!(error = %e, "disk_cache insert failed");
                }
            });
        }
        Ok(bytes)
    }
}

impl fmt::Debug for ProductionObjectStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProductionObjectStore")
            .field("backend", &self.backend)
            .field("max_concurrency", &self.settings.max_concurrency)
            .finish_non_exhaustive()
    }
}

impl fmt::Display for ProductionObjectStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "MoleSignalProductionObjectStore({})",
            self.backend
        )
    }
}

#[async_trait::async_trait]
impl ObjectStore for ProductionObjectStore {
    async fn put_opts(
        &self,
        location: &ObjPath,
        payload: PutPayload,
        options: PutOptions,
    ) -> ObjectStoreResult<PutResult> {
        let bytes = payload.content_length() as u64;
        self.run(
            "put",
            Some(location),
            || {
                self.inner
                    .put_opts(location, payload.clone(), options.clone())
            },
            move |_| bytes,
        )
        .await
    }

    async fn put_multipart_opts(
        &self,
        location: &ObjPath,
        options: PutMultipartOptions,
    ) -> ObjectStoreResult<Box<dyn MultipartUpload>> {
        let upload = self
            .run(
                "multipart",
                Some(location),
                || self.inner.put_multipart_opts(location, options.clone()),
                |_| 0,
            )
            .await?;
        Ok(Box::new(InstrumentedMultipartUpload {
            inner: upload,
            span: operation_span(self.backend, "multipart_session", Some(location)),
            backend: self.backend,
            parts: 0,
            bytes: 0,
        }))
    }

    async fn get_opts(
        &self,
        location: &ObjPath,
        options: GetOptions,
    ) -> ObjectStoreResult<GetResult> {
        let operation = if options.head {
            "head"
        } else if options.range.is_some() {
            "get_range"
        } else {
            "get"
        };
        self.run(
            operation,
            Some(location),
            || self.inner.get_opts(location, options.clone()),
            |result| result.meta.size,
        )
        .await
    }

    async fn get_ranges(
        &self,
        location: &ObjPath,
        ranges: &[Range<u64>],
    ) -> ObjectStoreResult<Vec<Bytes>> {
        let ranges = ranges.to_vec();
        self.run(
            "get_ranges",
            Some(location),
            || self.inner.get_ranges(location, &ranges),
            |values| values.iter().map(|value| value.len() as u64).sum(),
        )
        .await
    }

    fn delete_stream(
        &self,
        locations: BoxStream<'static, ObjectStoreResult<ObjPath>>,
    ) -> BoxStream<'static, ObjectStoreResult<ObjPath>> {
        let backend = self.backend;
        let span = operation_span(backend, "delete", None);
        self.inner
            .delete_stream(locations)
            .map(move |result| {
                let _entered = span.enter();
                match &result {
                    Ok(_) => ops_total().with_label_values(&[backend, "delete"]).inc(),
                    Err(error) => {
                        let reason = error_reason(error);
                        span.record("error.type", reason);
                        errors_total()
                            .with_label_values(&[backend, "delete", reason])
                            .inc();
                    }
                }
                result
            })
            .boxed()
    }

    fn list(&self, prefix: Option<&ObjPath>) -> BoxStream<'static, ObjectStoreResult<ObjectMeta>> {
        let backend = self.backend;
        let span = operation_span(backend, "list", prefix);
        ops_total().with_label_values(&[backend, "list"]).inc();
        self.inner
            .list(prefix)
            .map(move |result| {
                let _entered = span.enter();
                if let Err(error) = &result {
                    let reason = error_reason(error);
                    span.record("error.type", reason);
                    errors_total()
                        .with_label_values(&[backend, "list", reason])
                        .inc();
                }
                result
            })
            .boxed()
    }

    async fn list_with_delimiter(&self, prefix: Option<&ObjPath>) -> ObjectStoreResult<ListResult> {
        self.run(
            "list_delimiter",
            prefix,
            || self.inner.list_with_delimiter(prefix),
            |_| 0,
        )
        .await
    }

    async fn copy_opts(
        &self,
        from: &ObjPath,
        to: &ObjPath,
        options: CopyOptions,
    ) -> ObjectStoreResult<()> {
        self.run(
            "copy",
            Some(from),
            || self.inner.copy_opts(from, to, options.clone()),
            |_| 0,
        )
        .await
    }

    async fn rename_opts(
        &self,
        from: &ObjPath,
        to: &ObjPath,
        options: RenameOptions,
    ) -> ObjectStoreResult<()> {
        self.run(
            "rename",
            Some(from),
            || self.inner.rename_opts(from, to, options.clone()),
            |_| 0,
        )
        .await
    }
}

struct InstrumentedMultipartUpload {
    inner: Box<dyn MultipartUpload>,
    span: tracing::Span,
    backend: &'static str,
    parts: u32,
    bytes: u64,
}

impl fmt::Debug for InstrumentedMultipartUpload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InstrumentedMultipartUpload")
            .field("backend", &self.backend)
            .field("parts", &self.parts)
            .field("bytes", &self.bytes)
            .finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl MultipartUpload for InstrumentedMultipartUpload {
    fn put_part(&mut self, data: PutPayload) -> UploadPart {
        self.parts = self.parts.saturating_add(1);
        self.bytes = self
            .bytes
            .saturating_add(data.content_length().try_into().unwrap_or(u64::MAX));
        let part = self.parts;
        if part <= 128 {
            self.span.in_scope(|| {
                tracing::info!(
                    molesignal.span_event = true,
                    otel.event.name = "object_store.multipart_part",
                    multipart.part_number = part,
                    multipart.part_bytes = data.content_length() as u64,
                );
            });
        }
        let future = self.inner.put_part(data);
        let span = self.span.clone();
        async move {
            let result = future.instrument(span.clone()).await;
            if let Err(error) = &result {
                span.record("error.type", error_reason(error));
            }
            result
        }
        .boxed()
    }

    async fn complete(&mut self) -> ObjectStoreResult<PutResult> {
        let span = self.span.clone();
        let result = self.inner.complete().instrument(span.clone()).await;
        span.record("molesignal.object.bytes", self.bytes);
        span.record("molesignal.object.retry_count", 0);
        span.in_scope(|| {
            tracing::info!(
                molesignal.span_event = true,
                otel.event.name = "object_store.multipart_complete",
                multipart.part_count = self.parts,
                multipart.total_bytes = self.bytes,
            );
        });
        match &result {
            Ok(_) => {
                ops_total()
                    .with_label_values(&[self.backend, "multipart_complete"])
                    .inc();
                bytes_total()
                    .with_label_values(&[self.backend, "multipart_complete"])
                    .inc_by(self.bytes);
            }
            Err(error) => {
                let reason = error_reason(error);
                span.record("error.type", reason);
                errors_total()
                    .with_label_values(&[self.backend, "multipart_complete", reason])
                    .inc();
            }
        }
        result
    }

    async fn abort(&mut self) -> ObjectStoreResult<()> {
        let span = self.span.clone();
        let result = self.inner.abort().instrument(span.clone()).await;
        span.in_scope(|| {
            tracing::info!(
                molesignal.span_event = true,
                otel.event.name = "object_store.multipart_abort",
                multipart.part_count = self.parts,
            );
        });
        if let Err(error) = &result {
            span.record("error.type", error_reason(error));
        }
        result
    }
}

fn operation_span(
    backend: &'static str,
    operation: &'static str,
    location: Option<&ObjPath>,
) -> tracing::Span {
    let category = location.map(object_category).unwrap_or("collection");
    let fingerprint = location.and_then(|path| {
        crate::shared::trace_normalization::optional_hmac_fingerprint(path.as_ref())
    });
    tracing::info_span!(
        "object_store.operation",
        otel.kind = "client",
        molesignal.trace.category = "object_store",
        object_store.system = backend,
        object_store.operation = operation,
        molesignal.object.category = category,
        molesignal.object.key_fingerprint = fingerprint.as_deref().unwrap_or(""),
        molesignal.object.bytes = field::Empty,
        molesignal.object.retry_count = field::Empty,
        error.type = field::Empty,
    )
}

fn object_category(path: &ObjPath) -> &'static str {
    let value = path.as_ref().to_ascii_lowercase();
    if value.ends_with(".parquet") {
        "parquet"
    } else if value.ends_with(".puffin") || value.contains("tantivy") {
        "search_index"
    } else if value.contains("profile") || value.ends_with(".pprof") {
        "profile"
    } else if value.contains("replay") {
        "rum_replay"
    } else if value.contains("report") {
        "report"
    } else if value.contains("sourcemap") || value.ends_with(".map") {
        "source_map"
    } else if value.contains("parquet_file_meta") || value.contains("dump") {
        "metadata"
    } else {
        "other"
    }
}

fn timeout_error(store: &'static str, operation: &str) -> OsError {
    OsError::Generic {
        store,
        source: std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!("{operation} deadline exceeded"),
        )
        .into(),
    }
}

fn error_reason(error: &OsError) -> &'static str {
    match error {
        OsError::NotFound { .. } => "not_found",
        OsError::InvalidPath { .. } => "invalid_path",
        OsError::NotSupported { .. } | OsError::NotImplemented { .. } => "not_supported",
        OsError::AlreadyExists { .. } => "already_exists",
        OsError::Precondition { .. } => "precondition",
        OsError::NotModified { .. } => "not_modified",
        OsError::PermissionDenied { .. } => "permission_denied",
        OsError::Unauthenticated { .. } => "unauthenticated",
        OsError::UnknownConfigurationKey { .. } => "configuration",
        OsError::Generic { source, .. }
            if source
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::TimedOut) =>
        {
            "timeout"
        }
        OsError::Generic { .. } => "backend",
        OsError::JoinError { .. } => "join",
        _ => "other",
    }
}

/// metric 注册（一次性）。
static OPS_TOTAL: OnceLock<IntCounterVec> = OnceLock::new();
static BYTES_TOTAL: OnceLock<IntCounterVec> = OnceLock::new();
static ERRORS_TOTAL: OnceLock<IntCounterVec> = OnceLock::new();
static OP_DUR: OnceLock<HistogramVec> = OnceLock::new();
static HEALTH_DUR: OnceLock<HistogramVec> = OnceLock::new();

pub fn ops_total() -> &'static IntCounterVec {
    OPS_TOTAL.get_or_init(|| {
        register_int_counter_vec(
            "object_store_operations_total",
            "object store operations",
            &["backend", "op"],
        )
    })
}
pub fn bytes_total() -> &'static IntCounterVec {
    BYTES_TOTAL.get_or_init(|| {
        register_int_counter_vec(
            "object_store_bytes_total",
            "object store bytes by op",
            &["backend", "op"],
        )
    })
}
pub fn errors_total() -> &'static IntCounterVec {
    ERRORS_TOTAL.get_or_init(|| {
        register_int_counter_vec(
            "object_store_errors_total",
            "object store errors",
            &["backend", "op", "reason"],
        )
    })
}
pub fn op_dur() -> &'static HistogramVec {
    OP_DUR.get_or_init(|| {
        register_histogram_vec(
            "object_store_op_duration_seconds",
            "object store op duration",
            &["backend", "op"],
            vec![0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0, 30.0],
        )
    })
}
pub fn health_dur() -> &'static HistogramVec {
    HEALTH_DUR.get_or_init(|| {
        register_histogram_vec(
            "object_store_health_check_duration_seconds",
            "health probe round-trip",
            &["backend"],
            vec![0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0],
        )
    })
}

// =====================================================================
//  Retry policy
// =====================================================================

#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_backoff_ms: u64,
    pub max_backoff_ms: u64,
    pub jitter_ratio: f32,
}

impl RetryPolicy {
    pub fn from(settings: &ObjectStoreSettings) -> Self {
        Self {
            max_attempts: settings.retry.max_attempts.max(1),
            base_backoff_ms: settings.retry.base_backoff_ms,
            max_backoff_ms: settings.retry.max_backoff_ms,
            jitter_ratio: settings.retry.jitter_ratio,
        }
    }

    /// 永久错误（NotFound / AlreadyExists / PermissionDenied / InvalidArgument）
    /// 不重试；其余 transient 类型重试。
    pub fn is_retryable(err: &OsError) -> bool {
        match err {
            OsError::NotFound { .. }
            | OsError::AlreadyExists { .. }
            | OsError::PermissionDenied { .. }
            | OsError::Unauthenticated { .. }
            | OsError::InvalidPath { .. }
            | OsError::Precondition { .. }
            | OsError::NotModified { .. }
            | OsError::NotSupported { .. }
            | OsError::UnknownConfigurationKey { .. }
            | OsError::NotImplemented { .. } => false,
            OsError::Generic { source, .. } => {
                let msg = source.to_string().to_lowercase();
                msg.contains("timeout")
                    || msg.contains("slowdown")
                    || msg.contains("throttl")
                    || msg.contains("connection")
                    || msg.contains("5")
            }
            _ => true,
        }
    }

    pub async fn backoff(&self, attempt: u32) {
        let exp = self
            .base_backoff_ms
            .saturating_mul(1u64 << (attempt.saturating_sub(1).min(20)));
        let capped = exp.min(self.max_backoff_ms);
        // jitter
        let jitter_max = (capped as f32 * self.jitter_ratio) as u64;
        let jitter = if jitter_max > 0 {
            use std::time::SystemTime;
            let nanos = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.subsec_nanos() as u64)
                .unwrap_or(0);
            (nanos % (jitter_max * 2 + 1)) as i64 - jitter_max as i64
        } else {
            0
        };
        let wait_ms = (capped as i64 + jitter).max(0) as u64;
        tokio::time::sleep(Duration::from_millis(wait_ms)).await;
    }
}

#[cfg(test)]
mod tests {
    use futures::TryStreamExt;
    use tracing_subscriber::prelude::*;

    use super::*;

    #[test]
    fn permanent_errors_not_retried() {
        assert!(!RetryPolicy::is_retryable(&OsError::NotFound {
            path: "p".into(),
            source: "not found".to_string().into(),
        }));
        assert!(!RetryPolicy::is_retryable(&OsError::AlreadyExists {
            path: "p".into(),
            source: "already".to_string().into(),
        }));
        assert!(!RetryPolicy::is_retryable(&OsError::PermissionDenied {
            path: "p".into(),
            source: "forbidden".to_string().into(),
        }));
    }

    #[test]
    fn generic_with_throttling_retried() {
        let e = OsError::Generic {
            store: "s3",
            source: "throttling: too many requests".to_string().into(),
        };
        assert!(RetryPolicy::is_retryable(&e));
    }

    #[test]
    fn hostile_object_keys_map_to_a_closed_category_catalog_property() {
        const CATEGORIES: &[&str] = &[
            "parquet",
            "search_index",
            "profile",
            "rum_replay",
            "report",
            "source_map",
            "metadata",
            "other",
        ];
        for index in 0..512 {
            let suffix = match index % 8 {
                0 => "payload.parquet",
                1 => "tantivy/index.puffin",
                2 => "profile/cpu.pprof",
                3 => "replay/session.bin",
                4 => "report/render.bin",
                5 => "sourcemap/app.js.map",
                6 => "parquet_file_meta/dump.bin",
                _ => "unknown.bin",
            };
            let secret = format!("alice+{index}@example.com/private-token-{index:08}");
            let path = ObjPath::from(format!("tenant-{index}/{secret}/{suffix}"));
            let category = object_category(&path);
            assert!(CATEGORIES.contains(&category));
            assert!(category.len() <= 16);
            assert!(!category.contains("alice"));
            assert!(!category.contains("token"));
            assert!(!category.contains(&index.to_string()));
        }
    }

    #[tokio::test]
    async fn get_or_cache_hit_increments_metrics_and_skips_inner() {
        use object_store::{ObjectStore, PutPayload, memory::InMemory, path::Path as ObjPath};
        use tempfile::TempDir;

        use crate::infra::caching::{
            DiskCacheSettings as InfraDiskCacheSettings, ParquetDiskCache,
        };

        let tmp = TempDir::new().expect("tmpdir");
        let cache = Arc::new(
            ParquetDiskCache::new(InfraDiskCacheSettings {
                dir: tmp.path().to_path_buf(),
                max_bytes: 1 << 20,
            })
            .expect("cache builds"),
        );
        let inner: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let path = ObjPath::from("e2e/test.parquet");
        inner
            .put(&path, PutPayload::from_static(b"hello-parquet"))
            .await
            .expect("seed inner");

        let wrapped = ProductionObjectStore::wrap(inner.clone(), ObjectStoreSettings::default());
        let with_cache = (*wrapped).clone().with_disk_cache(cache);

        // 第一次：disk miss → 走 inner → 异步落盘
        let b1 = with_cache.get_or_cache(&path).await.expect("first get");
        assert_eq!(&b1[..], b"hello-parquet");

        // 等异步 insert 落盘（最多 500ms 轮询）
        for _ in 0..50 {
            if let Some(c) = with_cache.disk_cache()
                && c.get(path.as_ref()).await.is_some()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        // 第二次：disk hit → 直接返磁盘
        let b2 = with_cache.get_or_cache(&path).await.expect("second get");
        assert_eq!(&b2[..], b"hello-parquet");

        // /metrics 文本里必须看到 hits/misses/hit_ratio，且至少 1 个 hit。
        let text = crate::shared::metrics::gather_text().expect("gather");
        assert!(
            text.contains("cache_parquet_disk_hits_total"),
            "hits counter must appear in /metrics"
        );
        assert!(
            text.contains("cache_parquet_disk_misses_total"),
            "misses counter must appear in /metrics"
        );
        assert!(
            text.contains("cache_parquet_disk_evictions_total"),
            "evictions counter must appear in /metrics"
        );
        assert!(
            text.contains("cache_parquet_disk_hit_ratio"),
            "hit_ratio gauge must appear in /metrics"
        );

        // 解析 hits_total 与 hit_ratio 当前值，断言 hits >= 1 且 hit_ratio > 0。
        let hits =
            parse_metric_value(&text, "cache_parquet_disk_hits_total").expect("hits_total parsed");
        assert!(hits >= 1.0, "expected hits >= 1, got {hits}");
        let ratio =
            parse_metric_value(&text, "cache_parquet_disk_hit_ratio").expect("hit_ratio parsed");
        assert!(ratio > 0.0, "expected hit_ratio > 0, got {ratio}");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn decorator_covers_operations_without_recording_complete_keys() {
        use object_store::{PutPayload, memory::InMemory};

        use crate::shared::self_telemetry::{
            ResourceIdentity, SelfTelemetryHub, SelfTelemetryInit, SelfTelemetryLayer,
            SelfTelemetrySignal,
        };

        let hub = SelfTelemetryHub::new(SelfTelemetryInit {
            queue_capacity: 64,
            logs_enabled: false,
            traces_enabled: true,
            resource: ResourceIdentity::new("molesignal", "test", "test", "test", "node"),
        });
        let mut traces = hub.take_receiver(SelfTelemetrySignal::Traces).unwrap();
        let subscriber =
            tracing_subscriber::registry().with(SelfTelemetryLayer::traces(hub.clone()));
        let _guard = tracing::subscriber::set_default(subscriber);

        let wrapped =
            ProductionObjectStore::wrap(Arc::new(InMemory::new()), ObjectStoreSettings::default());
        let secret_path = ObjPath::from("tenant-private/customer-42/credential.parquet");
        let copied = ObjPath::from("tenant-private/customer-42/copy.parquet");
        wrapped
            .put(&secret_path, PutPayload::from_static(b"abcdef"))
            .await
            .unwrap();
        wrapped.get_range(&secret_path, 1..4).await.unwrap();
        wrapped.head(&secret_path).await.unwrap();
        wrapped.copy(&secret_path, &copied).await.unwrap();
        wrapped.delete(&copied).await.unwrap();
        let _: Vec<_> = wrapped.list(None).try_collect().await.unwrap();

        let mut encoded = Vec::new();
        while let Ok(event) = traces.try_recv() {
            encoded.extend(serde_json::to_vec(&event.fields).unwrap());
        }
        let encoded = String::from_utf8(encoded).unwrap();
        assert!(encoded.contains("object_store.operation"));
        assert!(encoded.contains("parquet"));
        assert!(!encoded.contains("customer-42"));
        assert!(!encoded.contains("credential.parquet"));
    }

    /// 在 prometheus textfmt 中提取无 label 的样本数值。
    /// 同名 metric 在测试进程里可能因为其它测试 emit 出现多行，本函数返回最后一行的值
    /// （Prometheus 累计计数 / Gauge 都是最新值就是当前值）。
    #[cfg(test)]
    fn parse_metric_value(text: &str, name: &str) -> Option<f64> {
        text.lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| {
                let rest = l.strip_prefix(name)?;
                // 必须紧跟空格或 '{'，避免 cache_x_hits_total 也匹配到 cache_x_hits_total_blah。
                let after = rest.chars().next()?;
                if after != ' ' && after != '{' {
                    return None;
                }
                rest.split_whitespace().next_back()?.parse::<f64>().ok()
            })
            .next_back()
    }
}
