// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! BufferPool：按 [`PhysicalDatasetId`] 维护 Arrow `RecordBuilder` 内存 buffer。
//!
//! 数据流：
//! 1. 写入方先持有 dataset buffer 锁，再执行 WAL append + buffer push；flush 取得同一把锁
//!    做 generation rotation，因此不存在“WAL 已写但尚未进入 buffer”的高水位竞态。
//! 2. FlushScheduler 把 active generation 旋转为 immutable in-flight batch。发布期间该 batch
//!    仍对查询可见；Catalog commit 后才退休，失败则转为 retry generation。
//!
//! `RecordBuilder` 内部按 [`StreamDefinition::schema`] 顺序维护每列一个 `*Builder`；
//! `extend_schema` 在 schema 演化时对历史行追加 null 补齐；整型字段提升为 Float64 时，
//! `sync_schema` 把已有行转成浮点。

use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Result, anyhow};
use arrow::{
    array::{
        Array, ArrayRef, AsArray, BooleanBuilder, Float64Builder, Int64Builder, RecordBatch,
        StringBuilder, TimestampMicrosecondBuilder,
    },
    datatypes::{DataType, Field, Float64Type, Int64Type, TimestampMicrosecondType},
};
use domain::{
    storage::{PhysicalDatasetId, WalSequence},
    stream::{FieldDef, FieldType, StreamDefinition},
};

use crate::infra::{
    intake::{rotation::RotationReason, row_columns::timestamp_type},
    storage::arrow_schema::TS_COL,
};

mod discard;
mod generation;
mod memory;
mod pool;

pub use discard::Discarded;
pub(crate) use generation::align_to_schema;
pub use generation::{BufferWriter, BufferedRecordBatch};
pub use memory::MemoryReservation;
pub use pool::{BufferPool, DatasetBuffer};

/// Buffer、flush single-flight 与 adaptive rotation 的唯一身份。
pub type BufferKey = PhysicalDatasetId;

/// 单列内存 builder，按 schema field type dispatch。
enum ColumnBuilder {
    Timestamp(TimestampMicrosecondBuilder),
    Bool(BooleanBuilder),
    Int64(Int64Builder),
    Float64(Float64Builder),
    /// Utf8 / Json 都落 String（Json 走 to_string）。
    Utf8(StringBuilder),
}

impl ColumnBuilder {
    fn new(t: FieldType) -> Self {
        match t {
            FieldType::Bool => Self::Bool(BooleanBuilder::new()),
            FieldType::Int64 => Self::Int64(Int64Builder::new()),
            FieldType::Float64 => Self::Float64(Float64Builder::new()),
            FieldType::Utf8 | FieldType::Json => Self::Utf8(StringBuilder::new()),
            FieldType::Timestamp => Self::new_timestamp(),
        }
    }

    /// A time column, in UTC like the arrays appended to it.
    fn new_timestamp() -> Self {
        Self::Timestamp(TimestampMicrosecondBuilder::new().with_data_type(timestamp_type()))
    }

    fn data_type(&self) -> DataType {
        match self {
            Self::Timestamp(_) => timestamp_type(),
            Self::Bool(_) => DataType::Boolean,
            Self::Int64(_) => DataType::Int64,
            Self::Float64(_) => DataType::Float64,
            Self::Utf8(_) => DataType::Utf8,
        }
    }

    fn append_nulls(&mut self, rows: usize) {
        match self {
            Self::Timestamp(b) => b.append_nulls(rows),
            Self::Bool(b) => b.append_nulls(rows),
            Self::Int64(b) => b.append_nulls(rows),
            Self::Float64(b) => b.append_nulls(rows),
            Self::Utf8(b) => b.append_nulls(rows),
        }
    }

