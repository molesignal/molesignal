// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Trace 摘要物理数据集的跨功能读取器。

use std::{
    cmp::Ordering,
    collections::{HashSet, VecDeque},
    sync::Arc,
};

use arrow::{
    array::{Array, Int64Array, RecordBatch, StringArray, UInt64Array},
    datatypes::DataType,
};
use futures::StreamExt;
use object_store::ObjectStore;

use crate::{
    domain::{
        storage::{ParquetFileMetaRepository, PhysicalDatasetKind},
        stream::StreamType,
    },
    infra::storage::parquet::reader::{ParquetReader, ReadOptions},
    shared::{
        Error, Result,
        ids::Id,
        time::TimeRange,
        trace::summary::{
            TRACE_SUMMARY_DURATION_NS_FIELD, TRACE_SUMMARY_SPAN_COUNT_FIELD,
            TRACE_SUMMARY_START_NS_FIELD,
        },
    },
};

const COLUMNS: &[&str] = &[
    "trace_id",
    "service.name",
    TRACE_SUMMARY_START_NS_FIELD,
    TRACE_SUMMARY_DURATION_NS_FIELD,
    TRACE_SUMMARY_SPAN_COUNT_FIELD,
];

#[derive(Clone, Debug, PartialEq)]
pub struct TraceSummaryRecord {
    pub trace_id: String,
    pub service: Option<String>,
    pub start_ns: i64,
    pub duration_ns: i64,
    pub span_count: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SummaryOrder {
    Latest,
    Earliest,
}

#[derive(Clone, Copy, Debug)]
pub struct TraceSummaryQuery<'a> {
    pub trace_ids: Option<&'a HashSet<String>>,
    pub require_contained: bool,
    pub order: SummaryOrder,
    pub limit: usize,
}

pub struct TraceSummaryReader {
    files: Arc<dyn ParquetFileMetaRepository>,
    object_store: Arc<dyn ObjectStore>,
}

impl TraceSummaryReader {
    pub fn new(
        files: Arc<dyn ParquetFileMetaRepository>,
        object_store: Arc<dyn ObjectStore>,
    ) -> Self {
        Self {
            files,
            object_store,
        }
    }

    /// 从独立 `trace_summary` 数据集读取，不回退扫描原始 Span。
    /// `trace_ids` 非空时找齐 ID 即停；窗口查询使用有界 Top-K。
    pub async fn scan(
        &self,
        org_id: &Id,
        stream: &str,
        range: TimeRange,
        query: TraceSummaryQuery<'_>,
    ) -> Result<Vec<TraceSummaryRecord>> {
        let TraceSummaryQuery {
            trace_ids,
            require_contained,
            order,
            limit,
        } = query;
        if limit == 0 || trace_ids.is_some_and(HashSet::is_empty) {
            return Ok(Vec::new());
        }
        let mut files = self
            .files
            .find_dataset(
                org_id,
                stream,
                StreamType::Traces,
                PhysicalDatasetKind::TraceSummary,
                range,
            )
            .await?;
        files.sort_by(|left, right| match order {
            SummaryOrder::Latest => right
                .time_range
                .end
                .cmp(&left.time_range.end)
                .then_with(|| right.object_key.cmp(&left.object_key)),
            SummaryOrder::Earliest => left
                .time_range
                .start
                .cmp(&right.time_range.start)
                .then_with(|| left.object_key.cmp(&right.object_key)),
        });

        let reader = ParquetReader::new(self.object_store.clone());
        let from_ns = range
            .start
            .0
            .checked_mul(1_000)
            .ok_or_else(|| Error::invalid("trace range start is out of range"))?;
        let to_ns = range
            .end
            .0
            .checked_mul(1_000)
            .ok_or_else(|| Error::invalid("trace range end is out of range"))?;
        let mut files = VecDeque::from(files);
        let mut rows = Vec::with_capacity(limit.min(trace_ids.map_or(limit, HashSet::len)));
        let mut found = HashSet::new();

        'files: while let Some(file) = files.pop_front() {
            let options = ReadOptions::new()
                .with_time_range(range.start.0, range.end.0)
                .with_columns(COLUMNS)
                .with_known_size(file.size_bytes);
            let mut batches = match reader
                .stream_from_store(self.object_store.clone(), &file.object_key, options)
                .await
            {
                Ok(batches) => batches,
                Err(Error::NotFound(_)) => {
                    tracing::warn!(object_key = %file.object_key, "trace summary parquet is missing");
                    continue;
                }
                Err(error) => return Err(error),
            };
            while let Some(batch) = batches.next().await {
                let batch = batch?;
                for row in 0..batch.num_rows() {
                    let Some(trace_id) = string_at(&batch, "trace_id", row) else {
                        continue;
                    };
                    if let Some(ids) = trace_ids
                        && (!ids.contains(trace_id) || !found.insert(trace_id.to_string()))
                    {
                        continue;
                    }
                    let start_ns =
                        integer_at(&batch, TRACE_SUMMARY_START_NS_FIELD, row).unwrap_or_default();
                    let duration_ns = integer_at(&batch, TRACE_SUMMARY_DURATION_NS_FIELD, row)
                        .unwrap_or_default()
                        .max(0);
                    if start_ns < from_ns
                        || start_ns >= to_ns
                        || (require_contained && start_ns.saturating_add(duration_ns) > to_ns)
                    {
                        found.remove(trace_id);
                        continue;
                    }
                    let candidate = TraceSummaryRecord {
                        trace_id: trace_id.to_string(),
                        service: string_at(&batch, "service.name", row)
                            .filter(|value| !value.is_empty())
                            .map(str::to_string),
                        start_ns,
                        duration_ns,
                        span_count: integer_at(&batch, TRACE_SUMMARY_SPAN_COUNT_FIELD, row)
                            .unwrap_or_default()
                            .max(0) as u64,
                    };
                    if trace_ids.is_some() {
                        rows.push(candidate);
                    } else {
                        push_top_k(&mut rows, candidate, limit, order);
                    }
                    if trace_ids.is_some_and(|ids| found.len() == ids.len()) {
                        break 'files;
                    }
                }
            }
            if trace_ids.is_none()
                && rows.len() >= limit
                && files
                    .front()
                    .is_some_and(|next| file_cannot_beat(next, &rows, order))
            {
                break;
            }
        }

