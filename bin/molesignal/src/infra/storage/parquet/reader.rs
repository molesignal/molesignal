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
//! 使用 [`ParquetReader::stream_from_store`] 逐 batch 消费并可提前停止。合并文件这类要同时
//! 读许多文件的调用方用 [`ParquetReader::stream_windows_from_store`]，每个文件只占一个窗口的
//! 内存。

use std::{ops::Range, sync::Arc};

use arrow::{array::RecordBatch, datatypes::SchemaRef};
use bytes::Bytes;
use futures::{
    FutureExt, StreamExt, TryStreamExt,
    future::BoxFuture,
    stream::{self, BoxStream},
};
use object_store::{Error as OsError, ObjectStore, ObjectStoreExt, path::Path};
use parquet::{
    arrow::{
        ParquetRecordBatchStreamBuilder, ProjectionMask,
        arrow_reader::{ArrowReaderMetadata, ArrowReaderOptions, RowSelection, RowSelector},
        async_reader::AsyncFileReader,
    },
    errors::ParquetError,
    file::metadata::{PageIndexPolicy, ParquetMetaData, ParquetMetaDataReader},
};

use crate::shared::{Error, Result};

mod pages;

use pages::RowTest;

/// Footer bytes fetched with the first request: the Thrift footer and the page index before it
/// usually fit, so the metadata loads in one request.
const FOOTER_READ_BYTES: usize = 512 * 1024;

/// [`AsyncFileReader`] over one immutable object whose size is known.
#[derive(Clone)]
struct ObjectFileReader {
    store: Arc<dyn ObjectStore>,
    path: Path,
    size: u64,
}

impl ObjectFileReader {
    fn new(store: Arc<dyn ObjectStore>, path: Path, size: u64) -> Self {
        Self { store, path, size }
    }
}

fn parquet_error(error: OsError) -> ParquetError {
    ParquetError::External(Box::new(error))
}