    /// Appends `array`, which must be of this column's type, and returns the bytes it adds to the
    /// generation's size: a string's bytes, eight for a number or a time, one for a boolean or a
    /// null.
    fn append_array(&mut self, array: &dyn Array) -> Result<usize> {
        let nulls = array.null_count();
        let values = array.len() - nulls;
        let expected = self.data_type();
        let wrong = || anyhow!("expect {expected:?}, got {:?}", array.data_type());

        let bytes = match self {
            Self::Timestamp(b) => {
                let array = array
                    .as_primitive_opt::<TimestampMicrosecondType>()
                    .ok_or_else(wrong)?;

                b.append_array(array);
                values * 8 + nulls
            }
            Self::Bool(b) => {
                b.append_array(array.as_boolean_opt().ok_or_else(wrong)?);
                array.len()
            }
            Self::Int64(b) => {
                b.append_array(array.as_primitive_opt::<Int64Type>().ok_or_else(wrong)?);
                values * 8 + nulls
            }
            // A batch built while the field was still Int64 (a WAL record replayed, or a write
            // that raced the widening) is converted.
            Self::Float64(b) => {
                match array.as_primitive_opt::<Int64Type>() {
                    Some(integers) => {
                        for value in integers {
                            b.append_option(value.map(|value| value as f64));
                        }
                    }
                    None => {
                        b.append_array(array.as_primitive_opt::<Float64Type>().ok_or_else(wrong)?);
                    }
                }

                values * 8 + nulls
            }
            Self::Utf8(b) => {
                let array = array.as_string_opt::<i32>().ok_or_else(wrong)?;
                let offsets = array.value_offsets();
                let text = (offsets[array.len()] - offsets[0]) as usize;

                b.append_array(array)?;
                text + nulls
            }
        };

        Ok(bytes)
    }

    /// An Int64 column as Float64, its rows so far converted; any other column is left as it is.
    fn widen(&mut self) {
        if let Self::Int64(b) = self {
            let integers = b.finish();
            let mut floats = Float64Builder::with_capacity(integers.len());

            for value in &integers {
                floats.append_option(value.map(|value| value as f64));
            }

            *self = Self::Float64(floats);
        }
    }

    fn finish(&mut self) -> ArrayRef {
        match self {
            Self::Timestamp(b) => Arc::new(b.finish().with_timezone("UTC")),
            Self::Bool(b) => Arc::new(b.finish()),
            Self::Int64(b) => Arc::new(b.finish()),
            Self::Float64(b) => Arc::new(b.finish()),
            Self::Utf8(b) => Arc::new(b.finish()),
        }
    }

    fn finish_cloned(&self) -> ArrayRef {
        match self {
            Self::Timestamp(b) => Arc::new(b.finish_cloned().with_timezone("UTC")),
            Self::Bool(b) => Arc::new(b.finish_cloned()),
            Self::Int64(b) => Arc::new(b.finish_cloned()),
            Self::Float64(b) => Arc::new(b.finish_cloned()),
            Self::Utf8(b) => Arc::new(b.finish_cloned()),
        }
    }
}

/// 单个 stream 的内存 buffer。线程不安全：调用方用 `Mutex` 串行化。
pub struct RecordBuilder {
    /// 含 `_timestamp` 在内的字段定义（顺序固定为：`_timestamp` 在前，其余按 schema 序）。
    fields: Vec<Field>,
    /// 名字 → 列 builder。
    columns: HashMap<String, ColumnBuilder>,
    /// 列名顺序，等同 `fields` 顺序。
    column_order: Vec<String>,
    /// 已 push 的行数（≡ 各列 builder 的 len）。
    row_count: usize,
    /// 估算的字节数，用于 `buffer_max_size` 阈值判断。
    approx_size_bytes: usize,
    /// 从进程级内存预算中为当前 generation（含 pending）保留的原始 payload 字节。
    accounted_size_bytes: usize,
    collected_bytes: Option<u64>,
    /// 当前活跃 generation 首行完成写入的单调时钟时间。
    active_started_at: Option<Instant>,
    /// 当前 active generation 的 WAL namespace 与逐行 sequence sidecar。
    active_writer: Option<BufferWriter>,
    active_sequences: Vec<WalSequence>,
    /// 发布失败后等待重试的 immutable generations（最旧优先）。
    pending: VecDeque<BufferedRecordBatch>,
    /// 正在编码 / 上传 / Catalog commit 的 generation。发布期间查询仍必须看见它。
    inflight: Option<BufferedRecordBatch>,
}