        rows.sort_by(|left, right| compare(left, right, order));
        rows.truncate(limit);
        Ok(rows)
    }
}

fn compare(left: &TraceSummaryRecord, right: &TraceSummaryRecord, order: SummaryOrder) -> Ordering {
    match order {
        SummaryOrder::Latest => right
            .start_ns
            .cmp(&left.start_ns)
            .then_with(|| right.trace_id.cmp(&left.trace_id)),
        SummaryOrder::Earliest => left
            .start_ns
            .cmp(&right.start_ns)
            .then_with(|| left.trace_id.cmp(&right.trace_id)),
    }
}

fn push_top_k(
    rows: &mut Vec<TraceSummaryRecord>,
    candidate: TraceSummaryRecord,
    limit: usize,
    order: SummaryOrder,
) {
    if let Some(existing) = rows
        .iter()
        .position(|row| row.trace_id == candidate.trace_id)
    {
        if compare(&candidate, &rows[existing], order) == Ordering::Less {
            rows[existing] = candidate;
        }
        return;
    }
    if rows.len() < limit {
        rows.push(candidate);
        return;
    }
    let worst = rows
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| compare(left, right, order))
        .map(|(index, _)| index)
        .unwrap_or_default();
    if compare(&candidate, &rows[worst], order) == Ordering::Less {
        rows[worst] = candidate;
    }
}

fn file_cannot_beat(
    file: &crate::domain::storage::ParquetFileMeta,
    rows: &[TraceSummaryRecord],
    order: SummaryOrder,
) -> bool {
    let Some(worst) = rows
        .iter()
        .max_by(|left, right| compare(left, right, order))
    else {
        return false;
    };
    match order {
        SummaryOrder::Latest => {
            file.max_values
                .get(TRACE_SUMMARY_START_NS_FIELD)
                .and_then(serde_json::Value::as_i64)
                .unwrap_or_else(|| {
                    file.time_range
                        .end
                        .0
                        .saturating_mul(1_000)
                        .saturating_add(999)
                })
                < worst.start_ns
        }
        SummaryOrder::Earliest => {
            file.min_values
                .get(TRACE_SUMMARY_START_NS_FIELD)
                .and_then(serde_json::Value::as_i64)
                .unwrap_or_else(|| file.time_range.start.0.saturating_mul(1_000))
                > worst.start_ns
        }
    }
}

fn string_at<'a>(batch: &'a RecordBatch, name: &str, row: usize) -> Option<&'a str> {
    let column = batch.column_by_name(name)?;
    let values = column.as_any().downcast_ref::<StringArray>()?;
    (!values.is_null(row)).then(|| values.value(row))
}

fn integer_at(batch: &RecordBatch, name: &str, row: usize) -> Option<i64> {
    let column = batch.column_by_name(name)?;
    if column.is_null(row) {
        return None;
    }
    match column.data_type() {
        DataType::Int64 => Some(column.as_any().downcast_ref::<Int64Array>()?.value(row)),
        DataType::UInt64 => {
            i64::try_from(column.as_any().downcast_ref::<UInt64Array>()?.value(row)).ok()
        }
        DataType::Utf8 => column
            .as_any()
            .downcast_ref::<StringArray>()?
            .value(row)
            .parse()
            .ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_order_is_stable_on_trace_id() {
        let mut rows = [
            TraceSummaryRecord {
                trace_id: "a".into(),
                service: None,
                start_ns: 10,
                duration_ns: 1,
                span_count: 1,
            },
            TraceSummaryRecord {
                trace_id: "b".into(),
                service: None,
                start_ns: 10,
                duration_ns: 1,
                span_count: 1,
            },
        ];
        rows.sort_by(|left, right| {
            right
                .start_ns
                .cmp(&left.start_ns)
                .then_with(|| right.trace_id.cmp(&left.trace_id))
        });
        assert_eq!(rows[0].trace_id, "b");
    }

    #[test]
    fn top_k_collapses_duplicate_trace_summary_rows() {
        let mut rows = Vec::new();
        let row = |start_ns| TraceSummaryRecord {
            trace_id: "same".into(),
            service: None,
            start_ns,
            duration_ns: 1,
            span_count: 1,
        };
        push_top_k(&mut rows, row(10), 2, SummaryOrder::Latest);
        push_top_k(&mut rows, row(20), 2, SummaryOrder::Latest);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].start_ns, 20);
    }
}