impl AsyncFileReader for ObjectFileReader {
    fn get_bytes(&mut self, range: Range<u64>) -> BoxFuture<'_, parquet::errors::Result<Bytes>> {
        async move {
            self.store
                .get_range(&self.path, range)
                .await
                .map_err(parquet_error)
        }
        .boxed()
    }

    fn get_byte_ranges(
        &mut self,
        ranges: Vec<Range<u64>>,
    ) -> BoxFuture<'_, parquet::errors::Result<Vec<Bytes>>> {
        async move {
            self.store
                .get_ranges(&self.path, &ranges)
                .await
                .map_err(parquet_error)
        }
        .boxed()
    }

    fn get_metadata<'a>(
        &'a mut self,
        options: Option<&'a ArrowReaderOptions>,
    ) -> BoxFuture<'a, parquet::errors::Result<Arc<ParquetMetaData>>> {
        async move {
            let mut reader = ParquetMetaDataReader::new()
                .with_metadata_options(options.map(|options| options.metadata_options().clone()))
                .with_prefetch_hint(Some(FOOTER_READ_BYTES));

            // Page indexes load only when the reader options ask for them.
            if let Some(options) = options
                && (options.column_index_policy() != PageIndexPolicy::Skip
                    || options.offset_index_policy() != PageIndexPolicy::Skip)
            {
                reader = reader
                    .with_column_index_policy(options.column_index_policy())
                    .with_offset_index_policy(options.offset_index_policy());
            }

            let size = self.size;
            Ok(Arc::new(reader.load_and_finish(self, size).await?))
        }
        .boxed()
    }
}

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
    /// 只读可能含这个值的 row group 与页：`(列, 值)`。读到的行里仍可能有别的值，调用方自己过滤。
    equals: Option<(&'a str, &'a str)>,
    /// 已知的对象字节数，用于省掉一次 `head()` 往返。
    ///
    /// **必须与对象实际大小一致**：parquet 的 footer 靠文件尾定位，size 不对会直接读失败。
    /// 唯一可信来源是 Catalog `Artifact.object.size_bytes`（过渡查询投影里的
    /// `QueryFile.size_bytes` 来自同一字段）。别的来源一律别传，让它 head。
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

    /// 按 row group 统计与页索引跳过不可能含 `value` 的部分；文件按 `column` 排过序时，读到的
    /// 基本只有这个值的行。时间窗同样按页裁剪。
    pub fn with_equals(mut self, column: &'a str, value: &'a str) -> Self {
        self.equals = Some((column, value));
        self
    }

    /// 见 [`ReadOptions::size_bytes`] 的契约：只传 Catalog 记录的对象大小。
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
        let reader = ObjectFileReader::new(store, Path::from(object_key), size_bytes);

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

        // 调用方给了 Catalog 权威 size 就直接用，省一次往返；否则 head 一次拿。
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

        let reader = ObjectFileReader::new(store.clone(), location, size);

        // 页索引说明哪些页可能含要找的值；它通常和 footer 在同一次读里。
        let options = if opts.equals.is_some() {
            ArrowReaderOptions::new().with_page_index_policy(PageIndexPolicy::Optional)
        } else {
            ArrowReaderOptions::new()
        };

        let mut builder = ParquetRecordBatchStreamBuilder::new_with_options(reader, options)
            .await
            .map_err(|e| Error::internal(format!("parquet builder: {e}")))?;

        // arrow field 下标 == parquet leaf 下标仅对平铺 schema 成立（molesignal 的流 schema 均为
        // 平铺）；不成立时不裁剪。
        let flat = builder.parquet_schema().num_columns() == builder.schema().fields().len();
        let mut tests = Vec::new();

        if flat
            && let Some((start_us, end_us)) = opts.time_range
            && let Ok(column) = builder.schema().index_of("_timestamp")
        {
            tests.push((column, RowTest::Within(start_us, end_us)));
        }

        if flat
            && let Some((name, value)) = opts.equals
            && let Ok(column) = builder.schema().index_of(name)
        {
            tests.push((column, RowTest::Equals(value.as_bytes())));
        }

        if !tests.is_empty() {
            let (row_groups, selection) = pages::select(builder.metadata(), &tests);

            if row_groups.is_empty() {
                return Ok(futures::stream::empty().boxed());
            }

            if row_groups.len() < builder.metadata().num_row_groups() {
                builder = builder.with_row_groups(row_groups);
            }

            if let Some(selection) = selection {
                builder = builder.with_row_selection(selection);
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

    /// 按行窗口逐段读整个对象：全部列、全部行，保持文件里的行序。每段只取窗口覆盖的数据页
    /// （借助 offset index），一个对象同一时刻读进内存的压缩字节约为 `window_bytes`，与它的
    /// row group 多大无关；没有 offset index 的 row group 整个读。对象不存在时返回
    /// `Error::NotFound`。
    pub async fn stream_windows_from_store(
        &self,
        store: Arc<dyn ObjectStore>,
        object_key: &str,
        window_bytes: u64,
    ) -> Result<BoxStream<'static, Result<RecordBatch>>> {
        let path = Path::from(object_key);

        let meta = store
            .head(&path)
            .await
            .map_err(|e| head_err(object_key, e))?;

        let mut reader = ObjectFileReader::new(store, meta.location, meta.size);
        let options = ArrowReaderOptions::new().with_offset_index_policy(PageIndexPolicy::Optional);

        let metadata = ArrowReaderMetadata::load_async(&mut reader, options)
            .await
            .map_err(|e| Error::internal(format!("parquet metadata: {e}")))?;

        let windows = row_windows(metadata.metadata(), window_bytes);

        let batches = stream::iter(windows)
            .map(move |(row_group, selection)| {
                let mut builder = ParquetRecordBatchStreamBuilder::new_with_metadata(
                    reader.clone(),
                    metadata.clone(),
                )
                .with_row_groups(vec![row_group]);

                if let Some(selection) = selection {
                    builder = builder.with_row_selection(selection);
                }

                builder
                    .build()
                    .map_err(|e| Error::internal(format!("parquet stream: {e}")))
            })
            .map_ok(|window| window.map_err(|e| Error::internal(format!("parquet read: {e}"))))
            .try_flatten();

        Ok(batches.boxed())
    }
}