impl RecordBuilder {
    pub fn new(stream: &StreamDefinition) -> Self {
        let mut fields: Vec<Field> = Vec::with_capacity(stream.schema.fields.len() + 1);
        let mut columns: HashMap<String, ColumnBuilder> = HashMap::new();
        let mut column_order: Vec<String> = Vec::new();

        // 1. _timestamp（隐式列）
        fields.push(Field::new(TS_COL, timestamp_type(), false));
        columns.insert(TS_COL.to_string(), ColumnBuilder::new_timestamp());
        column_order.push(TS_COL.to_string());

        // 2. domain schema 各列（加密列是 Utf8 密文列）
        for f in &stream.schema.fields {
            let builder = if f.encrypted {
                ColumnBuilder::Utf8(StringBuilder::new())
            } else {
                ColumnBuilder::new(f.data_type)
            };

            let dt = builder.data_type();
            fields.push(Field::new(&f.name, dt, f.nullable));
            columns.insert(f.name.clone(), builder);
            column_order.push(f.name.clone());
        }

        Self {
            fields,
            columns,
            column_order,
            row_count: 0,
            approx_size_bytes: 0,
            accounted_size_bytes: 0,
            collected_bytes: Some(0),
            active_started_at: None,
            active_writer: None,
            active_sequences: Vec::new(),
            pending: VecDeque::new(),
            inflight: None,
        }
    }

    pub fn row_count(&self) -> usize {
        self.row_count
    }

    pub fn approx_size_bytes(&self) -> usize {
        self.approx_size_bytes
    }

    pub fn accounted_size_bytes(&self) -> usize {
        self.accounted_size_bytes
            + self
                .pending
                .iter()
                .map(BufferedRecordBatch::accounted_size_bytes)
                .sum::<usize>()
            + self
                .inflight
                .as_ref()
                .map_or(0, BufferedRecordBatch::accounted_size_bytes)
    }

    /// 把一批已经成功写入 WAL 的 reservation 转移给此 generation。
    pub fn add_accounted_bytes(&mut self, bytes: usize) {
        self.accounted_size_bytes = self.accounted_size_bytes.saturating_add(bytes);
    }

    pub fn high_watermark_seq(&self) -> u64 {
        self.active_sequences
            .last()
            .map(|sequence| sequence.0)
            .into_iter()
            .chain(
                self.pending
                    .iter()
                    .map(|batch| batch.sequence_range().end.0),
            )
            .chain(
                self.inflight
                    .iter()
                    .map(|batch| batch.sequence_range().end.0),
            )
            .max()
            .unwrap_or_default()
    }

    /// Appends a batch that is in the WAL at `sequence`.
    ///
    /// The batch's columns are matched to the buffer's by name: a buffer column the batch lacks is
    /// null for its rows, and a batch column the buffer lacks is an error, since the caller
    /// synchronizes the schema first (`sync_schema`). `writer` and `sequence` are recorded for every
    /// row, for flush provenance and for deduplication by queries.
    pub fn push_batch(
        &mut self,
        batch: &RecordBatch,
        writer: &BufferWriter,
        sequence: WalSequence,
    ) -> Result<()> {
        // A generation has one writer. Rows of a new writer epoch (this process took a new one
        // after being fenced) start a new generation; the old one waits for its flush, which
        // the fence refuses, and is then dropped.
        if self.row_count > 0
            && self
                .active_writer
                .as_ref()
                .is_some_and(|active_writer| active_writer != writer)
        {
            let sealed = self.finish_active()?;
            self.pending.push_back(sealed);
        }

        if let Some(previous) = self.active_sequences.last()
            && sequence < *previous
        {
            return Err(anyhow!(
                "buffer WAL sequence regressed from {previous} to {sequence}"
            ));
        }

        let rows = batch.num_rows();

        if rows == 0 {
            return Ok(());
        }

        if let Some(unknown) = batch
            .schema()
            .fields()
            .iter()
            .find(|field| !self.columns.contains_key(field.name()))
        {
            return Err(anyhow!(
                "column {} is not in the buffer schema",
                unknown.name()
            ));
        }

        // A Float64 column in the batch widens an Int64 column of the buffer: the batch was built
        // after the stream widened the field.
        let widening: Vec<String> = batch
            .schema()
            .fields()
            .iter()
            .filter(|field| field.data_type() == &DataType::Float64)
            .filter(|field| {
                matches!(
                    self.columns.get(field.name()),
                    Some(ColumnBuilder::Int64(_))
                )
            })
            .map(|field| field.name().clone())
            .collect();

        for name in widening {
            self.widen_column(&name);
        }

        let mut bytes = 0;

        for name in &self.column_order {
            let column = self
                .columns
                .get_mut(name)
                .ok_or_else(|| anyhow!("column {name} missing from builder map"))?;

            match batch.column_by_name(name) {
                Some(array) => {
                    bytes += column
                        .append_array(array.as_ref())
                        .map_err(|error| anyhow!("column {name}: {error}"))?;
                }
                None => {
                    column.append_nulls(rows);
                    bytes += rows;
                }
            }
        }

        self.row_count += rows;
        self.approx_size_bytes += bytes;
        self.active_started_at.get_or_insert_with(Instant::now);
        self.active_writer.get_or_insert_with(|| writer.clone());
        self.active_sequences
            .extend(std::iter::repeat_n(sequence, rows));
        Ok(())
    }

