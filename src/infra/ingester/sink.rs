// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! IngesterSink：真实 IngestSink 实现（替换 `MemoryIngestSink`）。
//!
//! 入口语义：
//! 1. 用 batch 的 `(org, stream_type, stream)` 找 [`StreamDefinition`]（schema 已由
//!    [`IngestService`] 演化好）。
//! 2. 串行先写 WAL（durable）后写 buffer（内存）：
//!    - 整批 bincode/JSON 序列化 → [`WalPool::append`]
//!    - 逐条 [`BufferPool::push`]，并把高水位 `seq` 记入 buffer
//! 3. 返 `IngestResult { accepted, rejected: 0, errors: [] }`。
//!
//! seq 编号：全局 `AtomicU64`，每条 batch 自增一次，整 batch 共享同一个 seq（足以让
//! FlushScheduler 在 truncate WAL 时识别已落 parquet 的 segments）。

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use async_trait::async_trait;

use super::{BufferPool, WalPool};
use crate::{
    domain::{
        ingestion::{IngestBatch, IngestResult, IngestSink},
        storage::PhysicalDatasetKind,
        stream::StreamRepository,
    },
    infra::cipher::FieldKeyService,
    shared::{Error, Result},
};

pub struct IngesterSink {
    wal: Arc<WalPool>,
    buffer: Arc<BufferPool>,
    streams: Arc<dyn StreamRepository>,
    /// 字段加密 DEK 服务；`None` 时不解析 DEK（无加密字段的流不受影响）。
    field_keys: Option<Arc<FieldKeyService>>,
    seq: AtomicU64,
}

impl IngesterSink {
    pub fn new(
        wal: Arc<WalPool>,
        buffer: Arc<BufferPool>,
        streams: Arc<dyn StreamRepository>,
    ) -> Self {
        Self {
            wal,
            buffer,
            streams,
            field_keys: None,
            seq: AtomicU64::new(1),
        }
    }

    /// 注入字段加密 DEK 服务；流含 `encrypted` 字段时 push 前解析 org 当前 DEK。
    pub fn with_field_keys(mut self, svc: Arc<FieldKeyService>) -> Self {
        self.field_keys = Some(svc);
        self
    }

    pub fn wal(&self) -> &Arc<WalPool> {
        &self.wal
    }

    pub fn buffer(&self) -> &Arc<BufferPool> {
        &self.buffer
    }

    fn next_seq(&self) -> u64 {
        // 只要求每批拿到互不相同的递增值，不用它给别的内存操作定序。
        self.seq.fetch_add(1, Ordering::Relaxed)
    }

    async fn write_inner(
        &self,
        dataset_kind: PhysicalDatasetKind,
        batch: IngestBatch,
    ) -> Result<IngestResult> {
        if batch.events.is_empty() {
            return Ok(IngestResult {
                accepted: 0,
                rejected: 0,
                errors: Vec::new(),
            });
        }

        let stream = self
            .streams
            .get(&batch.org_id, &batch.stream, batch.stream_type)
            .await?;

        let field_dek = if stream.schema.fields.iter().any(|f| f.encrypted) {
            match &self.field_keys {
                Some(svc) => Some(svc.current(&batch.org_id).await?),
                None => {
                    return Err(Error::internal(
                        "stream has encrypted fields but field key service not configured",
                    ));
                }
            }
        } else {
            None
        };

        let seq = self.next_seq();
        let payload = serde_json::to_vec(&batch)
            .map_err(|e| Error::internal(format!("ingest serialize: {e}")))?;
        let reservation = self.buffer.try_reserve(stream.stream_type, payload.len())?;
        let key = (
            stream.org_id.clone(),
            stream.stream_type,
            stream.name.clone(),
            dataset_kind,
        );
        self.wal
            .append(&key, payload, seq)
            .await
            .map_err(|e| Error::internal(format!("wal append: {e}")))?;

        let buffer_handle = self.buffer.get_or_create_dataset(&stream, dataset_kind);
        let mut guard = buffer_handle.lock().await;
        if let Some(dek) = field_dek {
            guard.set_field_key(dek);
        }
        guard.add_accounted_bytes(reservation.commit());
        let accepted = batch.events.len();
        let buffer_span = tracing::info_span!(
            "ingest.buffer",
            otel.kind = "internal",
            molesignal.ingest.event_count = accepted,
            molesignal.dataset.kind = dataset_kind.as_str()
        );
        buffer_span.in_scope(|| {
            for event in &batch.events {
                guard
                    .push(event, seq)
                    .map_err(|e| Error::internal(format!("buffer push: {e}")))?;
            }
            Ok::<(), Error>(())
        })?;

        Ok(IngestResult {
            accepted,
            rejected: 0,
            errors: Vec::new(),
        })
    }
}