/// 把每个 row group 切成行窗口。压缩后不超过 `window_bytes`、或没有 offset index 的 row group
/// 整个读（`None`）；其余切成行数相等的几段，每段的压缩字节大致是 `window_bytes`。
fn row_windows(
    metadata: &ParquetMetaData,
    window_bytes: u64,
) -> Vec<(usize, Option<RowSelection>)> {
    let mut windows = Vec::new();

    for (index, row_group) in metadata.row_groups().iter().enumerate() {
        let rows = usize::try_from(row_group.num_rows()).unwrap_or_default();
        let bytes = u64::try_from(row_group.compressed_size()).unwrap_or_default();
        let parts = bytes.div_ceil(window_bytes.max(1));

        let paged = metadata
            .offset_index()
            .and_then(|indexes| indexes.get(index))
            .is_some_and(|columns| !columns.is_empty());

        if !paged || parts <= 1 || rows == 0 {
            windows.push((index, None));
            continue;
        }

        let step = rows.div_ceil(usize::try_from(parts).unwrap_or(usize::MAX));
        let mut start = 0;

        while start < rows {
            let end = rows.min(start + step);

            let selectors = [
                RowSelector::skip(start),
                RowSelector::select(end - start),
                RowSelector::skip(rows - end),
            ];

            let selection = selectors
                .into_iter()
                .filter(|selector| selector.row_count > 0)
                .collect::<Vec<_>>();

            windows.push((index, Some(RowSelection::from(selection))));
            start = end;
        }
    }

    windows
}

#[cfg(test)]
mod tests {
    use arrow::array::{Int64Array, StringArray, TimestampMicrosecondArray};
    use object_store::{ObjectStoreExt, PutPayload, local::LocalFileSystem, path::Path};

    use super::*;
    use crate::{
        domain::stream::{FieldDef, FieldType, Retention, Schema, StreamDefinition, StreamType},
        infra::storage::{arrow_schema::to_arrow, parquet::writer::encode_parquet},
        shared::{ids::Id, time::TimestampMicros},
    };