    /// 返回当前 generation 是否满足 rotation 条件；retry 优先于 size/age。
    pub fn rotation_due(
        &self,
        max_bytes: usize,
        max_age: Duration,
        now: Instant,
    ) -> Option<RotationReason> {
        if !self.pending.is_empty() {
            return Some(RotationReason::Retry);
        }

        if self.row_count == 0 {
            return None;
        }

        if self.approx_size_bytes >= max_bytes {
            return Some(RotationReason::Size);
        }

        self.active_started_at
            .filter(|started| now.saturating_duration_since(*started) >= max_age)
            .map(|_| RotationReason::Age)
    }

    /// 在 schema 演化后追加一个新字段：补 null 到当前 row_count，使新列覆盖已有行。
    pub fn extend_schema(&mut self, field: &FieldDef) {
        if self.columns.contains_key(&field.name) {
            return;
        }

        let mut col = if field.encrypted {
            ColumnBuilder::Utf8(StringBuilder::new())
        } else {
            ColumnBuilder::new(field.data_type)
        };

        col.append_nulls(self.row_count);

        let dt = col.data_type();
        self.fields
            .push(Field::new(&field.name, dt, field.nullable));
        self.columns.insert(field.name.clone(), col);
        self.column_order.push(field.name.clone());
    }

    /// Synchronize an existing dataset buffer with the latest projected stream schema.
    ///
    /// Intake calls this while holding the same per-dataset mutex that orders WAL append and
    /// buffer push. New columns therefore become visible atomically to subsequent generations,
    /// while rows already present in the active generation are padded with nulls.
    ///
    /// An Int64 column whose field the stream has widened to Float64 becomes Float64, its rows
    /// converted.
    pub fn sync_schema(&mut self, stream: &StreamDefinition) {
        for field in &stream.schema.fields {
            self.extend_schema(field);

            if field.data_type == FieldType::Float64 && !field.encrypted {
                self.widen_column(&field.name);
            }
        }
    }

    /// Makes the Int64 column `name` Float64, its rows converted; any other column is left as it
    /// is.
    fn widen_column(&mut self, name: &str) {
        let Some(column) = self.columns.get_mut(name) else {
            return;
        };

        if !matches!(column, ColumnBuilder::Int64(_)) {
            return;
        }

        column.widen();

        if let Some(field) = self.fields.iter_mut().find(|field| field.name() == name) {
            *field = Field::new(name, DataType::Float64, field.is_nullable());
        }
    }

    /// 无待 flush 或正在发布的数据。
    pub fn is_empty(&self) -> bool {
        self.row_count == 0 && self.pending.is_empty() && self.inflight.is_none()
    }
}

/// Test shorthands: an event becomes a one-row batch under this buffer's columns.
#[cfg(test)]
impl RecordBuilder {
    pub fn push(&mut self, event: &domain::intake::RawEvent, seq: u64) -> Result<()> {
        self.push_with_position(
            event,
            &BufferWriter::new(
                domain::storage::WriterNodeId::new("test-node"),
                domain::storage::WriterEpoch(1),
            ),
            WalSequence(seq),
        )
    }

