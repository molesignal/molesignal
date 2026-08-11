// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 从 object_store 拉 parquet 并解析为 Arrow `RecordBatch`。
//!
//! 主要服务于 querier 的扫描路径。[`ReadOptions`] 把时间窗裁剪与列投影收在一处，
//! 两者可组合——先前它们分属两个方法，「按时间裁剪 + 只读几列」表达不出来。
//!
//! ```ignore
//! let batches = reader
//!     .read_from_store(
//!         store,
//!         &fm.object_key,
//!         ReadOptions::new().with_time_range(start_us, end_us),
//!     )
//!     .await?;
//! ```
//!
//! 需要全量的调用方可物化为 `Vec<RecordBatch>`；Top-K / Cursor 路径
//! 使用 [`ParquetReader::stream_from_store`] 逐 batch 消费并可提前停止。

use std::sync::Arc;

use arrow::{array::RecordBatch, datatypes::SchemaRef};
use futures::{StreamExt, stream::BoxStream};
use object_store::{Error as OsError, ObjectStore, ObjectStoreExt, path::Path};
use parquet::{
    arrow::{ParquetRecordBatchStreamBuilder, ProjectionMask, async_reader::ParquetObjectReader},
    file::statistics::Statistics,
};

use crate::shared::{Error, Result};

fn head_err(object_key: &str, e: OsError) -> Error {
    match e {
        OsError::NotFound { .. } => {
            Error::not_found(format!("object_store head: {object_key} not found"))
        }
        other => Error::internal(format!("object_store head: {other}")),
    }
}

/// 单次读取的裁剪选项。默认（[`ReadOptions::default`]）等价于「全文件全列」。
#[derive(Default, Clone, Copy)]
pub struct ReadOptions<'a> {
    /// 按 `_timestamp` 的 row-group min/max 统计裁剪，窗口语义 `[start_us, end_us)`，
    /// 与查询侧的时间过滤一致。统计缺失或 schema 含嵌套列时保守保留全部。
    time_range: Option<(i64, i64)>,
    /// 只解码这些列。
    columns: Option<&'a [&'a str]>,
    /// 已知的对象字节数，用于省掉一次 `head()` 往返。
    ///
    /// **必须与对象实际大小一致**：parquet 的 footer 靠文件尾定位，size 不对会直接读失败。
    /// 唯一可信来源是 `ParquetFileMeta.size_bytes`——它由 `ParquetWriter` 在上传时记成
    /// `bytes.len()`。别的来源一律别传，让它 head。
    size_bytes: Option<u64>,
}

impl<'a> ReadOptions<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_time_range(mut self, start_us: i64, end_us: i64) -> Self {
        self.time_range = Some((start_us, end_us));
        self
    }

    pub fn with_columns(mut self, columns: &'a [&'a str]) -> Self {
        self.columns = Some(columns);
        self
    }

    /// 见 [`ReadOptions::size_bytes`] 的契约：只传 `ParquetFileMeta.size_bytes`。
    pub fn with_known_size(mut self, size_bytes: u64) -> Self {
        self.size_bytes = Some(size_bytes);
        self
    }
}

pub struct ParquetReader {
    object_store: Arc<dyn ObjectStore>,
}

impl ParquetReader {
    pub fn new(object_store: Arc<dyn ObjectStore>) -> Self {
        Self { object_store }
    }

    /// 默认 store 走 bootstrap 阶段注入的 fallback；显式 store 路径调 `read_all_from_store`。
    pub fn default_store(&self) -> Arc<dyn ObjectStore> {
        self.object_store.clone()
    }

    /// 打开一个 parquet 文件，返回全部 RecordBatch（内存物化）。
    /// 批量读完再返回；后续可改为流式 yield。
    pub async fn read_all(&self, object_key: &str) -> Result<Vec<RecordBatch>> {
        self.read_all_from_store(self.object_store.clone(), object_key)
            .await
    }

    /// caller 显式传入目标 store。
    pub async fn read_all_from_store(
        &self,
        store: Arc<dyn ObjectStore>,
        object_key: &str,
    ) -> Result<Vec<RecordBatch>> {
        self.read_from_store(store, object_key, ReadOptions::new())
            .await
    }

