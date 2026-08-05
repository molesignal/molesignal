// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Ingester role：IngesterWorker 组合 IngesterSink（WAL→buffer 串行）+ 后台 flush
//! scheduler（buffer→parquet→parquet_file_meta→wal_truncate 四步）+ 启动 replay。
//!
//! 由 [`crate::bootstrap::build_state`] 构造一个 `Arc<IngesterWorker>` 后：
//! 1. `recover_and_replay()` —— 重放 WAL 剩余 record 回 buffer，force-flush 一次后翻
//!    `probe.replay_done = true`。
//! 2. `spawn_flush_loop()` —— 后台 tokio task：`interval(flush_interval_secs) OR Notify`，
//!    遍历 BufferPool 触发 [`IngesterWorker::flush_one`]。
//! 3. `Arc<IngesterWorker>` 同时是 `IngestSink`，注入到 [`IngestService`]。

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use dashmap::DashMap;
use futures::{StreamExt, stream};
use tokio::{
    sync::{Mutex, Notify},
    task::JoinHandle,
};

use crate::{
    config::IngesterSettings,
    domain::{
        ingestion::{IngestBatch, IngestResult, IngestSink},
        storage::{ParquetFileMetaRepository, PhysicalDatasetKind},
        stream::StreamRepository,
    },
    infra::{
        caching::ParquetFileMetaCache,
        cipher::FieldKeyService,
        ingester::{
            AdaptiveRotation, BufferKey, BufferPool, FlushInflightGuard, IngesterSink,
            RotationReason, WalPool, inc_flush_error, inc_rotation,
        },
        storage::parquet::writer::ParquetWriter,
    },
    shared::{Error, Result, drain::DrainController, health::Probe},
};

#[derive(Debug, Clone, Copy)]
struct PendingWalCommit {
    high_watermark_seq: u64,
    accounted_size_bytes: usize,
}

pub struct IngesterWorker {
    sink: Arc<IngesterSink>,
    wal: Arc<WalPool>,
    buffer: Arc<BufferPool>,
    streams: Arc<dyn StreamRepository>,
    parquet_file_meta: Arc<dyn ParquetFileMetaRepository>,
    parquet_writer: Arc<ParquetWriter>,
    parquet_file_meta_cache: Option<Arc<ParquetFileMetaCache>>,
    probe: Arc<Probe>,
    settings: IngesterSettings,
    flush_notify: Arc<Notify>,
    /// 完整 flush transaction 的 per-key single-flight；不同 key 仍可并行。
    flush_locks: DashMap<BufferKey, Arc<Mutex<()>>>,
    /// ParquetFileMeta 已提交、但 WAL seal/truncate 尚未成功的 generation。
    pending_wal_commits: DashMap<BufferKey, PendingWalCommit>,
    adaptive_rotation: AdaptiveRotation,
    /// 字段加密 DEK 服务；replay 时按 org 解析 DEK 后注入 buffer 再重放（与 sink 同语义）。
    field_keys: Option<Arc<FieldKeyService>>,
    /// 节点 drain 状态；退役中 flush loop 把 buffer 全部落盘后 mark_drained 并退出。
    drain: Option<Arc<DrainController>>,
}