    pub fn push_with_position(
        &mut self,
        event: &domain::intake::RawEvent,
        writer: &BufferWriter,
        sequence: WalSequence,
    ) -> Result<()> {
        let fields = self
            .fields
            .iter()
            .skip(1)
            .map(|field| FieldDef {
                name: field.name().clone(),
                data_type: match field.data_type() {
                    DataType::Boolean => FieldType::Bool,
                    DataType::Int64 => FieldType::Int64,
                    DataType::Float64 => FieldType::Float64,
                    DataType::Timestamp(..) => FieldType::Timestamp,
                    _ => FieldType::Utf8,
                },
                nullable: field.is_nullable(),
                index_type: domain::stream::StreamIndexType::None,
                encrypted: false,
            })
            .collect();

        let stream = StreamDefinition {
            id: kernel::ids::Id::new(),
            org_id: kernel::ids::Id::from_string("test-org"),
            name: "test".into(),
            stream_type: domain::stream::StreamType::LOGS,
            schema: domain::stream::Schema { fields },
            retention: None,
            created_at: kernel::time::TimestampMicros(0),
            updated_at: kernel::time::TimestampMicros(0),
        };

        let batch =
            super::row_columns::rows_to_columns(&stream, std::slice::from_ref(event), None)?;
        self.push_batch(&batch, writer, sequence)
    }
}

#[cfg(test)]
mod tests {
    use arrow::array::Array;
    use domain::{
        intake::RawEvent,
        stream::{Retention, Schema, StreamIndexType, StreamType},
    };
    use kernel::{ids::Id, time::TimestampMicros};
    use serde_json::json;

    use super::*;

    fn stream_def() -> StreamDefinition {
        StreamDefinition {
            id: Id::new(),
            org_id: Id::from_string("org-a"),
            name: "app".into(),
            stream_type: StreamType::LOGS,
            schema: Schema {
                fields: vec![
                    FieldDef {
                        name: "level".into(),
                        data_type: FieldType::Utf8,
                        nullable: false,
                        index_type: StreamIndexType::FullText,
                        encrypted: false,
                    },
                    FieldDef {
                        name: "latency_ms".into(),
                        data_type: FieldType::Int64,
                        nullable: true,
                        index_type: StreamIndexType::Skip,
                        encrypted: false,
                    },
                ],
            },
            retention: Some(Retention { days: 7 }),
            created_at: TimestampMicros::now(),
            updated_at: TimestampMicros::now(),
        }
    }

    fn raw_event(ts: i64, fields: serde_json::Map<String, serde_json::Value>) -> RawEvent {
        RawEvent {
            timestamp: TimestampMicros(ts),
            fields,
        }
    }

    #[test]
    fn rotation_due_distinguishes_size_age_retry_and_under_threshold() {
        let s = stream_def();
        let mut age_buffer = RecordBuilder::new(&s);
        let mut fields = serde_json::Map::new();
        fields.insert("level".into(), json!("info"));
        age_buffer
            .push(&raw_event(1_000_000, fields.clone()), 1)
            .unwrap();
        let started = age_buffer.active_started_at.unwrap();
        assert_eq!(
            age_buffer.rotation_due(
                usize::MAX,
                Duration::from_secs(10),
                started + Duration::from_secs(9)
            ),
            None
        );
        assert_eq!(
            age_buffer.rotation_due(
                usize::MAX,
                Duration::from_secs(10),
                started + Duration::from_secs(10)
            ),
            Some(RotationReason::Age)
        );

        let mut size_buffer = RecordBuilder::new(&s);
        size_buffer.push(&raw_event(2_000_000, fields), 2).unwrap();
        let now = size_buffer.active_started_at.unwrap();
        assert_eq!(
            size_buffer.rotation_due(
                size_buffer.approx_size_bytes(),
                Duration::from_secs(10),
                now
            ),
            Some(RotationReason::Size)
        );
        let batch = size_buffer.begin_flush().unwrap().unwrap();
        assert!(size_buffer.active_started_at.is_none());
        size_buffer.fail_flush(&batch.flush_id()).unwrap();
        assert_eq!(
            size_buffer.rotation_due(usize::MAX, Duration::MAX, now),
            Some(RotationReason::Retry)
        );
    }

