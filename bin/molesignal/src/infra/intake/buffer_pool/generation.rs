// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Buffer generation identity and immutable query/flush snapshots.

use std::sync::Arc;

use anyhow::{Result, anyhow};
use arrow::{
    array::{
        Array, ArrayRef, BooleanArray, RecordBatch, TimestampMicrosecondArray, new_null_array,
    },
    compute::filter_record_batch,
    datatypes::{DataType, Schema as ArrowSchema, TimeUnit},
};

use super::{ColumnBuilder, RecordBuilder};
use crate::{
    domain::storage::{
        FlushId, FlushProvenance, SequenceRange, WalSequence, WriterEpoch, WriterNodeId,
    },
    infra::storage::arrow_schema::TS_COL,
    shared::time::TimeRange,
};

/// WAL sequence namespace carried by one buffer generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BufferWriter {
    pub writer_node_id: WriterNodeId,
    pub writer_epoch: WriterEpoch,
}

impl BufferWriter {
    pub fn new(writer_node_id: WriterNodeId, writer_epoch: WriterEpoch) -> Self {
        Self {
            writer_node_id,
            writer_epoch,
        }
    }
}

/// Immutable Arrow generation kept visible while it is being published.
///
/// `sequences` is a row-aligned sidecar rather than a user-visible Arrow column. A single WAL
/// record may contain several events, so adjacent rows can legitimately carry the same sequence.
#[derive(Clone)]
pub struct BufferedRecordBatch {
    pub batch: RecordBatch,
    pub writer: BufferWriter,
    sequences: Arc<[WalSequence]>,
    accounted_size_bytes: usize,
    approximate_size_bytes: usize,
}

impl std::fmt::Debug for BufferedRecordBatch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BufferedRecordBatch")
            .field("rows", &self.batch.num_rows())
            .field("writer", &self.writer)
            .field("sequence_range", &self.sequence_range())
            .field("accounted_size_bytes", &self.accounted_size_bytes)
            .finish()
    }
}

impl BufferedRecordBatch {
    pub(super) fn new(
        batch: RecordBatch,
        writer: BufferWriter,
        sequences: Vec<WalSequence>,
        accounted_size_bytes: usize,
        approximate_size_bytes: usize,
    ) -> Result<Self> {
        if batch.num_rows() == 0 {
            return Err(anyhow!("buffer generation must not be empty"));
        }
        if batch.num_rows() != sequences.len() {
            return Err(anyhow!(
                "buffer sequence sidecar has {} rows for {} Arrow rows",
                sequences.len(),
                batch.num_rows()
            ));
        }
        if sequences.windows(2).any(|pair| pair[0] > pair[1]) {
            return Err(anyhow!("buffer WAL sequences are not monotonic"));
        }
        Ok(Self {
            batch,
            writer,
            sequences: sequences.into(),
            accounted_size_bytes,
            approximate_size_bytes,
        })
    }

    pub fn sequence_range(&self) -> SequenceRange {
        SequenceRange::new(
            *self
                .sequences
                .first()
                .expect("non-empty generation has first sequence"),
            *self
                .sequences
                .last()
                .expect("non-empty generation has last sequence"),
        )
    }

    pub fn provenance(&self) -> FlushProvenance {
        FlushProvenance::derive(
            self.writer.writer_node_id.clone(),
            self.writer.writer_epoch,
            self.sequence_range(),
        )
    }

    pub fn flush_id(&self) -> FlushId {
        self.provenance().flush_id
    }

    pub fn accounted_size_bytes(&self) -> usize {
        self.accounted_size_bytes
    }

    pub fn approximate_size_bytes(&self) -> usize {
        self.approximate_size_bytes
    }

    /// Apply the query snapshot's checkpoint and absolute time range without exposing WAL
    /// sequence as a table column.
    pub fn visible_after(
        &self,
        committed_sequence: WalSequence,
        time_range: TimeRange,
    ) -> Result<Option<RecordBatch>> {
        let timestamp_index = self
            .batch
            .schema()
            .index_of(TS_COL)
            .map_err(|_| anyhow!("buffer batch missing {TS_COL}"))?;
        let timestamp_column = self.batch.column(timestamp_index);
        let DataType::Timestamp(TimeUnit::Microsecond, _) = timestamp_column.data_type() else {
            return Err(anyhow!(
                "buffer {TS_COL} must be Timestamp(Microsecond), got {:?}",
                timestamp_column.data_type()
            ));
        };
        let timestamps = timestamp_column
            .as_any()
            .downcast_ref::<TimestampMicrosecondArray>()
            .ok_or_else(|| anyhow!("downcast buffer {TS_COL}"))?;

        let mut all_visible = true;
        let mut any_visible = false;
        let mask = self
            .sequences
            .iter()
            .enumerate()
            .map(|(row, sequence)| {
                let visible = !timestamps.is_null(row)
                    && *sequence > committed_sequence
                    && timestamps.value(row) >= time_range.start.0
                    && timestamps.value(row) < time_range.end.0;
                all_visible &= visible;
                any_visible |= visible;
                visible
            })
            .collect::<Vec<_>>();
        if !any_visible {
            return Ok(None);
        }
        if all_visible {
            return Ok(Some(self.batch.clone()));
        }
        filter_record_batch(&self.batch, &BooleanArray::from(mask))
            .map(Some)
            .map_err(|error| anyhow!("filter buffer query snapshot: {error}"))
    }
}