impl IngesterWorker {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        wal: Arc<WalPool>,
        buffer: Arc<BufferPool>,
        streams: Arc<dyn StreamRepository>,
        parquet_file_meta: Arc<dyn ParquetFileMetaRepository>,
        parquet_writer: Arc<ParquetWriter>,
        parquet_file_meta_cache: Option<Arc<ParquetFileMetaCache>>,
        probe: Arc<Probe>,
        settings: IngesterSettings,
    ) -> Self {
        let buffer_max_bytes = (settings.buffer_max_mb as usize).saturating_mul(1024 * 1024);
        let adaptive_rotation = AdaptiveRotation::new(&settings.rotation, buffer_max_bytes);
        let sink = Arc::new(IngesterSink::new(
            wal.clone(),
            buffer.clone(),
            streams.clone(),
        ));
        Self {
            sink,
            wal,
            buffer,
            streams,
            parquet_file_meta,
            parquet_writer,
            parquet_file_meta_cache,
            probe,
            settings,
            flush_notify: Arc::new(Notify::new()),
            flush_locks: DashMap::new(),
            pending_wal_commits: DashMap::new(),
            adaptive_rotation,
            field_keys: None,
            drain: None,
        }
    }

    /// 注入节点 drain 状态；退役中 flush loop 落盘剩余 buffer 后退出。
    pub fn with_drain(mut self, drain: Arc<DrainController>) -> Self {
        self.drain = Some(drain);
        self
    }

    /// 注入字段加密 DEK 服务：sink（写路径）与 replay 路径同源用它解析 org DEK。
    pub fn with_field_keys(mut self, svc: Arc<FieldKeyService>) -> Self {
        self.sink = Arc::new(
            IngesterSink::new(self.wal.clone(), self.buffer.clone(), self.streams.clone())
                .with_field_keys(svc.clone()),
        );
        self.field_keys = Some(svc);
        self
    }

    fn rotation_threshold(&self, key: &BufferKey) -> usize {
        self.adaptive_rotation.threshold_for(key)
    }

    fn flush_max_age(&self) -> Duration {
        Duration::from_secs(self.settings.flush_interval_secs.max(1) as u64)
    }

    fn flush_lock(&self, key: &BufferKey) -> Arc<Mutex<()>> {
        self.flush_locks
            .entry(key.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// 启动期 replay：扫 WAL 残留 → 重新喂 buffer → 强制 flush 一次 → set probe.replay_done(true)。
    #[tracing::instrument(
        name = "worker.wal_replay",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "wal_replay")
    )]
    pub async fn recover_and_replay(&self) -> Result<()> {
        tracing::info!("ingester wal replay starting");
        let restored = self
            .wal
            .recover()
            .map_err(|e| Error::internal(format!("wal recover: {e}")))?;
        let mut row_count = 0usize;
        for (key, records) in restored {
            let stream_def = match self.streams.get(&key.0, &key.2, key.1).await {
                Ok(d) => d,
                Err(e) => {
                    // 残留 WAL 里引用了已不存在的旧流定义时跳过即可——这在改过
                    // schema / 清过 DB 的本地环境很常见，是无害噪音，降到 debug。
                    tracing::debug!(?key, error=%e, "stream definition missing during replay; skip");
                    continue;
                }
            };
            // 字段加密：流含 `encrypted` 字段时，重放前解析 org 当前 DEK 注入 buffer
            //（与写路径同语义，保证 replay 也加密落 parquet）。
            let field_dek = if stream_def.schema.fields.iter().any(|f| f.encrypted) {
                match &self.field_keys {
                    Some(svc) => match svc.current(&key.0).await {
                        Ok(dek) => Some(dek),
                        Err(e) => {
                            tracing::warn!(?key, error=%e, "field DEK resolve failed during replay; skip stream");
                            continue;
                        }
                    },
                    None => {
                        tracing::warn!(
                            ?key,
                            "stream has encrypted fields but no field key service; skip replay"
                        );
                        continue;
                    }
                }
            } else {
                None
            };
            let buf_handle = self.buffer.get_or_create_dataset(&stream_def, key.3);
            let mut guard = buf_handle.lock().await;
            if let Some(dek) = field_dek {
                guard.set_field_key(dek);
            }
            for rec in records {
                let batch: IngestBatch = match serde_json::from_slice(&rec.payload) {
                    Ok(b) => b,
                    Err(e) => {
                        tracing::warn!(?key, error=%e, "wal payload decode failed; skip");
                        continue;
                    }
                };
                if batch.events.is_empty() {
                    continue;
                }
                let reservation = self.buffer.force_reserve(rec.payload.len())?;
                guard.add_accounted_bytes(reservation.commit());
                for ev in &batch.events {
                    guard
                        .push(ev, rec.index)
                        .map_err(|e| Error::internal(format!("replay push: {e}")))?;
                    row_count += 1;
                }
            }
        }
        tracing::info!(rows = row_count, "ingester replay loaded; force-flushing");
        self.flush_keys(self.buffer.snapshot_keys(), true, "replay")
            .await;
        self.probe.set_replay_done(true);
        tracing::info!("ingester ready");
        Ok(())
    }

    /// `(buffer.finish_and_clear → parquet/tantivy → parquet_file_meta.insert → wal truncate)`。
    /// 对象或元数据失败会恢复 batch + reservation；ParquetFileMeta 已提交后的 WAL 失败只重试
    /// seal/truncate，避免重复生成对象，并在 WAL 提交完成前继续保留 reservation。
    #[tracing::instrument(
        name = "worker.ingester_flush",
        parent = None,
        skip_all,
        fields(
            otel.kind = "internal",
            molesignal.worker.name = "ingester_flush",
            molesignal.stream.type = ?key.1
        )
    )]
    pub async fn flush_one(&self, key: &BufferKey) -> Result<()> {
        self.flush_one_inner(key, None).await
    }

    async fn flush_one_if_due(&self, key: &BufferKey) -> Result<()> {
        self.flush_one_if_due_at(key, Instant::now()).await
    }

    async fn flush_one_if_due_at(&self, key: &BufferKey, now: Instant) -> Result<()> {
        self.flush_one_inner(key, Some(now)).await
    }

    async fn flush_one_inner(&self, key: &BufferKey, due_at: Option<Instant>) -> Result<()> {
        let flush_lock = self.flush_lock(key);
        let _single_flight = flush_lock.lock().await;
        self.retry_pending_wal_commit(key).await?;
        let buf_handle = match self.buffer.get(key) {
            Some(b) => b,
            None => return Ok(()),
        };
        let stream_def = self.streams.get(&key.0, &key.2, key.1).await?;

        let mut guard = buf_handle.lock().await;
        if guard.is_empty() {
            return Ok(());
        }
        let reason = match due_at {
            None => RotationReason::Forced,
            Some(now) => {
                match guard.rotation_due(self.rotation_threshold(key), self.flush_max_age(), now) {
                    Some(reason) => reason,
                    None => return Ok(()),
                }
            }
        };
        let accounted_size_bytes = guard.accounted_size_bytes();
        let estimated_raw_bytes = if accounted_size_bytes > 0 {
            accounted_size_bytes
        } else {
            guard.approx_size_bytes()
        };
        let (batch, hwm) = guard
            .finish_and_clear()
            .map_err(|e| Error::internal(format!("buffer snapshot: {e}")))?;
        drop(guard);

        inc_rotation(key.1.as_str(), reason);
        let _inflight = FlushInflightGuard::enter(key.1.as_str());

        // step 1 parquet —— 传克隆：`RecordBatch` 是 Arc 数组的浅壳，克隆只是 refcount，
        // 失败时用原件 restore 回 buffer。
        let physical_stream = crate::infra::ingester::physical_schema::project(&stream_def, key.3);
        let flush_result = self
            .parquet_writer
            .flush_partitioned_with_index(&physical_stream, key.3, batch.clone())
            .await;
        let outputs = match flush_result {
            Ok(output) => output,
            Err(e) => {
                inc_flush_error("parquet_write");
                self.restore_buffer(&buf_handle, batch, accounted_size_bytes)
                    .await;
                return Err(e);
            }
        };

        // step 2 parquet_file_meta insert
        let encoded_size_bytes = outputs.iter().map(|(meta, _)| meta.size_bytes).sum();
        let metadata = outputs.iter().map(|(meta, _)| meta.clone()).collect();
        if let Err(e) = self.parquet_file_meta.insert_many(metadata).await {
            inc_flush_error("parquet_file_meta_insert");
            for (meta, archive) in &outputs {
                if let Err(cleanup_error) = self
                    .parquet_writer
                    .delete_outputs(
                        &meta.object_key,
                        archive.as_ref().map(|archive| archive.object_key.as_str()),
                    )
                    .await
                {
                    tracing::warn!(
                        ?key,
                        error = %cleanup_error,
                        "failed to delete orphan outputs after parquet_file_meta insert failure"
                    );
                }
            }
            self.restore_buffer(&buf_handle, batch, accounted_size_bytes)
                .await;
            return Err(e);
        }

        if let Some(c) = &self.parquet_file_meta_cache {
            c.invalidate_prefix(&key.0, &key.2, key.1);
        }
        self.adaptive_rotation
            .observe(key, estimated_raw_bytes, encoded_size_bytes);

        // step 3 WAL：ParquetFileMeta 已提交，失败时不重复写对象；只保留 hwm + reservation，
        // 下一轮 per-key flush 先重试该提交，成功后才释放全局内存预算。
        if let Err(error) = self.commit_wal(key, hwm).await {
            self.pending_wal_commits.insert(
                key.clone(),
                PendingWalCommit {
                    high_watermark_seq: hwm,
                    accounted_size_bytes,
                },
            );
            return Err(error);
        }
        self.buffer.release_memory(accounted_size_bytes);
        Ok(())
    }

    async fn flush_keys(&self, keys: Vec<BufferKey>, force: bool, phase: &'static str) {
        let parallelism = self.settings.flush_parallelism.max(1) as usize;
        stream::iter(keys)
            .for_each_concurrent(parallelism, |key| async move {
                let result = if force {
                    self.flush_one(&key).await
                } else {
                    self.flush_one_if_due(&key).await
                };
                if let Err(error) = result {
                    tracing::error!(?key, %error, phase, "ingester flush failed");
                }
            })
            .await;
    }

    async fn restore_buffer(
        &self,
        buf_handle: &Arc<tokio::sync::Mutex<crate::infra::ingester::RecordBuilder>>,
        batch: arrow::array::RecordBatch,
        accounted_size_bytes: usize,
    ) {
        buf_handle
            .lock()
            .await
            .restore_batch_with_accounting(batch, accounted_size_bytes);
    }

    async fn retry_pending_wal_commit(&self, key: &BufferKey) -> Result<()> {
        let Some(pending) = self.pending_wal_commits.get(key).map(|entry| *entry) else {
            return Ok(());
        };
        self.commit_wal(key, pending.high_watermark_seq).await?;
        self.pending_wal_commits.remove(key);
        self.buffer.release_memory(pending.accounted_size_bytes);
        Ok(())
    }

    async fn commit_wal(&self, key: &BufferKey, high_watermark_seq: u64) -> Result<()> {
        // truncate_up_to 不动活跃段；先 seal，否则已持久化 record 会在重启时重复 replay。
        if let Err(error) = self.wal.seal_active(key).await {
            inc_flush_error("wal_seal");
            return Err(Error::internal(format!("wal seal_active: {error}")));
        }
        if let Err(error) = self.wal.truncate_up_to(key, high_watermark_seq).await {
            inc_flush_error("wal_truncate");
            return Err(Error::internal(format!("wal truncate: {error}")));
        }
        Ok(())
    }

    /// 后台 flush task：interval tick + Notify 双驱。
    pub fn spawn_flush_loop(self: Arc<Self>) -> JoinHandle<()> {
        let me = self;
        tokio::spawn(async move {
            let secs = me.settings.flush_interval_secs.max(1) as u64;
            let mut interval = tokio::time::interval(Duration::from_secs(secs));
            // 跳过第一次 tick（避免 worker 起来还没 replay 完就 flush）
            interval.tick().await;
            loop {
                tokio::select! {
                    _ = interval.tick() => {}
                    _ = me.flush_notify.notified() => {}
                    // 退役信号：不再傻等下一次 flush 间隔，开始 drain 即刻醒来落盘退出。
                    _ = drain_tripwire(&me.drain) => {}
                }
                // 节点退役：写入已被 IngestService 拒（停接新活），把残留 buffer 再 flush 一遍
                // 兜底竞态后标记 drained 退出——pending parquet 已全部落盘，可安全下线本节点。
                if let Some(drain) = &me.drain
                    && drain.is_draining()
                {
                    me.flush_keys(me.buffer.snapshot_keys(), true, "drain")
                        .await;
                    me.flush_keys(me.buffer.snapshot_keys(), true, "drain_final")
                        .await;
                    drain.mark_drained();
                    tracing::info!("ingester drained: pending buffers flushed; flush loop exiting");
                    break;
                }
                me.flush_keys(me.buffer.snapshot_keys(), false, "steady")
                    .await;
            }
        })
    }

    pub fn notify_flush(&self) {
        self.flush_notify.notify_one();
    }
}