    /// flush 失败后 retry generation 与期间的新写入保持独立，按 generation 顺序各提交一次。
    #[test]
    fn failed_generation_retries_before_new_active_rows() {
        let s = stream_def();
        let mut b = RecordBuilder::new(&s);
        for (ts, lvl) in [(1_000_000, "a"), (2_000_000, "b")] {
            let mut f = serde_json::Map::new();
            f.insert("level".into(), json!(lvl));
            b.push(&raw_event(ts, f), 1).unwrap();
        }
        b.add_collected_bytes(Some(100));
        let batch = b.begin_flush().unwrap().unwrap();
        assert_eq!(batch.batch.num_rows(), 2);

        // flush 失败：immutable generation 回到 retry 队列。
        b.fail_flush(&batch.flush_id()).unwrap();
        assert!(!b.is_empty(), "暂存的数据必须让 buffer 不为空");

        // flush 的 IO 期间又来了一条新写入
        let mut f = serde_json::Map::new();
        f.insert("level".into(), json!("c"));
        b.push(&raw_event(3_000_000, f), 2).unwrap();

        b.add_collected_bytes(Some(200));
        let retry = b.begin_flush().unwrap().unwrap();
        assert_eq!(retry.provenance().collected_bytes, Some(100));
        assert_eq!(retry.batch.num_rows(), 2, "retry generation 不与新写入混合");
        assert_eq!(retry.sequence_range().end, WalSequence(1));
        let levels = retry
            .batch
            .column(retry.batch.schema().index_of("level").unwrap())
            .as_any()
            .downcast_ref::<arrow::array::StringArray>()
            .unwrap();
        let got: Vec<&str> = (0..levels.len()).map(|i| levels.value(i)).collect();
        assert_eq!(got, vec!["a", "b"]);
        b.complete_flush(&retry.flush_id()).unwrap();

        let active = b.begin_flush().unwrap().unwrap();
        assert_eq!(active.provenance().collected_bytes, Some(200));
        assert_eq!(active.batch.num_rows(), 1);
        assert_eq!(active.sequence_range().end, WalSequence(2));
        b.complete_flush(&active.flush_id()).unwrap();
        assert!(b.is_empty(), "两个 generation 提交后必须清空");
        assert!(b.begin_flush().unwrap().is_none());
    }

    /// flush 失败后 schema 又演化了：暂存 batch 比当前 schema 少列，concat 前必须补齐 null 列。
    #[test]
    fn restore_batch_aligns_to_schema_evolved_after_the_failed_flush() {
        let s = stream_def();
        let mut b = RecordBuilder::new(&s);
        let mut f = serde_json::Map::new();
        f.insert("level".into(), json!("old"));
        b.push(&raw_event(1_000_000, f), 1).unwrap();
        let batch = b.begin_flush().unwrap().unwrap();
        b.fail_flush(&batch.flush_id()).unwrap();

        // flush 失败后新字段出现，schema 演化
        b.extend_schema(&FieldDef {
            name: "trace_id".into(),
            data_type: FieldType::Utf8,
            nullable: true,
            index_type: StreamIndexType::None,
            encrypted: false,
        });
        let mut f2 = serde_json::Map::new();
        f2.insert("level".into(), json!("new"));
        f2.insert("trace_id".into(), json!("abc"));
        b.push(&raw_event(2_000_000, f2), 2).unwrap();

        let retry = b.begin_flush().unwrap().unwrap();
        let target = Arc::new(arrow::datatypes::Schema::new(b.fields.clone()));
        let aligned = align_to_schema(retry.batch.clone(), &target).unwrap();
        assert_eq!(aligned.num_rows(), 1);
        let tid = aligned
            .column(aligned.schema().index_of("trace_id").unwrap())
            .as_any()
            .downcast_ref::<arrow::array::StringArray>()
            .unwrap();
        assert!(tid.is_null(0), "暂存的老行在新列上应为 null");
        b.complete_flush(&retry.flush_id()).unwrap();
        let active = b.begin_flush().unwrap().unwrap();
        let tid = active
            .batch
            .column(active.batch.schema().index_of("trace_id").unwrap())
            .as_any()
            .downcast_ref::<arrow::array::StringArray>()
            .unwrap();
        assert_eq!(tid.value(0), "abc");
    }

    /// flush 失败后一直没有新写入：不能因为 `row_count == 0` 就把 buffer 当空的跳过，
    /// 否则暂存的数据永远不会被重试。
    #[test]
    fn pending_alone_is_still_flushable_without_any_new_writes() {
        let s = stream_def();
        let mut b = RecordBuilder::new(&s);
        let mut f = serde_json::Map::new();
        f.insert("level".into(), json!("stranded"));
        b.push(&raw_event(1_000_000, f), 7).unwrap();
        let batch = b.begin_flush().unwrap().unwrap();
        b.fail_flush(&batch.flush_id()).unwrap();

        assert_eq!(b.row_count(), 0, "builder 自身确实没有行");
        assert!(!b.is_empty(), "但有 pending → 不能被当作空 buffer 跳过");
        let retry = b.begin_flush().unwrap().unwrap();
        assert_eq!(retry.batch.num_rows(), 1, "暂存的行必须被重新交出");
        assert_eq!(retry.sequence_range().end, WalSequence(7));
    }