    /// 按 `_timestamp` 的 row-group 统计裁剪后读取。文件级时间裁剪由元数据库完成；
    /// 这里进一步跳过边界文件内与窗口不相交的 row group，减少解码量。
    pub async fn read_time_range(
        &self,
        object_key: &str,
        start_us: i64,
        end_us: i64,
    ) -> Result<Vec<RecordBatch>> {
        self.read_from_store(
            self.object_store.clone(),
            object_key,
            ReadOptions::new().with_time_range(start_us, end_us),
        )
        .await
    }

    /// 投影特定列读取。
    pub async fn read_projection(
        &self,
        object_key: &str,
        column_names: &[&str],
    ) -> Result<Vec<RecordBatch>> {
        self.read_from_store(
            self.object_store.clone(),
            object_key,
            ReadOptions::new().with_columns(column_names),
        )
        .await
    }

    /// 只读取 parquet footer 并返回 Arrow schema，不解码任何 row group。
    pub async fn schema_from_store(
        &self,
        store: Arc<dyn ObjectStore>,
        object_key: &str,
        size_bytes: u64,
    ) -> Result<SchemaRef> {
        let location = Path::from(object_key);
        let reader = ParquetObjectReader::new(store, location).with_file_size(size_bytes);
        let builder = ParquetRecordBatchStreamBuilder::new(reader)
            .await
            .map_err(|error| Error::internal(format!("parquet schema footer: {error}")))?;
        Ok(builder.schema().clone())
    }

    /// 核心读取路径：时间窗裁剪与列投影在同一次读里生效。
    pub async fn read_from_store(
        &self,
        store: Arc<dyn ObjectStore>,
        object_key: &str,
        opts: ReadOptions<'_>,
    ) -> Result<Vec<RecordBatch>> {
        let mut stream = self.stream_from_store(store, object_key, opts).await?;
        let mut out = Vec::new();
        while let Some(batch) = stream.next().await {
            out.push(batch?);
        }
        Ok(out)
    }

    /// 与 [`Self::read_from_store`] 相同的 projection / row-group pruning，但逐 batch
    /// yield，调用方达到 Top-K / page_size+1 后可以立即停止，不再解码文件余下内容。
    pub async fn stream_from_store(
        &self,
        store: Arc<dyn ObjectStore>,
        object_key: &str,
        opts: ReadOptions<'_>,
    ) -> Result<BoxStream<'static, Result<RecordBatch>>> {
        let path = Path::from(object_key);
        // 调用方给了权威 size（ParquetFileMeta）就直接用，省一次往返；否则 head 一次拿。
        let (location, size) = match opts.size_bytes {
            Some(size) => (path, size),
            None => {
                let meta = store
                    .head(&path)
                    .await
                    .map_err(|e| head_err(object_key, e))?;
                (meta.location, meta.size)
            }
        };
        let reader = ParquetObjectReader::new(store.clone(), location).with_file_size(size);
        let mut builder = ParquetRecordBatchStreamBuilder::new(reader)
            .await
            .map_err(|e| Error::internal(format!("parquet builder: {e}")))?;

        if let Some((start_us, end_us)) = opts.time_range {
            // arrow field 下标 == parquet leaf 下标仅对平铺 schema 成立（molesignal
            // 的流 schema 均为平铺）；不成立时跳过裁剪。
            let flat = builder.parquet_schema().num_columns() == builder.schema().fields().len();
            if let (true, Ok(ts_col)) = (flat, builder.schema().index_of("_timestamp")) {
                let keep: Vec<usize> = builder
                    .metadata()
                    .row_groups()
                    .iter()
                    .enumerate()
                    .filter_map(|(i, rg)| {
                        let overlaps = match rg.column(ts_col).statistics() {
                            Some(Statistics::Int64(s)) => match (s.min_opt(), s.max_opt()) {
                                (Some(&min), Some(&max)) => max >= start_us && min < end_us,
                                _ => true,
                            },
                            _ => true,
                        };
                        overlaps.then_some(i)
                    })
                    .collect();
                if keep.is_empty() {
                    return Ok(futures::stream::empty().boxed());
                }
                if keep.len() < builder.metadata().num_row_groups() {
                    builder = builder.with_row_groups(keep);
                }
            }
        }

