// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 把一次 buffer snapshot 按事件所属 UTC 小时拆成独立 RecordBatch。

use std::collections::BTreeMap;

use arrow::{
    array::{Array, RecordBatch, TimestampMicrosecondArray, UInt32Array},
    compute::{SortColumn, SortOptions, lexsort_to_indices, take},
    datatypes::{DataType, TimeUnit},
};

use crate::{
    domain::{
        intake::EVENT_ID_FIELD,
        storage::{DatasetTypeId, PartitionPolicy, type_id::builtin},
    },
    infra::storage::arrow_schema::TS_COL,
    shared::{Error, Result, trace::summary::TRACE_SUMMARY_START_NS_FIELD},
};

pub fn split_by_utc_hour(batch: &RecordBatch) -> Result<Vec<RecordBatch>> {
    split_by_partition(batch, &PartitionPolicy::default())
}

/// Split a buffer generation according to the dataset's persisted partition policy.
pub fn split_by_partition(
    batch: &RecordBatch,
    partition_policy: &PartitionPolicy,
) -> Result<Vec<RecordBatch>> {
    if batch.num_rows() == 0 {
        return Ok(Vec::new());
    }
    let timestamp_index = batch
        .schema()
        .index_of(TS_COL)
        .map_err(|_| Error::internal(format!("batch missing column {TS_COL}")))?;
    let timestamp_column = batch.column(timestamp_index);
    let DataType::Timestamp(TimeUnit::Microsecond, _) = timestamp_column.data_type() else {
        return Err(Error::internal(format!(
            "{TS_COL} must be Timestamp(Microsecond), got {:?}",
            timestamp_column.data_type()
        )));
    };
    let timestamps = timestamp_column
        .as_any()
        .downcast_ref::<TimestampMicrosecondArray>()
        .ok_or_else(|| Error::internal(format!("downcast {TS_COL}")))?;

    let mut rows_by_partition: BTreeMap<i64, Vec<u32>> = BTreeMap::new();
    for row in 0..batch.num_rows() {
        if timestamps.is_null(row) {
            return Err(Error::invalid(format!(
                "{TS_COL} contains null at row {row}"
            )));
        }
        let row_index = u32::try_from(row)
            .map_err(|_| Error::invalid("record batch exceeds u32 row index capacity"))?;
        let partition = partition_policy.bucket_start_micros(timestamps.value(row));
        rows_by_partition
            .entry(partition)
            .or_default()
            .push(row_index);
    }

    let mut partitions = Vec::with_capacity(rows_by_partition.len());
    for indices in rows_by_partition.into_values() {
        let indices = UInt32Array::from(indices);
        let columns = batch
            .columns()
            .iter()
            .map(|column| {
                take(column.as_ref(), &indices, None)
                    .map_err(|error| Error::internal(format!("partition take: {error}")))
            })
            .collect::<Result<Vec<_>>>()?;
        partitions.push(
            RecordBatch::try_new(batch.schema(), columns)
                .map_err(|error| Error::internal(format!("partition batch: {error}")))?,
        );
    }
    Ok(partitions)
}

/// 物理文件内固定使用“时间倒序 + 唯一键倒序”。
///
/// 这不是查询排序的替代品，而是让最新文件优先扫描和 Cursor 边界具备稳定的物理顺序。
/// Compactor 合并时也会经过这里，因此不会破坏写入期建立的顺序。
pub fn sort_for_storage(batch: RecordBatch, dataset_type: &DatasetTypeId) -> Result<RecordBatch> {
    if batch.num_rows() <= 1 {
        return Ok(batch);
    }

    let schema = batch.schema();
    let names = storage_sort_column_names(schema.as_ref(), Some(dataset_type));
    let options = SortOptions {
        descending: true,
        nulls_first: false,
    };
    let sort_columns = names
        .into_iter()
        .filter_map(|name| schema.index_of(name).ok())
        .map(|index| SortColumn {
            values: batch.column(index).clone(),
            options: Some(options),
        })
        .collect::<Vec<_>>();
    let indices = lexsort_to_indices(&sort_columns, None)
        .map_err(|error| Error::internal(format!("storage lexsort: {error}")))?;
    let columns = batch
        .columns()
        .iter()
        .map(|column| {
            take(column.as_ref(), &indices, None)
                .map_err(|error| Error::internal(format!("storage sort take: {error}")))
        })
        .collect::<Result<Vec<_>>>()?;
    RecordBatch::try_new(schema, columns)
        .map_err(|error| Error::internal(format!("storage sorted batch: {error}")))
}