#[async_trait]
impl IngestSink for IngesterSink {
    fn supports_derived_datasets(&self) -> bool {
        true
    }

    #[tracing::instrument(
        name = "ingest.persist",
        skip_all,
        fields(
            otel.kind = "internal",
            molesignal.ingest.event_count = batch.events.len(),
            molesignal.stream.type = ?batch.stream_type
        )
    )]
    async fn write(&self, batch: IngestBatch) -> Result<IngestResult> {
        self.write_inner(PhysicalDatasetKind::Raw, batch).await
    }

    async fn write_dataset(
        &self,
        dataset_kind: PhysicalDatasetKind,
        batch: IngestBatch,
    ) -> Result<IngestResult> {
        self.write_inner(dataset_kind, batch).await
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Mutex as StdMutex};

    use async_trait::async_trait;
    use serde_json::json;
    use tempfile::tempdir;

    use super::*;
    use crate::{
        domain::{
            ingestion::RawEvent,
            stream::{FieldDef, FieldType, Retention, Schema, StreamDefinition, StreamType},
        },
        infra::ingester::{BufferPool, WalPool},
        shared::{ids::Id, time::TimestampMicros},
    };

    struct InMemStreamRepo {
        inner: StdMutex<HashMap<(Id, String, StreamType), StreamDefinition>>,
    }

    impl InMemStreamRepo {
        fn with(stream: StreamDefinition) -> Arc<Self> {
            let mut m = HashMap::new();
            m.insert(
                (
                    stream.org_id.clone(),
                    stream.name.clone(),
                    stream.stream_type,
                ),
                stream,
            );
            Arc::new(Self {
                inner: StdMutex::new(m),
            })
        }
    }

    #[async_trait]
    impl StreamRepository for InMemStreamRepo {
        async fn create(&self, def: StreamDefinition) -> Result<StreamDefinition> {
            self.inner.lock().unwrap().insert(
                (def.org_id.clone(), def.name.clone(), def.stream_type),
                def.clone(),
            );
            Ok(def)
        }
        async fn update_schema(
            &self,
            _id: &Id,
            _schema: crate::domain::stream::Schema,
        ) -> Result<()> {
            Ok(())
        }
        async fn get(
            &self,
            org_id: &Id,
            name: &str,
            stream_type: StreamType,
        ) -> Result<StreamDefinition> {
            self.inner
                .lock()
                .unwrap()
                .get(&(org_id.clone(), name.to_string(), stream_type))
                .cloned()
                .ok_or_else(|| Error::not_found(format!("stream {name}")))
        }
        async fn list(&self, _org_id: &Id) -> Result<Vec<StreamDefinition>> {
            Ok(self.inner.lock().unwrap().values().cloned().collect())
        }
        async fn delete(&self, _id: &Id) -> Result<()> {
            Ok(())
        }
    }

    fn sample_stream() -> StreamDefinition {
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
                    indexed: true,
                    encrypted: false,
                    exact: false,
                }],
            },
            retention: Some(Retention { days: 7 }),
            created_at: TimestampMicros::now(),
            updated_at: TimestampMicros::now(),
        }
    }

    fn raw(ts: i64, lvl: &str) -> RawEvent {
        let mut fields = serde_json::Map::new();
        fields.insert("level".into(), json!(lvl));
        RawEvent {
            timestamp: TimestampMicros(ts),
            fields,
        }
    }

    #[tokio::test]
    async fn write_pipelines_wal_then_buffer() {
        let tmp = tempdir().unwrap();
        let wal = Arc::new(WalPool::new(
            tmp.path(),
            64 * 1024,
            crate::infra::segment_wal::FsyncPolicy::none_default(),
            Arc::new(crate::infra::segment_wal::StaticTermSource(1)),
        ));
        let buffer = Arc::new(BufferPool::new());
        let stream = sample_stream();
        let streams = InMemStreamRepo::with(stream.clone());
        let sink = IngesterSink::new(wal.clone(), buffer.clone(), streams);

        let batch = IngestBatch {
            batch_id: Id::new(),
            org_id: stream.org_id.clone(),
            stream: stream.name.clone(),
            stream_type: stream.stream_type,
            events: vec![raw(1_000_000, "info"), raw(2_000_000, "warn")],
            received_at: TimestampMicros::now(),
        };
        let res = sink.write(batch).await.unwrap();
        assert_eq!(res.accepted, 2);
        assert_eq!(res.rejected, 0);

        // buffer 持有 2 行 + high_watermark_seq == 1（首批 seq=1）
        let buf = buffer.get_or_create(&stream).lock_owned().await;
        assert_eq!(buf.row_count(), 2);
        assert_eq!(buf.high_watermark_seq(), 1);
        assert_eq!(buf.accounted_size_bytes(), buffer.reserved_bytes());
        assert!(buffer.reserved_bytes() > 0);
    }

    #[tokio::test]
    async fn seq_increments_per_batch() {
        let tmp = tempdir().unwrap();
        let wal = Arc::new(WalPool::new(
            tmp.path(),
            64 * 1024,
            crate::infra::segment_wal::FsyncPolicy::none_default(),
            Arc::new(crate::infra::segment_wal::StaticTermSource(1)),
        ));
        let buffer = Arc::new(BufferPool::new());
        let stream = sample_stream();
        let streams = InMemStreamRepo::with(stream.clone());
        let sink = IngesterSink::new(wal.clone(), buffer.clone(), streams);

        for i in 0..3 {
            let batch = IngestBatch {
                batch_id: Id::new(),
                org_id: stream.org_id.clone(),
                stream: stream.name.clone(),
                stream_type: stream.stream_type,
                events: vec![raw(1_000_000 + i, "x")],
                received_at: TimestampMicros::now(),
            };
            sink.write(batch).await.unwrap();
        }
        let buf = buffer.get_or_create(&stream).lock_owned().await;
        assert_eq!(buf.row_count(), 3);
        assert_eq!(buf.high_watermark_seq(), 3, "third batch carries seq=3");
    }

    #[tokio::test]
    async fn memory_rejection_happens_before_wal_or_buffer_mutation() {
        let tmp = tempdir().unwrap();
        let wal = Arc::new(WalPool::new(
            tmp.path(),
            64 * 1024,
            crate::infra::segment_wal::FsyncPolicy::none_default(),
            Arc::new(crate::infra::segment_wal::StaticTermSource(1)),
        ));
        let buffer = Arc::new(BufferPool::with_memory_limit_bytes(1));
        let stream = sample_stream();
        let streams = InMemStreamRepo::with(stream.clone());
        let sink = IngesterSink::new(wal.clone(), buffer.clone(), streams);
        let batch = IngestBatch {
            batch_id: Id::new(),
            org_id: stream.org_id.clone(),
            stream: stream.name.clone(),
            stream_type: stream.stream_type,
            events: vec![raw(1_000_000, "too-large")],
            received_at: TimestampMicros::now(),
        };

        let error = sink.write(batch).await.unwrap_err();
        assert!(error.to_string().contains("buffer memory limit"));
        assert_eq!(buffer.reserved_bytes(), 0);
        assert!(buffer.snapshot_keys().is_empty());
        assert!(wal.recover().unwrap().is_empty());
    }
}