    #[test]
    fn push_and_finish_round_trip() {
        let s = stream_def();
        let mut b = RecordBuilder::new(&s);
        let mut f1 = serde_json::Map::new();
        f1.insert("level".into(), json!("info"));
        f1.insert("latency_ms".into(), json!(12));
        b.push(&raw_event(1_000_000, f1), 1).unwrap();

        let mut f2 = serde_json::Map::new();
        f2.insert("level".into(), json!("warn"));
        // latency_ms 缺字段 → null
        b.push(&raw_event(2_000_000, f2), 2).unwrap();

        assert_eq!(b.row_count(), 2);
        assert_eq!(b.high_watermark_seq(), 2);

        let batch = b.begin_flush().unwrap().unwrap();
        assert_eq!(batch.batch.num_rows(), 2);
        assert_eq!(batch.batch.num_columns(), 3); // _timestamp + level + latency_ms
        assert_eq!(batch.sequence_range().end, WalSequence(2));
        assert_eq!(b.row_count(), 0, "row_count cleared after finish");
    }

    #[test]
    fn extend_schema_pads_history_with_null() {
        let s = stream_def();
        let mut b = RecordBuilder::new(&s);
        let mut f = serde_json::Map::new();
        f.insert("level".into(), json!("info"));
        b.push(&raw_event(1_000_000, f.clone()), 1).unwrap();
        b.push(&raw_event(2_000_000, f), 2).unwrap();

        // 此时加入新字段 user_id（之前 2 行历史，应填 null）
        let new_field = FieldDef {
            name: "user_id".into(),
            data_type: FieldType::Utf8,
            nullable: true,
            index_type: StreamIndexType::None,
            encrypted: false,
        };
        b.extend_schema(&new_field);

        // 再 push 一行，user_id = "u-3"
        let mut f3 = serde_json::Map::new();
        f3.insert("level".into(), json!("error"));
        f3.insert("user_id".into(), json!("u-3"));
        b.push(&raw_event(3_000_000, f3), 3).unwrap();

        let batch = b.begin_flush().unwrap().unwrap();
        assert_eq!(batch.batch.num_rows(), 3);
        assert_eq!(batch.batch.num_columns(), 4);

        // user_id 列：前 2 行 null，第 3 行 "u-3"
        let user_col = batch.batch.column_by_name("user_id").unwrap();
        let arr = user_col
            .as_any()
            .downcast_ref::<arrow::array::StringArray>()
            .unwrap();
        assert!(arr.is_null(0));
        assert!(arr.is_null(1));
        assert_eq!(arr.value(2), "u-3");
    }

    fn latency_values(batch: &RecordBatch) -> Vec<Option<f64>> {
        let column = batch.column_by_name("latency_ms").unwrap();

        assert_eq!(column.data_type(), &DataType::Float64);
        column.as_primitive::<Float64Type>().iter().collect()
    }

    fn latency_event(ts: i64, latency: serde_json::Value) -> RawEvent {
        let mut fields = serde_json::Map::new();
        fields.insert("level".into(), json!("info"));
        fields.insert("latency_ms".into(), latency);
        raw_event(ts, fields)
    }

    /// The stream widens `latency_ms` to Float64 while the generation holds Int64 rows: they are
    /// converted, and a batch still built as Int64 (a WAL record replayed after the widening) is
    /// converted on its way in.
    #[test]
    fn sync_schema_widens_an_int_column_with_its_rows() {
        let original = stream_def();
        let mut widened = original.clone();
        widened.schema.fields[1].data_type = FieldType::Float64;

        let mut builder = RecordBuilder::new(&original);

        builder
            .push(&latency_event(1_000_000, json!(7)), 1)
            .unwrap();

        builder.sync_schema(&widened);

        let replayed = crate::infra::intake::row_columns::rows_to_columns(
            &original,
            &[latency_event(2_000_000, json!(8))],
            None,
        )
        .unwrap();

        let writer = BufferWriter::new(
            domain::storage::WriterNodeId::new("test-node"),
            domain::storage::WriterEpoch(1),
        );

        builder
            .push_batch(&replayed, &writer, WalSequence(2))
            .unwrap();

        builder
            .push(&latency_event(3_000_000, json!(2.5)), 3)
            .unwrap();

        let batch = builder.begin_flush().unwrap().unwrap().batch;
        assert_eq!(latency_values(&batch), [Some(7.0), Some(8.0), Some(2.5)]);
    }