    fn sample() -> (StreamDefinition, RecordBatch) {
        let stream = StreamDefinition {
            id: Id::new(),
            org_id: Id::from_string("org-1"),
            name: "app".into(),
            stream_type: StreamType::LOGS,
            schema: Schema {
                fields: vec![FieldDef {
                    name: "msg".into(),
                    data_type: FieldType::Utf8,
                    nullable: false,
                    index_type: None,
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

    async fn write_sample(store: &dyn ObjectStore, key: &str) {
        let (stream, batch) = sample();
        let bytes = encode_parquet(&stream, &batch).unwrap();
        store
            .put(&Path::from(key), PutPayload::from(bytes))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn roundtrip_read_all() {
        let tmp = tempfile::tempdir().unwrap();
        let store: Arc<dyn ObjectStore> =
            Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
        let reader = ParquetReader::new(store.clone());
        let object_key = "tests/reader-roundtrip";
        write_sample(store.as_ref(), object_key).await;

        let batches = reader.read_all(object_key).await.unwrap();
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
        let reader = ParquetReader::new(store.clone());
        let object_key = "tests/reader-range";
        write_sample(store.as_ref(), object_key).await;

        // sample() 的 _timestamp 统计为 [10_000, 20_000]
        // 窗口与统计相交 → row group 保留（行级过滤在查询侧做，这里整组返回）
        let hit = reader.read_time_range(object_key, 0, 15_000).await.unwrap();
        assert_eq!(hit.iter().map(|b| b.num_rows()).sum::<usize>(), 2);

        // [20_001, 30_000)：max=20_000 小于 start → 裁掉
        let after = reader
            .read_time_range(object_key, 20_001, 30_000)
            .await
            .unwrap();
        assert!(after.is_empty());

        // [0, 5_000)：min=10_000 不小于 end → 裁掉
        let before = reader.read_time_range(object_key, 0, 5_000).await.unwrap();
        assert!(before.is_empty());
    }

    /// 按名字排过序、页很小的文件：只读名字可能相等的页，时间窗也按页裁剪。
    #[tokio::test]
    async fn equals_reads_only_the_pages_that_may_hold_the_value() {
        use arrow::datatypes::{DataType, Field, Schema as ArrowSchema, TimeUnit};
        use parquet::{arrow::ArrowWriter, file::properties::WriterProperties};

        let schema = Arc::new(ArrowSchema::new(vec![
            Field::new(
                "_timestamp",
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
            Field::new("metric_name", DataType::Utf8, false),
        ]));

        let names = ["c", "b", "a"]
            .iter()
            .flat_map(|name| std::iter::repeat_n(*name, 300))
            .collect::<Vec<_>>();

        let times = (0..900).map(|row| (row % 300) * 10).collect::<Vec<i64>>();

        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(TimestampMicrosecondArray::from(times).with_timezone("UTC")),
                Arc::new(StringArray::from(names)),
            ],
        )
        .unwrap();

        let properties = WriterProperties::builder()
            .set_data_page_row_count_limit(100)
            .set_write_batch_size(100)
            .build();

        let mut bytes = Vec::new();
        let mut writer = ArrowWriter::try_new(&mut bytes, schema, Some(properties)).unwrap();
        writer.write(&batch).unwrap();
        writer.close().unwrap();

        let store: Arc<dyn ObjectStore> = Arc::new(object_store::memory::InMemory::new());

        store
            .put(&Path::from("tests/equals"), PutPayload::from(bytes))
            .await
            .unwrap();

        let reader = ParquetReader::new(store.clone());

        let read = |options: ReadOptions<'static>| {
            let reader = &reader;
            let store = store.clone();

            async move {
                reader
                    .read_from_store(store, "tests/equals", options)
                    .await
                    .unwrap()
                    .iter()
                    .flat_map(|batch| {
                        let names = batch
                            .column(1)
                            .as_any()
                            .downcast_ref::<StringArray>()
                            .unwrap();

                        (0..batch.num_rows())
                            .map(|row| names.value(row).to_string())
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>()
            }
        };

        let b = read(ReadOptions::new().with_equals("metric_name", "b")).await;
        assert_eq!(b.len(), 300);
        assert!(b.iter().all(|name| name == "b"));

        let missing = read(ReadOptions::new().with_equals("metric_name", "bb")).await;
        assert!(missing.is_empty());

        let early = read(
            ReadOptions::new()
                .with_equals("metric_name", "b")
                .with_time_range(0, 1_000),
        )
        .await;

        assert_eq!(early.len(), 100);
    }

    /// 每个 row group 切成几个窗口读：每行正好读到一次，行序与文件里一致。
    #[tokio::test]
    async fn windows_read_every_row_once_in_file_order() {
        use arrow::datatypes::{DataType, Field, Schema as ArrowSchema};
        use parquet::{
            arrow::{ArrowWriter, arrow_reader::ParquetRecordBatchReaderBuilder},
            file::properties::WriterProperties,
        };

        let schema = Arc::new(ArrowSchema::new(vec![Field::new(
            "value",
            DataType::Int64,
            false,
        )]));

        let values: Vec<i64> = (0..3_000).rev().collect();

        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![Arc::new(Int64Array::from(values.clone()))],
        )
        .unwrap();

        // Three row groups of a thousand rows, in pages of a hundred.
        let properties = WriterProperties::builder()
            .set_max_row_group_row_count(Some(1_000))
            .set_data_page_row_count_limit(100)
            .set_write_batch_size(100)
            .set_dictionary_enabled(false)
            .build();

        let mut bytes = Vec::new();
        let mut writer = ArrowWriter::try_new(&mut bytes, schema, Some(properties)).unwrap();
        writer.write(&batch).unwrap();
        writer.close().unwrap();

        let bytes = Bytes::from(bytes);

        let metadata = ParquetRecordBatchReaderBuilder::try_new_with_options(
            bytes.clone(),
            ArrowReaderOptions::new().with_offset_index_policy(PageIndexPolicy::Optional),
        )
        .unwrap()
        .metadata()
        .clone();

        let window_bytes = u64::try_from(metadata.row_group(0).compressed_size()).unwrap() / 4;
        let windows = row_windows(&metadata, window_bytes);

        assert_eq!(metadata.num_row_groups(), 3);
        assert!(windows.len() >= 12, "{} windows", windows.len());
        assert!(windows.iter().all(|(_, selection)| selection.is_some()));

        let store: Arc<dyn ObjectStore> = Arc::new(object_store::memory::InMemory::new());

        store
            .put(&Path::from("tests/windows"), PutPayload::from(bytes))
            .await
            .unwrap();

        let read = ParquetReader::new(store.clone())
            .stream_windows_from_store(store, "tests/windows", window_bytes)
            .await
            .unwrap()
            .collect::<Vec<_>>()
            .await;

        let mut seen = Vec::new();

        for batch in read {
            let batch = batch.unwrap();
            let column = batch
                .column(0)
                .as_any()
                .downcast_ref::<Int64Array>()
                .unwrap();
            seen.extend(column.values().iter().copied());
        }

        assert_eq!(seen, values);
    }
}
