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
//! `extend_schema` 在 schema 演化时对历史行追加 null 补齐。

use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Result, anyhow};
use arrow::{
    array::{
        ArrayRef, BooleanBuilder, Float64Builder, Int64Builder, StringBuilder,
        TimestampMicrosecondBuilder,
    },
    datatypes::{DataType, Field, TimeUnit},
};

use crate::{
    domain::{
        intake::RawEvent,
        storage::{PhysicalDatasetId, WalSequence, WriterEpoch, WriterNodeId},
        stream::{FieldDef, FieldType, StreamDefinition},
    },
    infra::{
        cipher::{OrgFieldKey, encrypt_field},
        intake::rotation::RotationReason,
        storage::arrow_schema::TS_COL,
    },
};

mod generation;
mod memory;
mod pool;

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
            FieldType::Timestamp => Self::Timestamp(TimestampMicrosecondBuilder::new()),
        }
    }

    fn new_timestamp() -> Self {
        Self::Timestamp(TimestampMicrosecondBuilder::new())
    }

    fn data_type(&self) -> DataType {
        match self {
            Self::Timestamp(_) => DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            Self::Bool(_) => DataType::Boolean,
            Self::Int64(_) => DataType::Int64,
            Self::Float64(_) => DataType::Float64,
            Self::Utf8(_) => DataType::Utf8,
        }
    }

    fn append_null(&mut self) {
        match self {
            Self::Timestamp(b) => b.append_null(),
            Self::Bool(b) => b.append_null(),
            Self::Int64(b) => b.append_null(),
            Self::Float64(b) => b.append_null(),
            Self::Utf8(b) => b.append_null(),
        }
    }

    /// 追加一个 JSON 值，返回写入的近似字节数供 `buffer_max_mb` 判断。
    ///
    /// 由本函数顺带返回长度，是为了让 Utf8 列上 Object/Array 的 `to_string()` 每行只做一次：
    /// 调用方若另调一个估算函数，同一个值会被完整串化两次（一次写入、一次仅为量长度后丢弃）。
    fn append_json(&mut self, v: &serde_json::Value) -> Result<usize> {
        if v.is_null() {
            self.append_null();
            return Ok(1);
        }
        let written = match self {
            Self::Bool(b) => {
                b.append_value(v.as_bool().ok_or_else(|| anyhow!("expect bool"))?);
                1
            }
            Self::Int64(b) => {
                b.append_value(v.as_i64().ok_or_else(|| anyhow!("expect i64"))?);
                8
            }
            Self::Float64(b) => {
                b.append_value(v.as_f64().ok_or_else(|| anyhow!("expect f64"))?);
                8
            }
            Self::Utf8(b) => match v {
                serde_json::Value::String(s) => {
                    b.append_value(s);
                    s.len()
                }
                other => {
                    let s = other.to_string();
                    let n = s.len();
                    b.append_value(s);
                    n
                }
            },
            Self::Timestamp(b) => {
                let micros = v
                    .as_i64()
                    .ok_or_else(|| anyhow!("expect timestamp i64 micros"))?;
                b.append_value(micros);
                8
            }
        };
        Ok(written)
    }

    fn append_timestamp_micros(&mut self, micros: i64) -> Result<()> {
        match self {
            Self::Timestamp(b) => {
                b.append_value(micros);
                Ok(())
            }
            _ => Err(anyhow!("append_timestamp_micros on non-timestamp builder")),
        }
    }

    /// 直接追加一个 Utf8 值（加密字段：追加密文串）。仅 Utf8 builder 可用。
    fn append_utf8(&mut self, s: &str) -> Result<()> {
        match self {
            Self::Utf8(b) => {
                b.append_value(s);
                Ok(())
            }
            _ => Err(anyhow!("append_utf8 on non-utf8 builder")),
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
    /// 估算的字节数，用于 `buffer_max_mb` 阈值判断。
    approx_size_bytes: usize,
    /// 从进程级内存预算中为当前 generation（含 pending）保留的原始 payload 字节。
    accounted_size_bytes: usize,
    /// 当前活跃 generation 首行完成写入的单调时钟时间。
    active_started_at: Option<Instant>,
    /// 当前 active generation 的 WAL namespace 与逐行 sequence sidecar。
    active_writer: Option<BufferWriter>,
    active_sequences: Vec<WalSequence>,
    /// 发布失败后等待重试的 immutable generations（最旧优先）。
    pending: VecDeque<BufferedRecordBatch>,
    /// 正在编码 / 上传 / Catalog commit 的 generation。发布期间查询仍必须看见它。
    inflight: Option<BufferedRecordBatch>,
    /// 当前 org 的字段加密 DEK；由调用方（sink / replay）在 push 前 `set_field_key` 注入。
    /// `None` 且存在加密字段时 push 报错。
    field_key: Option<OrgFieldKey>,
    /// 标 `encrypted` 的列名集合：push 时这些列的值先用 DEK 加密成 `kid:` 串再落 Utf8 列。
    encrypted_cols: HashSet<String>,
}

impl RecordBuilder {
    pub fn new(stream: &StreamDefinition) -> Self {
        let mut fields: Vec<Field> = Vec::with_capacity(stream.schema.fields.len() + 1);
        let mut columns: HashMap<String, ColumnBuilder> = HashMap::new();
        let mut column_order: Vec<String> = Vec::new();
        let mut encrypted_cols: HashSet<String> = HashSet::new();

        // 1. _timestamp（隐式列）
        fields.push(Field::new(
            TS_COL,
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        ));
        columns.insert(TS_COL.to_string(), ColumnBuilder::new_timestamp());
        column_order.push(TS_COL.to_string());

        // 2. domain schema 各列（加密列建 Utf8 密文列）
        for f in &stream.schema.fields {
            let builder = if f.encrypted {
                encrypted_cols.insert(f.name.clone());
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
            active_started_at: None,
            active_writer: None,
            active_sequences: Vec::new(),
            pending: VecDeque::new(),
            inflight: None,
            field_key: None,
            encrypted_cols,
        }
    }

    /// 是否含加密字段（调用方据此决定是否需要在 push 前解析 + 注入 DEK）。
    pub fn has_encrypted_fields(&self) -> bool {
        !self.encrypted_cols.is_empty()
    }

    /// 注入 / 刷新当前 org 字段加密 DEK（轮换后值会变，故每批 push 前由调用方设置）。
    pub fn set_field_key(&mut self, key: OrgFieldKey) {
        self.field_key = Some(key);
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

    /// 把一条事件 push 进 buffer。
    /// - 缺字段填 null；
    /// - 未知字段当前忽略（schema 演化路径走 `extend_schema`）。
    /// - 同时记录 `(writer node, epoch, sequence)`，供 flush provenance 与查询去重。
    pub fn push(&mut self, event: &RawEvent, seq: u64) -> Result<()> {
        self.push_with_position(
            event,
            &BufferWriter::new(WriterNodeId::new("test-node"), WriterEpoch(1)),
            WalSequence(seq),
        )
    }

    pub fn push_with_position(
        &mut self,
        event: &RawEvent,
        writer: &BufferWriter,
        sequence: WalSequence,
    ) -> Result<()> {
        if let Some(active_writer) = &self.active_writer
            && active_writer != writer
        {
            return Err(anyhow!(
                "buffer active generation belongs to node {} epoch {}, cannot append node {} epoch {}",
                active_writer.writer_node_id,
                active_writer.writer_epoch,
                writer.writer_node_id,
                writer.writer_epoch
            ));
        }
        if let Some(previous) = self.active_sequences.last()
            && sequence < *previous
        {
            return Err(anyhow!(
                "buffer WAL sequence regressed from {previous} to {sequence}"
            ));
        }

        // _timestamp 列：用 event.timestamp（domain Required）。
        let ts_col = self
            .columns
            .get_mut(TS_COL)
            .ok_or_else(|| anyhow!("internal: _timestamp column missing"))?;
        ts_col.append_timestamp_micros(event.timestamp.0)?;

        // 其余列：按 column_order 取，event 缺则 null。
        let mut bytes_estimate = 8usize; // timestamp
        for name in self.column_order.iter().skip(1) {
            let col = self
                .columns
                .get_mut(name)
                .ok_or_else(|| anyhow!("column {name} missing from builder map"))?;
            match event.fields.get(name) {
                Some(v) if !v.is_null() => {
                    if self.encrypted_cols.contains(name) {
                        let key = self.field_key.as_ref().ok_or_else(|| {
                            anyhow!("field `{name}` is encrypted but no field DEK injected")
                        })?;
                        let ct = encrypt_field(key, &value_to_plaintext(v))
                            .map_err(|e| anyhow!("encrypt field `{name}`: {e}"))?;
                        bytes_estimate += ct.len();
                        col.append_utf8(&ct)?;
                    } else {
                        bytes_estimate += col.append_json(v)?;
                    }
                }
                _ => {
                    col.append_null();
                    bytes_estimate += 1;
                }
            }
        }

        self.row_count += 1;
        self.approx_size_bytes += bytes_estimate;
        self.active_started_at.get_or_insert_with(Instant::now);
        self.active_writer.get_or_insert_with(|| writer.clone());
        self.active_sequences.push(sequence);
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
            self.encrypted_cols.insert(field.name.clone());
            ColumnBuilder::Utf8(StringBuilder::new())
        } else {
            ColumnBuilder::new(field.data_type)
        };
        for _ in 0..self.row_count {
            col.append_null();
        }
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
    pub fn sync_schema(&mut self, stream: &StreamDefinition) {
        for field in &stream.schema.fields {
            self.extend_schema(field);
        }
    }

    /// 无待 flush 或正在发布的数据。
    pub fn is_empty(&self) -> bool {
        self.row_count == 0 && self.pending.is_empty() && self.inflight.is_none()
    }
}

/// 加密字段写入前的明文串化：与 Utf8 列的常规追加一致（String 取内值，其余 JSON 串化），
/// 保证 `decrypt(col)` 还原出的明文与未加密时落库的字符串一致。
fn value_to_plaintext(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use arrow::array::Array;
    use serde_json::json;

    use super::*;
    use crate::{
        domain::stream::{Retention, Schema, StreamType},
        shared::{ids::Id, time::TimestampMicros},
    };

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
                        index_type: None,
                        indexed: true,
                        encrypted: false,
                        exact: false,
                    },
                    FieldDef {
                        name: "latency_ms".into(),
                        data_type: FieldType::Int64,
                        nullable: true,
                        index_type: None,
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
        let batch = b.begin_flush().unwrap().unwrap();
        assert_eq!(batch.batch.num_rows(), 2);

        // flush 失败：immutable generation 回到 retry 队列。
        b.fail_flush(&batch.flush_id()).unwrap();
        assert!(!b.is_empty(), "暂存的数据必须让 buffer 不为空");

        // flush 的 IO 期间又来了一条新写入
        let mut f = serde_json::Map::new();
        f.insert("level".into(), json!("c"));
        b.push(&raw_event(3_000_000, f), 2).unwrap();

        let retry = b.begin_flush().unwrap().unwrap();
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
            index_type: None,
            indexed: false,
            encrypted: false,
            exact: false,
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
            index_type: None,
            indexed: false,
            encrypted: false,
            exact: false,
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

    #[test]
    fn sync_schema_adds_all_new_fields_to_an_existing_generation() {
        let original = stream_def();
        let mut evolved = original.clone();
        evolved.schema.fields.extend([
            FieldDef {
                name: "trace_id".into(),
                data_type: FieldType::Utf8,
                nullable: true,
                index_type: None,
                indexed: false,
                encrypted: false,
                exact: false,
            },
            FieldDef {
                name: "duration_ns".into(),
                data_type: FieldType::Int64,
                nullable: true,
                index_type: None,
                indexed: false,
                encrypted: false,
                exact: false,
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
                crate::domain::storage::primary_dataset_type(s.stream_type).unwrap(),
            )
            .await
            .unwrap();
        let b1 = pool.get_or_create_dataset(&s, dataset.clone()).unwrap();
        let b2 = pool.get_or_create_dataset(&s, dataset).unwrap();
        assert!(Arc::ptr_eq(&b1, &b2));
    }

    fn stream_with_encrypted_field() -> StreamDefinition {
        StreamDefinition {
            id: Id::new(),
            org_id: Id::from_string("org-a"),
            name: "users".into(),
            stream_type: StreamType::LOGS,
            schema: Schema {
                fields: vec![FieldDef {
                    name: "email".into(),
                    data_type: FieldType::Utf8,
                    nullable: true,
                    index_type: None,
                    indexed: false,
                    encrypted: true,
                    exact: false,
                }],
            },
            retention: Some(Retention { days: 7 }),
            created_at: TimestampMicros::now(),
            updated_at: TimestampMicros::now(),
        }
    }

    fn test_dek() -> OrgFieldKey {
        OrgFieldKey {
            key_id: "key-1".into(),
            version: 1,
            raw_key: vec![4u8; 32],
        }
    }

    #[test]
    fn encrypted_field_is_sealed_as_ciphertext_column() {
        let dek = test_dek();
        let s = stream_with_encrypted_field();
        let mut b = RecordBuilder::new(&s);
        b.set_field_key(dek.clone());
        let mut f = serde_json::Map::new();
        f.insert("email".into(), json!("alice@example.com"));
        b.push(&raw_event(1_000_000, f), 1).unwrap();

        let batch = b.begin_flush().unwrap().unwrap();
        // 列类型固定 Utf8（密文）。
        let col = batch.batch.column_by_name("email").unwrap();
        let arr = col
            .as_any()
            .downcast_ref::<arrow::array::StringArray>()
            .unwrap();
        let stored = arr.value(0);
        assert!(
            stored.starts_with("kid:key-1:v1:"),
            "stored value must be DEK ciphertext, got {stored}"
        );
        assert_ne!(stored, "alice@example.com");
        // 用对应 DEK 可还原明文。
        let keys: std::collections::HashMap<String, Vec<u8>> =
            [("key-1".to_string(), vec![4u8; 32])].into_iter().collect();
        assert_eq!(
            crate::infra::cipher::decrypt_field(&keys, stored).as_deref(),
            Some("alice@example.com")
        );
    }

    #[test]
    fn encrypted_field_without_dek_errors_on_push() {
        let s = stream_with_encrypted_field();
        let mut b = RecordBuilder::new(&s); // no field key injected
        let mut f = serde_json::Map::new();
        f.insert("email".into(), json!("bob@example.com"));
        let err = b.push(&raw_event(1_000_000, f), 1).unwrap_err();
        assert!(
            err.to_string().contains("no field DEK"),
            "expected missing-DEK error, got {err}"
        );
    }
}
