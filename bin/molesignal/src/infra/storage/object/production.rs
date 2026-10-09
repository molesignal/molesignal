// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Production ObjectStore decorator：所有生产对象 I/O 都从这里获得一致的
//! timeout/retry/concurrency、低基数 metrics 与脱敏 Span。对象完整 key 永不进入
//! Span；multipart part/retry 使用有界 Event，不为每个 part 创建子 Span。
//!
//! 达到 `[object_store]` 阈值的大对象写入与范围读取由 `transfer` 拆成多个后端请求。

use std::{
    fmt,
    future::Future,
    ops::Range,
    sync::Arc,
    time::{Duration, Instant},
};

use bytes::Bytes;
use futures::{StreamExt, stream::BoxStream};
use object_store::{
    CopyOptions, GetOptions, GetResult, ListResult, MultipartUpload, ObjectMeta, ObjectStore,
    PutMultipartOptions, PutOptions, PutPayload, PutResult, RenameOptions,
    Result as ObjectStoreResult, path::Path as ObjPath,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tracing::Instrument;

use crate::config::ObjectStoreSettings;

mod multipart;
mod retry;
mod telemetry;
mod transfer;

use multipart::InstrumentedMultipartUpload;
use retry::RetryPolicy;
pub use telemetry::health_dur;
use telemetry::{
    bytes_total, error_reason, errors_total, op_dur, operation_span, ops_total, timeout_error,
};

#[derive(Clone)]
pub struct ProductionObjectStore {
    inner: Arc<dyn ObjectStore>,
    backend: &'static str,
    settings: ObjectStoreSettings,
    semaphore: Arc<Semaphore>,
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
        })
    }

    pub fn backend(&self) -> &str {
        self.backend
    }

    fn op_timeout(&self) -> Duration {
        Duration::from_secs(self.settings.op_timeout_secs.max(1) as u64)
    }

    async fn run<T, F, Fut, B>(
        &self,
        operation: &'static str,
        location: Option<&ObjPath>,
        call: F,
        success_bytes: B,
    ) -> ObjectStoreResult<T>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = ObjectStoreResult<T>>,
        B: Fn(&T) -> u64,
    {
        self.run_with(operation, location, Budget::Request, call, success_bytes)
            .await
    }

    /// 统一的并发许可、超时、重试与指标。
    async fn run_with<T, F, Fut, B>(
        &self,
        operation: &'static str,
        location: Option<&ObjPath>,
        budget: Budget,
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
            let _permit = match budget {
                Budget::Request => Some(self.acquire_permit().await?),
                Budget::Composite => None,
            };
            let mut retries = 0_u32;
            loop {
                let attempt = retries + 1;
                let result = match budget {
                    Budget::Request => tokio::time::timeout(self.op_timeout(), call())
                        .await
                        .unwrap_or_else(|_| Err(timeout_error(self.backend, operation))),
                    Budget::Composite => call().await,
                };
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

    async fn acquire_permit(&self) -> ObjectStoreResult<OwnedSemaphorePermit> {
        tokio::time::timeout(self.op_timeout(), self.semaphore.clone().acquire_owned())
            .await
            .map_err(|_| timeout_error(self.backend, "concurrency_wait"))?
            .map_err(|_| timeout_error(self.backend, "concurrency_closed"))
    }
}

/// 一次 `run_with` 的资源预算。
#[derive(Clone, Copy)]
enum Budget {
    /// 单个后端请求：占用一个并发许可，每次尝试受 `op_timeout_secs` 约束。
    Request,
    /// 由多个后端请求组成的传输：外层只负责整体重试，每个内部请求各自占用许可并计时。
    Composite,
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
        if self.wants_multipart(location, &payload, &options) {
            return self.put_in_parts(location, payload, options).await;
        }
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
        Ok(Box::new(InstrumentedMultipartUpload::new(
            upload,
            operation_span(self.backend, "multipart_session", Some(location)),
            self.backend,
        )))
    }

    async fn get_opts(
        &self,
        location: &ObjPath,
        options: GetOptions,
    ) -> ObjectStoreResult<GetResult> {
        if let Some(range) = self.chunked_range(&options) {
            return self.get_range_in_chunks(location, options, range).await;
        }
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
            // 请求实际返回的字节范围；范围读取不计整个对象的大小。
            |result| result.range.end.saturating_sub(result.range.start),
        )
        .await
    }

    async fn get_ranges(
        &self,
        location: &ObjPath,
        ranges: &[Range<u64>],
    ) -> ObjectStoreResult<Vec<Bytes>> {
        self.get_ranges_split(location, ranges).await
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

#[cfg(test)]
mod tests {
    use futures::TryStreamExt;
    use object_store::{Error as OsError, ObjectStoreExt};
    use tracing_subscriber::prelude::*;

    use super::{telemetry::object_category, *};

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
                6 => "manifests/v1/org/dataset/p-0-00/1.parquet",
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
    #[tokio::test(flavor = "current_thread")]
    async fn decorator_covers_operations_without_recording_complete_keys() {
        use object_store::{PutPayload, memory::InMemory};

        use crate::shared::self_telemetry::{
            ResourceIdentity, SelfTelemetryHub, SelfTelemetryInit, SelfTelemetryLayer,
            SelfTelemetrySignal,
        };

        let hub = SelfTelemetryHub::new(SelfTelemetryInit {
            queue_capacity: 64,
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
}