        if let Some(column_names) = opts.columns {
            let schema = builder.parquet_schema().clone();
            let indices: Vec<usize> = column_names
                .iter()
                .filter_map(|name| schema.columns().iter().position(|c| c.name() == *name))
                .collect();
            builder = builder.with_projection(ProjectionMask::leaves(&schema, indices));
        }

        let stream = builder
            .build()
            .map_err(|e| Error::internal(format!("parquet stream: {e}")))?;
        Ok(stream
            .map(|batch| batch.map_err(|e| Error::internal(format!("parquet read: {e}"))))
            .boxed())
    }
}

#[cfg(test)]
mod tests {
    use arrow::array::{Int64Array, StringArray, TimestampMicrosecondArray};
    use object_store::local::LocalFileSystem;

    use super::*;
    use crate::{
        domain::stream::{FieldDef, FieldType, Retention, Schema, StreamDefinition, StreamType},
        infra::storage::{arrow_schema::to_arrow, parquet::writer::ParquetWriter},
        shared::{ids::Id, time::TimestampMicros},
    };

    fn sample() -> (StreamDefinition, RecordBatch) {
        let stream = StreamDefinition {
            id: Id::new(),
            org_id: Id::from_string("org-1"),
            name: "app".into(),
            stream_type: StreamType::Logs,
            schema: Schema {
                fields: vec![FieldDef {
                    name: "msg".into(),
                    data_type: FieldType::Utf8,
                    nullable: false,
                    indexed: false,
                    encrypted: false,
                    exact: false,
                }],
            },
            retention: Some(Retention { days: 30 }),
            created_at: TimestampMicros::now(),
            updated_at: TimestampMicros::now(),
        };
        let schema = to_arrow(&stream.schema);
        let ts = TimestampMicrosecondArray::from(vec![10_000, 20_000]).with_timezone("UTC");
        let msg = StringArray::from(vec!["hello", "world"]);
        let batch = RecordBatch::try_new(schema, vec![Arc::new(ts), Arc::new(msg)]).unwrap();
        (stream, batch)
    }

    #[tokio::test]
    async fn roundtrip_read_all() {
        let tmp = tempfile::tempdir().unwrap();
        let store: Arc<dyn ObjectStore> =
            Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
        let writer = ParquetWriter::new(store.clone());
        let reader = ParquetReader::new(store);

        let (stream, batch) = sample();
        let meta = writer.flush(&stream, batch).await.unwrap();

        let batches = reader.read_all(&meta.object_key).await.unwrap();
        let total: usize = batches.iter().map(|b| b.num_rows()).sum();
        assert_eq!(total, 2);
        // 列顺序应是 _timestamp + msg
        let schema = batches[0].schema();
        let names: Vec<&str> = schema.fields().iter().map(|f| f.name().as_str()).collect();
        assert_eq!(names, vec!["_timestamp", "msg"]);
        // 给 Int64Array 不报 unused
        let _ = Int64Array::from(vec![1_i64]);
    }

    #[tokio::test]
    async fn read_time_range_prunes_by_timestamp_stats() {
        let tmp = tempfile::tempdir().unwrap();
        let store: Arc<dyn ObjectStore> =
            Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
        let writer = ParquetWriter::new(store.clone());
        let reader = ParquetReader::new(store);

        // sample() 的 _timestamp 统计为 [10_000, 20_000]
        let (stream, batch) = sample();
        let meta = writer.flush(&stream, batch).await.unwrap();

        // 窗口与统计相交 → row group 保留（行级过滤在查询侧做，这里整组返回）
        let hit = reader
            .read_time_range(&meta.object_key, 0, 15_000)
            .await
            .unwrap();
        assert_eq!(hit.iter().map(|b| b.num_rows()).sum::<usize>(), 2);

        // [20_001, 30_000)：max=20_000 小于 start → 裁掉
        let after = reader
            .read_time_range(&meta.object_key, 20_001, 30_000)
            .await
            .unwrap();
        assert!(after.is_empty());

        // [0, 5_000)：min=10_000 不小于 end → 裁掉
        let before = reader
            .read_time_range(&meta.object_key, 0, 5_000)
            .await
            .unwrap();
        assert!(before.is_empty());
    }
}
