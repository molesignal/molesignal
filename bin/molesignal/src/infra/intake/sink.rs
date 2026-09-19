// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! DurableIntakeSink：真实 IntakeSink 实现（替换 `MemoryIntakeSink`）。
//!
//! 入口语义：
//! 1. 用 batch 的 `(org, stream_type, stream)` 找 [`StreamDefinition`]（schema 已由
//!    [`IntakeService`] 演化好），再经 [`DatasetResolver`] 解析出物理数据集。
//! 2. 串行先写 WAL（durable）后写 buffer（内存）：
//!    - 整批 JSON 序列化 → [`WalPool::append`]（按 dataset 键入独立 WAL，
//!      sequence 由 WAL 分配返回）
//!    - 逐条 [`BufferPool::push`]，并把高水位 `seq` 记入 buffer
//! 3. 返 `IntakeResult { accepted, rejected: 0, errors: [] }`。

use std::sync::Arc;

use async_trait::async_trait;

use super::{BufferPool, BufferWriter, DatasetResolver, WalPool, physical_schema};
use crate::{
    domain::{
        intake::{IntakeBatch, IntakeResult, IntakeSink},
        storage::{DatasetTypeId, WriterNodeId},
        stream::StreamRepository,
    },
    infra::cipher::FieldKeyService,
    shared::{Error, Result},
};

pub struct DurableIntakeSink {
    wal: Arc<WalPool>,
    buffer: Arc<BufferPool>,
    streams: Arc<dyn StreamRepository>,
    datasets: Arc<DatasetResolver>,
    /// 字段加密 DEK 服务；`None` 时不解析 DEK（无加密字段的流不受影响）。
    field_keys: Option<Arc<FieldKeyService>>,
}