impl RecordBuilder {
    /// Rotate the oldest retry generation, or the current active generation, into in-flight.
    /// The in-flight clone stays owned by the builder so query snapshots cannot observe a gap.
    pub fn begin_flush(&mut self) -> Result<Option<BufferedRecordBatch>> {
        if self.inflight.is_some() {
            return Err(anyhow!("buffer generation is already being flushed"));
        }
        let generation = match self.pending.pop_front() {
            Some(generation) => generation,
            None if self.row_count > 0 => self.finish_active()?,
            None => return Ok(None),
        };
        self.inflight = Some(generation.clone());
        Ok(Some(generation))
    }

    /// Catalog publication failed; keep the immutable generation for the next retry.
    pub fn fail_flush(&mut self, flush_id: &FlushId) -> Result<()> {
        let generation = self.take_inflight(flush_id)?;
        self.pending.push_front(generation);
        Ok(())
    }

    /// Catalog commit succeeded; retire the exact in-flight generation and return its memory
    /// reservation for process-wide accounting release.
    pub fn complete_flush(&mut self, flush_id: &FlushId) -> Result<usize> {
        Ok(self.take_inflight(flush_id)?.accounted_size_bytes())
    }

    /// Snapshot retry, in-flight, and active generations without mutating writer state.
    pub fn query_snapshot(&self) -> Result<Vec<BufferedRecordBatch>> {
        let mut batches = Vec::with_capacity(
            self.pending.len()
                + usize::from(self.inflight.is_some())
                + usize::from(self.row_count > 0),
        );
        batches.extend(self.pending.iter().cloned());
        batches.extend(self.inflight.iter().cloned());
        if self.row_count > 0 {
            batches.push(self.snapshot_active()?);
        }
        Ok(batches)
    }

    fn finish_active(&mut self) -> Result<BufferedRecordBatch> {
        let arrays = self
            .column_order
            .iter()
            .map(|name| {
                self.columns
                    .get_mut(name)
                    .ok_or_else(|| anyhow!("column {name} missing on finish"))
                    .map(ColumnBuilder::finish)
            })
            .collect::<Result<Vec<_>>>()?;
        let batch = RecordBatch::try_new(Arc::new(ArrowSchema::new(self.fields.clone())), arrays)
            .map_err(|error| anyhow!("RecordBatch::try_new failed: {error}"))?;
        let writer = self
            .active_writer
            .take()
            .ok_or_else(|| anyhow!("active buffer rows have no WAL writer identity"))?;
        let sequences = std::mem::take(&mut self.active_sequences);
        let accounted_size_bytes = std::mem::take(&mut self.accounted_size_bytes);
        let approximate_size_bytes = std::mem::take(&mut self.approx_size_bytes);
        self.row_count = 0;
        self.active_started_at = None;
        BufferedRecordBatch::new(
            batch,
            writer,
            sequences,
            accounted_size_bytes,
            approximate_size_bytes,
        )
    }

    fn snapshot_active(&self) -> Result<BufferedRecordBatch> {
        let arrays = self
            .column_order
            .iter()
            .map(|name| {
                self.columns
                    .get(name)
                    .ok_or_else(|| anyhow!("column {name} missing on snapshot"))
                    .map(ColumnBuilder::finish_cloned)
            })
            .collect::<Result<Vec<_>>>()?;
        let batch = RecordBatch::try_new(Arc::new(ArrowSchema::new(self.fields.clone())), arrays)
            .map_err(|error| anyhow!("RecordBatch snapshot failed: {error}"))?;
        BufferedRecordBatch::new(
            batch,
            self.active_writer
                .clone()
                .ok_or_else(|| anyhow!("active buffer rows have no WAL writer identity"))?,
            self.active_sequences.clone(),
            self.accounted_size_bytes,
            self.approx_size_bytes,
        )
    }

    fn take_inflight(&mut self, flush_id: &FlushId) -> Result<BufferedRecordBatch> {
        let generation = self
            .inflight
            .take()
            .ok_or_else(|| anyhow!("buffer has no in-flight generation"))?;
        if generation.flush_id() != *flush_id {
            self.inflight = Some(generation);
            return Err(anyhow!(
                "in-flight buffer generation does not match flush {flush_id}"
            ));
        }
        Ok(generation)
    }
}

/// 把暂存的历史 batch 转换为当前 schema：缺的列整列补 null。
pub(crate) fn align_to_schema(
    batch: RecordBatch,
    target: &Arc<ArrowSchema>,
) -> Result<RecordBatch> {
    if batch.schema() == *target {
        return Ok(batch);
    }
    let rows = batch.num_rows();
    let source = batch.schema();
    let mut columns: Vec<ArrayRef> = Vec::with_capacity(target.fields().len());
    for field in target.fields() {
        match source.index_of(field.name()) {
            Ok(index) => columns.push(batch.column(index).clone()),
            Err(_) => columns.push(new_null_array(field.data_type(), rows)),
        }
    }
    RecordBatch::try_new(target.clone(), columns)
        .map_err(|error| anyhow!("align pending batch to evolved schema: {error}"))
}