/// 节点进入退役即 resolve；无 drain controller（单机 / 测试路径）则永不 resolve。
/// `DrainController` 刻意运行时无关（无锁原子、无异步唤醒），故由 flush loop 在此以短
/// 间隔观察它——让停服在 ~200ms 内落盘退出，而非苦等满一个 `flush_interval_secs`。
async fn drain_tripwire(drain: &Option<Arc<DrainController>>) {
    match drain {
        Some(d) => {
            while !d.is_draining() {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
        None => std::future::pending().await,
    }
}

#[async_trait]
impl IngestSink for IngesterWorker {
    fn supports_derived_datasets(&self) -> bool {
        true
    }

    async fn write(&self, batch: IngestBatch) -> Result<IngestResult> {
        self.write_dataset(PhysicalDatasetKind::Raw, batch).await
    }

    async fn write_dataset(
        &self,
        dataset_kind: PhysicalDatasetKind,
        batch: IngestBatch,
    ) -> Result<IngestResult> {
        let key: BufferKey = (
            batch.org_id.clone(),
            batch.stream_type,
            batch.stream.clone(),
            dataset_kind,
        );
        let res = self.sink.write_dataset(dataset_kind, batch).await?;
        if let Some(buffer) = self.buffer.get(&key)
            && buffer
                .lock()
                .await
                .rotation_due(
                    self.rotation_threshold(&key),
                    self.flush_max_age(),
                    Instant::now(),
                )
                .is_some()
        {
            // 仅当前 buffer 已 due 时唤醒；无后续写入的 age 由周期 tick 发现。
            self.flush_notify.notify_one();
        }
        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Mutex as StdMutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    use object_store::{ObjectStore, memory::InMemory};
    use serde_json::json;
    use tempfile::tempdir;
    use tokio::sync::Semaphore;

    use super::*;
    use crate::{
        domain::{
            ingestion::RawEvent,
            storage::ParquetFileMeta,
            stream::{FieldDef, FieldType, Retention, Schema, StreamDefinition, StreamType},
        },
        infra::segment_wal::{FsyncPolicy, StaticTermSource},
        shared::{
            ids::Id,
            time::{TimeRange, TimestampMicros},
        },
    };

    struct OneStreamRepo(StreamDefinition);

    #[async_trait]
    impl StreamRepository for OneStreamRepo {
        async fn create(&self, def: StreamDefinition) -> Result<StreamDefinition> {
            Ok(def)
        }
        async fn update_schema(&self, _id: &Id, _schema: Schema) -> Result<()> {
            Ok(())
        }
        async fn get(&self, org: &Id, name: &str, st: StreamType) -> Result<StreamDefinition> {
            let mut stream = self.0.clone();
            stream.org_id = org.clone();
            stream.name = name.to_string();
            stream.stream_type = st;
            Ok(stream)
        }
        async fn list(&self, _org: &Id) -> Result<Vec<StreamDefinition>> {
            Ok(vec![self.0.clone()])
        }
        async fn delete(&self, _id: &Id) -> Result<()> {
            Ok(())
        }
    }

    /// 头一次 `insert` 注入失败，之后放行——用来精确打中 flush 的 step 2。
    struct FailFirstInsert {
        fail_next: AtomicBool,
        inserted: StdMutex<Vec<ParquetFileMeta>>,
    }

    #[async_trait]
    impl ParquetFileMetaRepository for FailFirstInsert {
        async fn insert(&self, file: ParquetFileMeta) -> Result<()> {
            if self.fail_next.swap(false, Ordering::SeqCst) {
                return Err(Error::internal("injected parquet_file_meta insert failure"));
            }
            self.inserted.lock().unwrap().push(file);
            Ok(())
        }
        async fn find(
            &self,
            _org: &Id,
            _stream: &str,
            _st: StreamType,
            _tr: TimeRange,
        ) -> Result<Vec<ParquetFileMeta>> {
            Ok(self.inserted.lock().unwrap().clone())
        }
        async fn replace(&self, _merged: &[Id], _new: Vec<ParquetFileMeta>) -> Result<()> {
            Ok(())
        }
        async fn mark_deleted(&self, _ids: &[Id]) -> Result<usize> {
            Ok(0)
        }
    }

    struct TrackingInsert {
        inserted: StdMutex<Vec<ParquetFileMeta>>,
        inflight: AtomicUsize,
        max_inflight: AtomicUsize,
        delay: Duration,
        gate_first: AtomicBool,
        first_entered: Option<Arc<tokio::sync::Notify>>,
        release_first: Option<Arc<Semaphore>>,
    }

    impl TrackingInsert {
        fn delayed(delay: Duration) -> Arc<Self> {
            Arc::new(Self {
                inserted: StdMutex::new(Vec::new()),
                inflight: AtomicUsize::new(0),
                max_inflight: AtomicUsize::new(0),
                delay,
                gate_first: AtomicBool::new(false),
                first_entered: None,
                release_first: None,
            })
        }

        fn gated(entered: Arc<tokio::sync::Notify>, release: Arc<Semaphore>) -> Arc<Self> {
            Arc::new(Self {
                inserted: StdMutex::new(Vec::new()),
                inflight: AtomicUsize::new(0),
                max_inflight: AtomicUsize::new(0),
                delay: Duration::ZERO,
                gate_first: AtomicBool::new(true),
                first_entered: Some(entered),
                release_first: Some(release),
            })
        }
    }

    #[async_trait]
    impl ParquetFileMetaRepository for TrackingInsert {
        async fn insert(&self, file: ParquetFileMeta) -> Result<()> {
            let current = self.inflight.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_inflight.fetch_max(current, Ordering::SeqCst);
            if self.gate_first.swap(false, Ordering::SeqCst) {
                self.first_entered.as_ref().unwrap().notify_one();
                let _permit = self
                    .release_first
                    .as_ref()
                    .unwrap()
                    .acquire()
                    .await
                    .unwrap();
            }
            tokio::time::sleep(self.delay).await;
            self.inserted.lock().unwrap().push(file);
            self.inflight.fetch_sub(1, Ordering::SeqCst);
            Ok(())
        }

        async fn find(
            &self,
            _org: &Id,
            _stream: &str,
            _st: StreamType,
            _tr: TimeRange,
        ) -> Result<Vec<ParquetFileMeta>> {
            Ok(self.inserted.lock().unwrap().clone())
        }

        async fn replace(&self, _merged: &[Id], _new: Vec<ParquetFileMeta>) -> Result<()> {
            Ok(())
        }

        async fn mark_deleted(&self, _ids: &[Id]) -> Result<usize> {
            Ok(0)
        }
    }

    fn stream_def() -> StreamDefinition {
        StreamDefinition {
            id: Id::new(),
            org_id: Id::from_string("org-a"),
            name: "app".into(),
            stream_type: StreamType::Logs,
            schema: Schema {
                fields: vec![FieldDef {
                    name: "level".into(),
                    data_type: FieldType::Utf8,
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

    fn batch_of(s: &StreamDefinition, base_ts: i64, n: usize) -> IngestBatch {
        IngestBatch {
            batch_id: Id::new(),
            org_id: s.org_id.clone(),
            stream: s.name.clone(),
            stream_type: s.stream_type,
            events: (0..n)
                .map(|i| {
                    let mut f = serde_json::Map::new();
                    f.insert("level".into(), json!("info"));
                    RawEvent {
                        timestamp: TimestampMicros(base_ts + i as i64 * 1_000),
                        fields: f,
                    }
                })
                .collect(),
            received_at: TimestampMicros::now(),
        }
    }

    fn worker_with_meta(
        wal_dir: &std::path::Path,
        stream: &StreamDefinition,
        parquet_file_meta: Arc<dyn ParquetFileMetaRepository>,
        settings: IngesterSettings,
    ) -> Arc<IngesterWorker> {
        let wal = Arc::new(WalPool::new(
            wal_dir,
            64 * 1024,
            FsyncPolicy::none_default(),
            Arc::new(StaticTermSource(1)),
        ));
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        Arc::new(IngesterWorker::new(
            wal,
            Arc::new(BufferPool::new()),
            Arc::new(OneStreamRepo(stream.clone())) as Arc<dyn StreamRepository>,
            parquet_file_meta,
            Arc::new(ParquetWriter::new(store)),
            None,
            Arc::new(Probe::new()),
            settings,
        ))
    }

    #[tokio::test]
    async fn due_flush_ignores_small_write_then_rotates_by_age_and_size() {
        let tmp = tempdir().unwrap();
        let stream = stream_def();
        let meta = TrackingInsert::delayed(Duration::ZERO);
        let settings = IngesterSettings {
            buffer_max_mb: 1,
            flush_interval_secs: 30,
            ..IngesterSettings::default()
        };
        let worker = worker_with_meta(
            tmp.path(),
            &stream,
            meta.clone() as Arc<dyn ParquetFileMetaRepository>,
            settings,
        );
        let key = (
            stream.org_id.clone(),
            stream.stream_type,
            stream.name.clone(),
            PhysicalDatasetKind::Raw,
        );

        worker.write(batch_of(&stream, 1_000_000, 1)).await.unwrap();
        worker
            .flush_one_if_due_at(&key, Instant::now())
            .await
            .unwrap();
        assert!(meta.inserted.lock().unwrap().is_empty());

        worker
            .flush_one_if_due_at(&key, Instant::now() + Duration::from_secs(31))
            .await
            .unwrap();
        assert_eq!(meta.inserted.lock().unwrap().len(), 1);

        let mut fields = serde_json::Map::new();
        fields.insert("level".into(), json!("x".repeat(1024 * 1024)));
        worker
            .buffer
            .get(&key)
            .unwrap()
            .lock()
            .await
            .push(
                &RawEvent {
                    timestamp: TimestampMicros(2_000_000),
                    fields,
                },
                2,
            )
            .unwrap();
        worker.flush_one_if_due(&key).await.unwrap();
        assert_eq!(meta.inserted.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn flush_parallelism_bounds_distinct_stream_transactions() {
        let tmp = tempdir().unwrap();
        let base = stream_def();
        let meta = TrackingInsert::delayed(Duration::from_millis(40));
        let settings = IngesterSettings {
            flush_parallelism: 2,
            ..IngesterSettings::default()
        };
        let worker = worker_with_meta(
            tmp.path(),
            &base,
            meta.clone() as Arc<dyn ParquetFileMetaRepository>,
            settings,
        );
        let mut keys = Vec::new();
        for index in 0..5 {
            let mut stream = base.clone();
            stream.name = format!("stream-{index}");
            worker
                .write(batch_of(&stream, 1_000_000 + index * 10_000, 1))
                .await
                .unwrap();
            keys.push((
                stream.org_id.clone(),
                stream.stream_type,
                stream.name.clone(),
                PhysicalDatasetKind::Raw,
            ));
        }

        worker.flush_keys(keys, true, "test").await;

        assert_eq!(meta.inserted.lock().unwrap().len(), 5);
        assert_eq!(meta.max_inflight.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn concurrent_same_stream_flushes_commit_in_generation_order() {
        let tmp = tempdir().unwrap();
        let stream = stream_def();
        let first_entered = Arc::new(tokio::sync::Notify::new());
        let release_first = Arc::new(Semaphore::new(0));
        let meta = TrackingInsert::gated(first_entered.clone(), release_first.clone());
        let worker = worker_with_meta(
            tmp.path(),
            &stream,
            meta.clone() as Arc<dyn ParquetFileMetaRepository>,
            IngesterSettings::default(),
        );
        let key = (
            stream.org_id.clone(),
            stream.stream_type,
            stream.name.clone(),
            PhysicalDatasetKind::Raw,
        );
        worker.write(batch_of(&stream, 1_000_000, 1)).await.unwrap();

        let first_worker = worker.clone();
        let first_key = key.clone();
        let first = tokio::spawn(async move { first_worker.flush_one(&first_key).await });
        tokio::time::timeout(Duration::from_secs(2), first_entered.notified())
            .await
            .expect("first flush must reach metadata insert");
        assert!(
            worker.buffer.reserved_bytes() > 0,
            "detached generation must remain charged while metadata is in flight"
        );

        worker.write(batch_of(&stream, 5_000_000, 1)).await.unwrap();
        let second_worker = worker.clone();
        let second_key = key.clone();
        let second = tokio::spawn(async move { second_worker.flush_one(&second_key).await });
        tokio::task::yield_now().await;
        assert_eq!(meta.max_inflight.load(Ordering::SeqCst), 1);

        release_first.add_permits(1);
        first.await.unwrap().unwrap();
        second.await.unwrap().unwrap();
        assert_eq!(worker.buffer.reserved_bytes(), 0);

        let starts: Vec<i64> = meta
            .inserted
            .lock()
            .unwrap()
            .iter()
            .map(|file| file.time_range.start.0)
            .collect();
        assert_eq!(starts, vec![1_000_000, 5_000_000]);
        assert_eq!(meta.max_inflight.load(Ordering::SeqCst), 1);
    }

    /// spec 4.5：flush 任一步失败 → buffer 内容保持不变，数据既不丢也不重。
    ///
    /// 这里打 step 2（parquet_file_meta insert）：step 1 已经把 parquet 写出去了，此时若 restore
    /// 语义有误，失败的那批要么丢（buffer 被清空）要么在重试时翻倍（暂存没清）。
    #[tokio::test]
    async fn flush_failure_preserves_buffer_and_retry_lands_every_row_once() {
        let tmp = tempdir().unwrap();
        let mut s = stream_def();
        s.schema.fields[0].indexed = true;
        let wal = Arc::new(WalPool::new(
            tmp.path(),
            64 * 1024,
            FsyncPolicy::none_default(),
            Arc::new(StaticTermSource(1)),
        ));
        let buffer = Arc::new(BufferPool::new());
        let parquet_file_meta = Arc::new(FailFirstInsert {
            fail_next: AtomicBool::new(true),
            inserted: StdMutex::new(Vec::new()),
        });
        let store = Arc::new(InMemory::new());
        let worker = Arc::new(IngesterWorker::new(
            wal,
            buffer.clone(),
            Arc::new(OneStreamRepo(s.clone())) as Arc<dyn StreamRepository>,
            parquet_file_meta.clone() as Arc<dyn ParquetFileMetaRepository>,
            Arc::new(ParquetWriter::new(store.clone())),
            None,
            Arc::new(Probe::new()),
            IngesterSettings::default(),
        ));
        let key: BufferKey = (
            s.org_id.clone(),
            s.stream_type,
            s.name.clone(),
            PhysicalDatasetKind::Raw,
        );

        // 2 行 → flush 打在 step 2 上
        worker.write(batch_of(&s, 1_000_000, 2)).await.unwrap();
        assert!(
            worker.flush_one(&key).await.is_err(),
            "注入的 parquet_file_meta 失败必须向上报错"
        );
        assert!(
            parquet_file_meta.inserted.lock().unwrap().is_empty(),
            "失败的那次不该留下 parquet_file_meta 行"
        );
        assert!(
            store.list(None).collect::<Vec<_>>().await.is_empty(),
            "parquet_file_meta 失败必须清理 parquet 和 tantivy orphan"
        );
        assert!(buffer.reserved_bytes() > 0, "失败 generation 必须继续计费");

        // 失败后再来 1 行，重试
        worker.write(batch_of(&s, 5_000_000, 1)).await.unwrap();
        worker.flush_one(&key).await.expect("重试必须成功");
        assert_eq!(
            buffer.reserved_bytes(),
            0,
            "完整 flush 成功后必须释放 reservation"
        );

        let rows: Vec<u64> = parquet_file_meta
            .inserted
            .lock()
            .unwrap()
            .iter()
            .map(|m| m.rows)
            .collect();
        assert_eq!(rows, vec![3], "失败的 2 行 + 新的 1 行，恰好一个文件 3 行");

        // buffer 已交空：再 flush 不得产生任何新文件（否则就是重复落盘）
        worker
            .flush_one(&key)
            .await
            .expect("空 buffer flush 应 no-op");
        assert_eq!(
            parquet_file_meta.inserted.lock().unwrap().len(),
            1,
            "同一批数据不得被落盘第二次"
        );
    }

    /// flush 失败后若一直没有新写入，暂存的数据仍必须在下一次 flush 被落盘，
    /// 不能因为 buffer 自身 row_count == 0 就被永久跳过。
    #[tokio::test]
    async fn flush_failure_without_further_writes_still_retries() {
        let tmp = tempdir().unwrap();
        let s = stream_def();
        let wal = Arc::new(WalPool::new(
            tmp.path(),
            64 * 1024,
            FsyncPolicy::none_default(),
            Arc::new(StaticTermSource(1)),
        ));
        let parquet_file_meta = Arc::new(FailFirstInsert {
            fail_next: AtomicBool::new(true),
            inserted: StdMutex::new(Vec::new()),
        });
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let worker = Arc::new(IngesterWorker::new(
            wal,
            Arc::new(BufferPool::new()),
            Arc::new(OneStreamRepo(s.clone())) as Arc<dyn StreamRepository>,
            parquet_file_meta.clone() as Arc<dyn ParquetFileMetaRepository>,
            Arc::new(ParquetWriter::new(store)),
            None,
            Arc::new(Probe::new()),
            IngesterSettings::default(),
        ));
        let key: BufferKey = (
            s.org_id.clone(),
            s.stream_type,
            s.name.clone(),
            PhysicalDatasetKind::Raw,
        );

        worker.write(batch_of(&s, 1_000_000, 4)).await.unwrap();
        assert!(worker.flush_one(&key).await.is_err());

        // 没有任何新写入，直接重试
        worker.flush_one(&key).await.expect("重试必须成功");
        let rows: Vec<u64> = parquet_file_meta
            .inserted
            .lock()
            .unwrap()
            .iter()
            .map(|m| m.rows)
            .collect();
        assert_eq!(rows, vec![4], "暂存的 4 行必须被重新落盘");
    }
}