impl DurableIntakeSink {
    pub fn new(
        wal: Arc<WalPool>,
        buffer: Arc<BufferPool>,
        streams: Arc<dyn StreamRepository>,
        datasets: Arc<DatasetResolver>,
    ) -> Self {
        Self {
            wal,
            buffer,
            streams,
            datasets,
            field_keys: None,
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

    async fn write_inner(
        &self,
        dataset_type: DatasetTypeId,
        batch: IntakeBatch,
    ) -> Result<IntakeResult> {
        if batch.events.is_empty() {
            return Ok(IntakeResult {
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

        let resolved = self.datasets.resolve(&stream, dataset_type.clone()).await?;
        let payload = serde_json::to_vec(&batch)
            .map_err(|e| Error::internal(format!("intake serialize: {e}")))?;
        let reservation = self.buffer.try_reserve(stream.stream_type, payload.len())?;
        let buffer = self
            .buffer
            .get_or_create_dataset(&stream, resolved.clone())?;

        // This mutex is the per-dataset ordering discipline shared with flush rotation. Holding it
        // across append + push closes the historical window where a flush high-watermark could
        // cover a WAL record that had not entered the memory buffer yet.
        let mut guard = buffer.records().lock().await;
        guard.sync_schema(&physical_schema::project(&stream, &dataset_type));
        let position = self
            .wal
            .append_position(&resolved.wal_identity(), payload)
            .await
            .map_err(|e| Error::internal(format!("wal append: {e}")))?;
        if let Some(dek) = field_dek {
            guard.set_field_key(dek);
        }
        guard.add_accounted_bytes(reservation.commit());
        let accepted = batch.events.len();
        let writer =
            BufferWriter::new(WriterNodeId::new(self.wal.node_id()), position.writer_epoch);
        let buffer_span = tracing::info_span!(
            "intake.buffer",
            otel.kind = "internal",
            molesignal.intake.event_count = accepted,
            molesignal.dataset.type = resolved.dataset.dataset_type.as_str()
        );
        buffer_span.in_scope(|| {
            for event in &batch.events {
                guard
                    .push_with_position(event, &writer, position.sequence)
                    .map_err(|e| Error::internal(format!("buffer push: {e}")))?;
            }
            Ok::<(), Error>(())
        })?;

        Ok(IntakeResult {
            accepted,
            rejected: 0,
            errors: Vec::new(),
        })
    }
}

#[async_trait]
impl IntakeSink for DurableIntakeSink {
    fn supports_derived_datasets(&self) -> bool {
        true
    }

    fn primary_dataset_type(
        &self,
        stream_type: crate::domain::stream::StreamType,
    ) -> Result<DatasetTypeId> {
        self.datasets.primary_dataset_type(stream_type)
    }

    #[tracing::instrument(
        name = "intake.persist",
        skip_all,
        fields(
            otel.kind = "internal",
            molesignal.intake.event_count = batch.events.len(),
            molesignal.stream.type = ?batch.stream_type
        )
    )]
    async fn write(&self, batch: IntakeBatch) -> Result<IntakeResult> {
        self.write_inner(self.primary_dataset_type(batch.stream_type)?, batch)
            .await
    }

    async fn write_dataset(
        &self,
        dataset_type: DatasetTypeId,
        batch: IntakeBatch,
    ) -> Result<IntakeResult> {
        self.write_inner(dataset_type, batch).await
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Mutex as StdMutex};

    use arrow::array::Array;
    use async_trait::async_trait;
    use serde_json::json;
    use tempfile::tempdir;

    use super::*;
    use crate::{
        domain::{
            intake::RawEvent,
            storage::primary_dataset_type,
            stream::{FieldDef, FieldType, Retention, Schema, StreamDefinition, StreamType},
        },
        infra::intake::{BufferPool, WalPool, dataset_resolver::test_support::test_resolver},
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
            id: &Id,
            schema: crate::domain::stream::Schema,
        ) -> Result<()> {
            let mut streams = self.inner.lock().unwrap();
            let stream = streams
                .values_mut()
                .find(|stream| stream.id == *id)
                .ok_or_else(|| Error::not_found(format!("stream {id}")))?;
            stream.schema = schema;
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
            stream_type: StreamType::LOGS,
            schema: Schema {
                fields: vec![FieldDef {
                    name: "level".into(),
                    data_type: FieldType::Utf8,
                    nullable: false,
                    index_type: None,
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
            "node-test",
            64 * 1024,
            crate::infra::segment_wal::FsyncPolicy::none_default(),
        ));
        let buffer = Arc::new(BufferPool::new());
        let stream = sample_stream();
        let streams = InMemStreamRepo::with(stream.clone());
        let resolver = test_resolver();
        let sink = DurableIntakeSink::new(wal.clone(), buffer.clone(), streams, resolver.clone());

        let batch = IntakeBatch {
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
        let dataset = resolver
            .resolve(&stream, primary_dataset_type(stream.stream_type).unwrap())
            .await
            .unwrap();
        let dataset_buffer = buffer.get_or_create_dataset(&stream, dataset).unwrap();
        let buf = dataset_buffer.records().lock().await;
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
            "node-test",
            64 * 1024,
            crate::infra::segment_wal::FsyncPolicy::none_default(),
        ));
        let buffer = Arc::new(BufferPool::new());
        let stream = sample_stream();
        let streams = InMemStreamRepo::with(stream.clone());
        let resolver = test_resolver();
        let sink = DurableIntakeSink::new(wal.clone(), buffer.clone(), streams, resolver.clone());

        for i in 0..3 {
            let batch = IntakeBatch {
                batch_id: Id::new(),
                org_id: stream.org_id.clone(),
                stream: stream.name.clone(),
                stream_type: stream.stream_type,
                events: vec![raw(1_000_000 + i, "x")],
                received_at: TimestampMicros::now(),
            };
            sink.write(batch).await.unwrap();
        }
        let dataset = resolver
            .resolve(&stream, primary_dataset_type(stream.stream_type).unwrap())
            .await
            .unwrap();
        let dataset_buffer = buffer.get_or_create_dataset(&stream, dataset).unwrap();
        let buf = dataset_buffer.records().lock().await;
        assert_eq!(buf.row_count(), 3);
        assert_eq!(buf.high_watermark_seq(), 3, "third batch carries seq=3");
    }

    #[tokio::test]
    async fn existing_dataset_buffer_tracks_stream_schema_evolution() {
        let tmp = tempdir().unwrap();
        let wal = Arc::new(WalPool::new(
            tmp.path(),
            "node-test",
            64 * 1024,
            crate::infra::segment_wal::FsyncPolicy::none_default(),
        ));
        let buffer = Arc::new(BufferPool::new());
        let stream = sample_stream();
        let streams = InMemStreamRepo::with(stream.clone());
        let resolver = test_resolver();
        let sink = DurableIntakeSink::new(wal, buffer.clone(), streams.clone(), resolver.clone());

        sink.write(IntakeBatch {
            batch_id: Id::new(),
            org_id: stream.org_id.clone(),
            stream: stream.name.clone(),
            stream_type: stream.stream_type,
            events: vec![raw(1_000_000, "before")],
            received_at: TimestampMicros::now(),
        })
        .await
        .unwrap();

        let mut evolved = stream.clone();
        evolved.schema.fields.push(FieldDef {
            name: "user_id".into(),
            data_type: FieldType::Utf8,
            nullable: true,
            index_type: None,
            indexed: false,
            encrypted: false,
            exact: false,
        });
        streams
            .update_schema(&stream.id, evolved.schema.clone())
            .await
            .unwrap();
        let mut fields = serde_json::Map::new();
        fields.insert("level".into(), json!("after"));
        fields.insert("user_id".into(), json!("user-1"));
        sink.write(IntakeBatch {
            batch_id: Id::new(),
            org_id: stream.org_id.clone(),
            stream: stream.name.clone(),
            stream_type: stream.stream_type,
            events: vec![RawEvent {
                timestamp: TimestampMicros(2_000_000),
                fields,
            }],
            received_at: TimestampMicros::now(),
        })
        .await
        .unwrap();

        let dataset = resolver
            .resolve(&evolved, primary_dataset_type(evolved.stream_type).unwrap())
            .await
            .unwrap();
        let snapshots = buffer.snapshot_dataset(&dataset.dataset.id).await.unwrap();
        assert_eq!(snapshots.len(), 1);
        let users = snapshots[0]
            .batch
            .column_by_name("user_id")
            .unwrap()
            .as_any()
            .downcast_ref::<arrow::array::StringArray>()
            .unwrap();
        assert!(users.is_null(0));
        assert_eq!(users.value(1), "user-1");
    }

    #[tokio::test]
    async fn memory_rejection_happens_before_wal_or_buffer_mutation() {
        let tmp = tempdir().unwrap();
        let wal = Arc::new(WalPool::new(
            tmp.path(),
            "node-test",
            64 * 1024,
            crate::infra::segment_wal::FsyncPolicy::none_default(),
        ));
        let buffer = Arc::new(BufferPool::with_memory_limit_bytes(1));
        let stream = sample_stream();
        let streams = InMemStreamRepo::with(stream.clone());
        let sink = DurableIntakeSink::new(wal.clone(), buffer.clone(), streams, test_resolver());
        let batch = IntakeBatch {
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
        assert!(wal.recovery_sources().unwrap().is_empty());
    }
}
