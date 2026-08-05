// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! ingester buffer → parquet → object_store。
//!
//! 入口 [`ParquetWriter::flush`]：拿一个 `RecordBatch`（schema 已经包含 `_timestamp` 列），
//! 序列化成 parquet 字节流上传到 object_store，并产出 [`ParquetFileMeta`]（含 time range +
//! 行数 + size + 字段 min/max）。
//!
//! ParquetFileMeta 不在此处直接落库；由调用方决定何时持久化（通常紧跟一次 `put` 成功之后）。

use std::{collections::HashMap, sync::Arc};

use arrow::array::{Array, RecordBatch, StringArray};
use bytes::Bytes;
use object_store::{ObjectStore, ObjectStoreExt, PutPayload, path::Path};
use parquet::{
    arrow::ArrowWriter, basic::Compression, file::properties::WriterProperties,
    schema::types::ColumnPath,
};

use super::partition::{sort_for_storage, split_by_utc_hour};
use crate::{
    domain::{
        storage::{ParquetFileMeta, PhysicalDatasetKind},
        stream::StreamDefinition,
    },
    infra::search::tantivy_index::{TantivyArchive, TantivyArchiveBuilder},
    shared::{
        Error, Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

mod metadata;

use metadata::{
    object_key as object_key_for, timestamp_range as ts_range, zone_maps as min_max_for_indexed,
};

pub struct ParquetWriter {
    object_store: Arc<dyn ObjectStore>,
}

impl ParquetWriter {
    pub fn new(object_store: Arc<dyn ObjectStore>) -> Self {
        Self { object_store }
    }

    /// 把 `batch` 写为 parquet 并上传，返回构造好的 ParquetFileMeta（未落库）。
    pub async fn flush(
        &self,
        stream: &StreamDefinition,
        batch: RecordBatch,
    ) -> Result<ParquetFileMeta> {
        self.flush_to_store(self.object_store.as_ref(), stream, batch)
            .await
    }

    /// 同上但显式指定目标 store（caller 自行传入）。
    pub async fn flush_to_store(
        &self,
        store: &dyn ObjectStore,
        stream: &StreamDefinition,
        batch: RecordBatch,
    ) -> Result<ParquetFileMeta> {
        let (meta, _) = self.flush_with_index_to_store(store, stream, batch).await?;
        Ok(meta)
    }

    /// 同时写 parquet + tantivy 索引：
    /// - parquet 上传后产 ParquetFileMeta；
    /// - 若 stream 中有 `indexed=true && Utf8/Json` 字段，构建 tantivy 索引并返回 archive。
    ///   archive 的对象上传由 caller 在同一 await 链路完成（规范路径映射为 `.ttv` sidecar）。
    pub async fn flush_with_index(
        &self,
        stream: &StreamDefinition,
        batch: RecordBatch,
    ) -> Result<(ParquetFileMeta, Option<TantivyArchive>)> {
        self.flush_with_index_to_store(self.object_store.as_ref(), stream, batch)
            .await
    }

    /// 一个 buffer generation 可能覆盖多个 UTC 小时。这里先按 `_timestamp` 拆分，再把
    /// 每个小时写成独立 parquet + index；任何一个分区失败都会清理本轮已写输出。
    pub async fn flush_partitioned_with_index(
        &self,
        stream: &StreamDefinition,
        dataset_kind: PhysicalDatasetKind,
        batch: RecordBatch,
    ) -> Result<Vec<(ParquetFileMeta, Option<TantivyArchive>)>> {
        self.flush_partitioned_with_index_to_store(
            self.object_store.as_ref(),
            stream,
            dataset_kind,
            batch,
        )
        .await
    }

    pub async fn flush_partitioned_with_index_to_store(
        &self,
        store: &dyn ObjectStore,
        stream: &StreamDefinition,
        dataset_kind: PhysicalDatasetKind,
        batch: RecordBatch,
    ) -> Result<Vec<(ParquetFileMeta, Option<TantivyArchive>)>> {
        let partitions = split_by_utc_hour(&batch)?;
        if partitions.is_empty() {
            return Err(Error::invalid("parquet flush called with empty batch"));
        }

        let mut outputs = Vec::with_capacity(partitions.len());
        for partition in partitions {
            match self
                .flush_dataset_with_index_to_store(store, stream, dataset_kind, partition)
                .await
            {
                Ok(output) => outputs.push(output),
                Err(error) => {
                    self.delete_output_set_from_store(store, &outputs).await;
                    return Err(error);
                }
            }
        }
        Ok(outputs)
    }

    #[tracing::instrument(
        name = "parquet.flush",
        skip_all,
        fields(
            otel.kind = "internal",
            molesignal.parquet.rows = batch.num_rows(),
            molesignal.stream.type = ?stream.stream_type
        )
    )]
    pub async fn flush_with_index_to_store(
        &self,
        store: &dyn ObjectStore,
        stream: &StreamDefinition,
        batch: RecordBatch,
    ) -> Result<(ParquetFileMeta, Option<TantivyArchive>)> {
        self.flush_dataset_with_index_to_store(store, stream, PhysicalDatasetKind::Raw, batch)
            .await
    }

    pub async fn flush_dataset_with_index_to_store(
        &self,
        store: &dyn ObjectStore,
        stream: &StreamDefinition,
        dataset_kind: PhysicalDatasetKind,
        batch: RecordBatch,
    ) -> Result<(ParquetFileMeta, Option<TantivyArchive>)> {
        if batch.num_rows() == 0 {
            return Err(Error::invalid("parquet flush called with empty batch"));
        }

        let batch = sort_for_storage(batch, dataset_kind)?;

        let (start_us, end_us) = ts_range(&batch)?;
        let start_hour = crate::domain::storage::hour_start_micros(TimestampMicros(start_us));
        let end_hour = crate::domain::storage::hour_start_micros(TimestampMicros(end_us));
        if start_hour != end_hour {
            return Err(Error::invalid(format!(
                "parquet file crosses UTC hour boundary: {start_us}..{end_us}"
            )));
        }
        let object_key = object_key_for(
            &stream.org_id,
            &stream.name,
            stream.stream_type,
            dataset_kind,
            start_us,
        );

        // 1. 序列化 parquet
        let bytes = encode_parquet(stream, &batch)?;
        let size_bytes = bytes.len() as u64;
        let rows = batch.num_rows() as u64;

        // 2. 构建 tantivy 索引（并发与 parquet 同步：实际同步执行；可后续优化）
        let archive = build_tantivy_for_batch(stream, &batch, &object_key)?;

        // 3. 上传 parquet
        store
            .put(&Path::from(object_key.clone()), PutPayload::from(bytes))
            .await
            .map_err(|e| Error::internal(format!("object_store put parquet: {e}")))?;

        // 4. 上传 tantivy archive（若有），与 parquet 同一链路
        if let Some(arc) = &archive
            && let Err(upload_error) = store
                .put(
                    &Path::from(arc.object_key.clone()),
                    PutPayload::from(arc.bytes.clone()),
                )
                .await
        {
            let original =
                Error::internal(format!("object_store put tantivy archive: {upload_error}"));
            if let Err(cleanup_error) = store.delete(&Path::from(object_key.clone())).await {
                tracing::warn!(
                    parquet = %object_key,
                    error = %cleanup_error,
                    "failed to delete parquet after tantivy upload failure"
                );
            }
            return Err(original);
        }

        let (min_values, max_values) = min_max_for_indexed(stream, &batch);
        let meta = ParquetFileMeta {
            id: Id::new(),
            org_id: stream.org_id.clone(),
            stream: stream.name.clone(),
            stream_type: stream.stream_type,
            dataset_kind,
            object_key,
            time_range: TimeRange::new(TimestampMicros(start_us), TimestampMicros(end_us)),
            rows,
            size_bytes,
            min_values,
            max_values,
            deleted: false,
        };
        Ok((meta, archive))
    }

    /// ParquetFileMeta 提交失败后尽力删除本次 flush 已上传的所有输出。
    ///
    /// 两个 delete 都会尝试；返回错误仅供调用方告警，不能覆盖原始数据库错误。
    pub async fn delete_outputs(
        &self,
        parquet_object_key: &str,
        tantivy_object_key: Option<&str>,
    ) -> Result<()> {
        let mut failures = Vec::new();
        for key in std::iter::once(parquet_object_key).chain(tantivy_object_key) {
            if let Err(error) = self.object_store.delete(&Path::from(key)).await {
                failures.push(format!("{key}: {error}"));
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(Error::internal(format!(
                "delete orphan flush outputs: {}",
                failures.join("; ")
            )))
        }
    }

    async fn delete_output_set_from_store(
        &self,
        store: &dyn ObjectStore,
        outputs: &[(ParquetFileMeta, Option<TantivyArchive>)],
    ) {
        for (meta, archive) in outputs {
            for key in std::iter::once(meta.object_key.as_str())
                .chain(archive.as_ref().map(|archive| archive.object_key.as_str()))
            {
                if let Err(error) = store.delete(&Path::from(key)).await {
                    tracing::warn!(object_key = key, %error, "failed to clean partial flush output");
                }
            }
        }
    }

    /// 写一个降采样产物 parquet，object_key 以 `.ds.parquet` 结尾作标记（compactor 据此
    /// 不再把它卷入小文件合并 / 重复降采样）。降采样针对 metrics（无 indexed 文本字段），
    /// 故不写 tantivy sidecar。schema 与原始文件一致 → 查询端同一权威 schema 投影可同读。
    pub async fn flush_downsampled_to_store(
        &self,
        store: &dyn ObjectStore,
        stream: &StreamDefinition,
        batch: RecordBatch,
    ) -> Result<ParquetFileMeta> {
        if batch.num_rows() == 0 {
            return Err(Error::invalid("downsample flush called with empty batch"));
        }
        let batch = sort_for_storage(batch, PhysicalDatasetKind::MetricRollup)?;
        let (start_us, end_us) = ts_range(&batch)?;
        if crate::domain::storage::hour_start_micros(TimestampMicros(start_us))
            != crate::domain::storage::hour_start_micros(TimestampMicros(end_us))
        {
            return Err(Error::invalid(
                "downsample parquet must not cross a UTC hour boundary",
            ));
        }
        let object_key = downsampled_key(object_key_for(
            &stream.org_id,
            &stream.name,
            stream.stream_type,
            PhysicalDatasetKind::MetricRollup,
            start_us,
        ));
        let bytes = encode_parquet(stream, &batch)?;
        let size_bytes = bytes.len() as u64;
        let rows = batch.num_rows() as u64;
        store
            .put(&Path::from(object_key.clone()), PutPayload::from(bytes))
            .await
            .map_err(|e| Error::internal(format!("object_store put downsampled parquet: {e}")))?;
        let (min_values, max_values) = min_max_for_indexed(stream, &batch);
        Ok(ParquetFileMeta {
            id: Id::new(),
            org_id: stream.org_id.clone(),
            stream: stream.name.clone(),
            stream_type: stream.stream_type,
            dataset_kind: PhysicalDatasetKind::MetricRollup,
            object_key,
            time_range: TimeRange::new(TimestampMicros(start_us), TimestampMicros(end_us)),
            rows,
            size_bytes,
            min_values,
            max_values,
            deleted: false,
        })
    }
}

/// `{...}.parquet` → `{...}.ds.parquet`（降采样标记）。
pub fn downsampled_key(key: String) -> String {
    match key.strip_suffix(".parquet") {
        Some(stem) => format!("{stem}.ds.parquet"),
        None => format!("{key}.ds"),
    }
}

/// object_key 是否为降采样产物。
pub fn is_downsampled_key(key: &str) -> bool {
    key.ends_with(".ds.parquet")
}

/// 把 batch 中的 indexed Utf8 列喂给 [`TantivyArchiveBuilder`] → archive bytes + object key。
fn build_tantivy_for_batch(
    stream: &StreamDefinition,
    batch: &RecordBatch,
    parquet_object_key: &str,
) -> Result<Option<TantivyArchive>> {
    let mut builder = match TantivyArchiveBuilder::try_new(stream)
        .map_err(|e| Error::internal(format!("tantivy builder: {e}")))?
    {
        Some(b) => b,
        None => return Ok(None),
    };
    let schema = batch.schema();
    let mut indexed_arrays: Vec<(String, &StringArray)> = Vec::new();
    for f in &stream.schema.fields {
        if !f.indexed {
            continue;
        }
        if let Ok(idx) = schema.index_of(&f.name)
            && let Some(a) = batch.column(idx).as_any().downcast_ref::<StringArray>()
        {
            indexed_arrays.push((f.name.clone(), a));
        }
    }
    if indexed_arrays.is_empty() {
        return Ok(None);
    }
    for row in 0..batch.num_rows() {
        let mut values: HashMap<&str, &str> = HashMap::new();
        for (name, arr) in &indexed_arrays {
            if !arr.is_null(row) {
                values.insert(name.as_str(), arr.value(row));
            }
        }
        builder
            .add_doc(&values)
            .map_err(|e| Error::internal(format!("tantivy add_doc: {e}")))?;
    }
    let bytes = builder
        .commit_and_archive()
        .map_err(|e| Error::internal(format!("tantivy commit_and_archive: {e}")))?;
    // `key_for` 只接受规范的 dataset + UTC 小时分区路径并映射为 `.ttv`，
    // 不符合约定时返 None → 静默丢弃 archive（保证 caller 不写出未知 sidecar）。
    let object_key = match TantivyArchive::key_for(parquet_object_key) {
        Some(k) => k,
        None => {
            tracing::warn!(
                parquet = %parquet_object_key,
                "tantivy sidecar skipped: parquet key does not match canonical layout"
            );
            return Ok(None);
        }
    };
    Ok(Some(TantivyArchive { object_key, bytes }))
}

fn encode_parquet(stream: &StreamDefinition, batch: &RecordBatch) -> Result<Bytes> {
    let mut properties = WriterProperties::builder().set_compression(Compression::SNAPPY);
    for field in stream
        .schema
        .fields
        .iter()
        .filter(|field| field.indexed && field.exact && !field.encrypted)
    {
        if batch.schema().index_of(&field.name).is_ok() {
            properties = properties
                .set_column_bloom_filter_enabled(ColumnPath::from(field.name.clone()), true)
                .set_column_bloom_filter_fpp(ColumnPath::from(field.name.clone()), 0.01);
        }
    }
    let props = properties.build();
    let mut buf: Vec<u8> = Vec::with_capacity(64 * 1024);
    {
        let mut w = ArrowWriter::try_new(&mut buf, batch.schema(), Some(props))
            .map_err(|e| Error::internal(format!("ArrowWriter::try_new: {e}")))?;
        w.write(batch)
            .map_err(|e| Error::internal(format!("ArrowWriter::write: {e}")))?;
        w.close()
            .map_err(|e| Error::internal(format!("ArrowWriter::close: {e}")))?;
    }
    Ok(Bytes::from(buf))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::array::{Int64Array, StringArray, TimestampMicrosecondArray};
    use object_store::local::LocalFileSystem;

    use super::*;
    use crate::{
        domain::stream::{FieldDef, FieldType, Retention, Schema, StreamType},
        infra::storage::arrow_schema::to_arrow,
    };

    fn sample_stream() -> StreamDefinition {
        StreamDefinition {
            id: Id::new(),
            org_id: Id::from_string("org-1"),
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
        let ts = TimestampMicrosecondArray::from(vec![1_000_000, 2_000_000, 3_000_000])
            .with_timezone("UTC");
        let level = StringArray::from(vec!["info", "warn", "error"]);
        let latency = Int64Array::from(vec![Some(10), Some(20), None]);
        RecordBatch::try_new(
            schema,
            vec![Arc::new(ts), Arc::new(level), Arc::new(latency)],
        )
        .unwrap()
    }

    #[tokio::test]
    async fn flush_writes_parquet_and_returns_meta() {
        let tmp = tempfile::tempdir().expect("tmpdir");
        let fs: Arc<dyn ObjectStore> =
            Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());

        let writer = ParquetWriter::new(fs.clone());
        let stream = sample_stream();
        let batch = sample_batch(&stream);

        let meta = writer.flush(&stream, batch).await.expect("flush");
        assert_eq!(meta.rows, 3);
        assert_eq!(meta.time_range.start.0, 1_000_000);
        assert_eq!(meta.time_range.end.0, 3_000_000);
        assert_eq!(meta.min_values.get("level").unwrap(), "error");
        assert_eq!(meta.max_values.get("level").unwrap(), "warn");
        assert_eq!(meta.min_values.get("latency_ms").unwrap(), 10);
        assert_eq!(meta.max_values.get("latency_ms").unwrap(), 20);
        // 物理文件确实存在
        let local_path = tmp.path().join(&meta.object_key);
        assert!(
            local_path.exists(),
            "file at {} missing",
            local_path.display()
        );
    }
}