/// Writer 与查询侧共享同一份物理排序声明，防止 Parquet 实际顺序和
/// `FileScanConfig.output_ordering` 漂移后产生错误的有序合并结果。
pub(crate) fn storage_sort_column_names(
    schema: &arrow::datatypes::Schema,
    dataset_type: Option<&DatasetTypeId>,
) -> Vec<&'static str> {
    let mut names = match dataset_type.map(DatasetTypeId::as_str) {
        // `_timestamp` only has microsecond precision. The trace cursor uses
        // nanoseconds, so use the exact cursor field as the physical primary
        // key and only fall back to `_timestamp` when a malformed row lacks it.
        Some(builtin::DATASET_TRACE_SUMMARY)
            if schema.index_of(TRACE_SUMMARY_START_NS_FIELD).is_ok() =>
        {
            vec![TRACE_SUMMARY_START_NS_FIELD]
        }
        _ => vec![TS_COL],
    };
    match dataset_type.map(DatasetTypeId::as_str) {
        Some(builtin::DATASET_TRACE_SUMMARY) => names.extend(["trace_id"]),
        Some(builtin::DATASET_RUM_SESSION_SUMMARY) => {
            if schema.index_of("session.id").is_ok() {
                names.push("session.id");
            } else {
                names.push("session_id");
            }
            names.push(EVENT_ID_FIELD);
        }
        Some(builtin::DATASET_RUM_ACTION_SUMMARY) => {
            names.push("session_id");
            names.push(EVENT_ID_FIELD);
        }
        Some(builtin::DATASET_RUM_ERROR_SUMMARY) => {
            if schema.index_of("error.id").is_ok() {
                names.push("error.id");
            } else if schema.index_of("error_id").is_ok() {
                names.push("error_id");
            }
            names.push(EVENT_ID_FIELD);
        }
        _ => names.extend([EVENT_ID_FIELD, "trace_id", "span_id"]),
    }
    names
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::{
        array::{Int64Array, StringArray},
        datatypes::{Field, Schema},
    };

    use super::*;

    #[test]
    fn splits_rows_at_hour_boundary_without_reordering_each_partition() {
        let schema = Arc::new(Schema::new(vec![
            Field::new(
                TS_COL,
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
            Field::new("value", DataType::Int64, false),
        ]));
        let timestamps = TimestampMicrosecondArray::from(vec![3_600_000_002, 1, 3_600_000_001, 2])
            .with_timezone("UTC");
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(timestamps),
                Arc::new(Int64Array::from(vec![20, 10, 21, 11])),
            ],
        )
        .unwrap();

        let split = split_by_utc_hour(&batch).unwrap();
        assert_eq!(split.len(), 2);
        assert_eq!(
            split[0]
                .column(1)
                .as_any()
                .downcast_ref::<Int64Array>()
                .unwrap()
                .values(),
            &[10, 11]
        );
        assert_eq!(
            split[1]
                .column(1)
                .as_any()
                .downcast_ref::<Int64Array>()
                .unwrap()
                .values(),
            &[20, 21]
        );
    }

    #[test]
    fn storage_sort_uses_timestamp_then_unique_key_descending() {
        let schema = Arc::new(Schema::new(vec![
            Field::new(
                TS_COL,
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
            Field::new("trace_id", DataType::Utf8, false),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(TimestampMicrosecondArray::from(vec![10, 20, 20]).with_timezone("UTC")),
                Arc::new(StringArray::from(vec!["a", "b", "c"])),
            ],
        )
        .unwrap();

        let sorted = sort_for_storage(
            batch,
            &DatasetTypeId::builtin(builtin::DATASET_TRACE_SUMMARY),
        )
        .unwrap();
        let trace_ids = sorted
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(trace_ids.value(0), "c");
        assert_eq!(trace_ids.value(1), "b");
        assert_eq!(trace_ids.value(2), "a");
    }
}