    /// A Float64 batch reaching a buffer whose column is still Int64 widens the column too.
    #[test]
    fn a_float_batch_widens_an_int_column() {
        let original = stream_def();
        let mut widened = original.clone();
        widened.schema.fields[1].data_type = FieldType::Float64;

        let mut builder = RecordBuilder::new(&original);

        builder
            .push(&latency_event(1_000_000, json!(7)), 1)
            .unwrap();

        let floats = crate::infra::intake::row_columns::rows_to_columns(
            &widened,
            &[latency_event(2_000_000, json!(0.5))],
            None,
        )
        .unwrap();

        let writer = BufferWriter::new(
            domain::storage::WriterNodeId::new("test-node"),
            domain::storage::WriterEpoch(1),
        );

        builder
            .push_batch(&floats, &writer, WalSequence(2))
            .unwrap();

        let batch = builder.begin_flush().unwrap().unwrap().batch;
        assert_eq!(latency_values(&batch), [Some(7.0), Some(0.5)]);
    }

    #[test]
    fn sync_schema_adds_all_new_fields_to_an_existing_generation() {
        let original = stream_def();
        let mut evolved = original.clone();
        evolved.schema.fields.extend([
            FieldDef {
                name: "trace_id".into(),
                data_type: FieldType::Utf8,
                nullable: true,
                index_type: StreamIndexType::None,
                encrypted: false,
            },
            FieldDef {
                name: "duration_ns".into(),
                data_type: FieldType::Int64,
                nullable: true,
                index_type: StreamIndexType::None,
                encrypted: false,
            },
        ]);

        let mut builder = RecordBuilder::new(&original);
        let mut old_fields = serde_json::Map::new();
        old_fields.insert("level".into(), json!("old"));
        builder.push(&raw_event(1_000_000, old_fields), 1).unwrap();

        builder.sync_schema(&evolved);
        builder.sync_schema(&evolved);
        let mut new_fields = serde_json::Map::new();
        new_fields.insert("level".into(), json!("new"));
        new_fields.insert("trace_id".into(), json!("trace-1"));
        new_fields.insert("duration_ns".into(), json!(42));
        builder.push(&raw_event(2_000_000, new_fields), 2).unwrap();

        let batch = builder.begin_flush().unwrap().unwrap().batch;
        assert_eq!(batch.num_rows(), 2);
        assert_eq!(batch.num_columns(), 5);
        let trace_ids = batch
            .column_by_name("trace_id")
            .unwrap()
            .as_any()
            .downcast_ref::<arrow::array::StringArray>()
            .unwrap();
        assert!(trace_ids.is_null(0));
        assert_eq!(trace_ids.value(1), "trace-1");
    }

    #[test]
    fn sequence_regression_is_rejected() {
        let s = stream_def();
        let mut b = RecordBuilder::new(&s);
        let mut f = serde_json::Map::new();
        f.insert("level".into(), json!("info"));
        b.push(&raw_event(1, f.clone()), 5).unwrap();
        assert!(b.push(&raw_event(2, f.clone()), 3).is_err());
        b.push(&raw_event(3, f), 7).unwrap();
        assert_eq!(b.high_watermark_seq(), 7);
    }

    #[tokio::test]
    async fn buffer_pool_get_or_create_returns_same_handle() {
        let s = stream_def();
        let pool = BufferPool::new();
        let resolver = crate::infra::intake::dataset_resolver::test_support::test_resolver();
        let dataset = resolver
            .resolve(
                &s,
                domain::storage::primary_dataset_type(s.stream_type).unwrap(),
            )
            .await
            .unwrap();
        let b1 = pool.get_or_create_dataset(&s, dataset.clone()).unwrap();
        let b2 = pool.get_or_create_dataset(&s, dataset).unwrap();
        assert!(Arc::ptr_eq(&b1, &b2));
    }
}
