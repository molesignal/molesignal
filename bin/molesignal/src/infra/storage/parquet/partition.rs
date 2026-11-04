// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 把一次 buffer snapshot 按事件所属 UTC 小时拆成独立 RecordBatch。

use std::collections::BTreeMap;

use arrow::{
    array::{Array, RecordBatch, StringArray, TimestampMicrosecondArray, UInt32Array},
    compute::{SortColumn, SortOptions, lexsort_to_indices, take},
    datatypes::{DataType, TimeUnit},
};
use datafusion::physical_expr::{LexOrdering, PhysicalSortExpr, expressions::col as physical_col};

use crate::{
    domain::{
        intake::EVENT_ID_FIELD,
        metrics::METRIC_NAME_FIELD,
        rum::event::Versioned,
        storage::{DatasetTypeId, PartitionPolicy, type_id::builtin},
        stream::FieldPartitions,
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

    rows_by_partition
        .into_values()
        .map(|indices| take_rows(batch, indices))
        .collect()
}

/// 再按分片字段把一个时间分区的行分进各自的分片，分片号从小到大。批里没有的分片字段，值都是
/// 空的。分片字段不是文本列时分不了，为 `None`：这批数据不分片写入。
pub fn split_by_shard(
    batch: &RecordBatch,
    partitions: &FieldPartitions,
) -> Result<Option<Vec<(u16, RecordBatch)>>> {
    let schema = batch.schema();
    let mut columns = Vec::with_capacity(partitions.fields().len());

    for partition in partitions.fields() {
        let column = match schema.index_of(&partition.field) {
            Ok(index) => match batch.column(index).as_any().downcast_ref::<StringArray>() {
                Some(values) => Some(values),
                None => return Ok(None),
            },
            Err(_) => None,
        };

        columns.push(column);
    }

    let mut rows_by_shard: BTreeMap<u16, Vec<u32>> = BTreeMap::new();

    for row in 0..batch.num_rows() {
        let row_index = u32::try_from(row)
            .map_err(|_| Error::invalid("record batch exceeds u32 row index capacity"))?;

        let shard = partitions.shard_of(columns.iter().map(|column| {
            column
                .filter(|values| !values.is_null(row))
                .map(|values| values.value(row))
        }));

        rows_by_shard.entry(shard).or_default().push(row_index);
    }

    if rows_by_shard.len() == 1 {
        let shard = *rows_by_shard.keys().next().expect("one shard");
        return Ok(Some(vec![(shard, batch.clone())]));
    }

    rows_by_shard
        .into_iter()
        .map(|(shard, indices)| Ok((shard, take_rows(batch, indices)?)))
        .collect::<Result<Vec<_>>>()
        .map(Some)
}

/// `batch` 里 `indices` 这些行，按给出的顺序。
fn take_rows(batch: &RecordBatch, indices: Vec<u32>) -> Result<RecordBatch> {
    let indices = UInt32Array::from(indices);

    let columns = batch
        .columns()
        .iter()
        .map(|column| {
            take(column.as_ref(), &indices, None)
                .map_err(|error| Error::internal(format!("partition take: {error}")))
        })
        .collect::<Result<Vec<_>>>()?;

    RecordBatch::try_new(batch.schema(), columns)
        .map_err(|error| Error::internal(format!("partition batch: {error}")))
}

/// 物理文件内固定使用“时间倒序 + 唯一键倒序”；指标样本先按 `metric_name` 倒序，同一指标的
/// 行连在一起，查一个指标只读它的那几页。
///
/// 这不是查询排序的替代品，而是让最新文件优先扫描和 Cursor 边界具备稳定的物理顺序。
/// Compactor 合并时也会经过这里，因此不会破坏写入期建立的顺序。
pub fn sort_for_storage(batch: RecordBatch, dataset_type: &DatasetTypeId) -> Result<RecordBatch> {
    if batch.num_rows() <= 1 {
        return Ok(batch);
    }

    let schema = batch.schema();
    let names = storage_sort_column_names(schema.as_ref(), Some(dataset_type));

    let sort_columns = names
        .into_iter()
        .filter_map(|name| schema.index_of(name).ok())
        .map(|index| SortColumn {
            values: batch.column(index).clone(),
            options: Some(STORAGE_SORT),
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

/// 物理文件里每个排序列的方向：倒序，null 在最后。
pub(crate) const STORAGE_SORT: SortOptions = SortOptions {
    descending: true,
    nulls_first: false,
};

/// 物理文件的排序，作为 `schema` 上的排序表达式：查询侧把它声明为每个文件的输出顺序，
/// Compactor 按它归并多个文件。
pub(crate) fn storage_ordering(
    schema: &arrow::datatypes::Schema,
    dataset_type: Option<&DatasetTypeId>,
) -> Option<LexOrdering> {
    let columns = storage_sort_column_names(schema, dataset_type)
        .into_iter()
        .filter_map(|name| physical_col(name, schema).ok())
        .map(|expression| PhysicalSortExpr::new(expression, STORAGE_SORT))
        .collect::<Vec<_>>();

    LexOrdering::new(columns)
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
        // A metrics stream holds the metrics of one source: keeping each metric's rows together
        // lets a query read only its pages.
        Some(builtin::DATASET_METRIC_SAMPLES | builtin::DATASET_METRIC_ROLLUP) => {
            vec![METRIC_NAME_FIELD, TS_COL]
        }
        _ => vec![TS_COL],
    };

    // Rows that report the same entity again (a RUM view, a RUM session) sit next to each other,
    // the highest revision first, so that the compactor can keep one row of each.
    if let Some(versioned) = Versioned::declared_by(|name| schema.index_of(name).is_ok()) {
        names.extend([versioned.key, versioned.revision, EVENT_ID_FIELD]);
        return names;
    }

    match dataset_type.map(DatasetTypeId::as_str) {
        Some(builtin::DATASET_TRACE_SUMMARY) => names.extend(["trace_id"]),
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
    use crate::domain::stream::{FieldPartition, PartitionScheme};

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
    fn metric_samples_sort_by_metric_then_time() {
        let schema = Arc::new(Schema::new(vec![
            Field::new(
                TS_COL,
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
            Field::new(METRIC_NAME_FIELD, DataType::Utf8, true),
        ]));

        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(
                    TimestampMicrosecondArray::from(vec![30, 10, 20, 40]).with_timezone("UTC"),
                ),
                Arc::new(StringArray::from(vec!["b", "a", "b", "a"])),
            ],
        )
        .unwrap();

        let sorted = sort_for_storage(
            batch,
            &DatasetTypeId::builtin(builtin::DATASET_METRIC_SAMPLES),
        )
        .unwrap();

        let names = sorted
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();

        let times = sorted
            .column(0)
            .as_any()
            .downcast_ref::<TimestampMicrosecondArray>()
            .unwrap();

        assert_eq!(
            (0..4)
                .map(|row| (names.value(row), times.value(row)))
                .collect::<Vec<_>>(),
            [("b", 30), ("b", 20), ("a", 40), ("a", 10)]
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

    fn versioned_batch() -> RecordBatch {
        let schema = Arc::new(Schema::new(vec![
            Field::new(
                TS_COL,
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
            Field::new("view.id", DataType::Utf8, true),
            Field::new("view.revision", DataType::Int64, true),
            Field::new(EVENT_ID_FIELD, DataType::Utf8, true),
        ]));

        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(
                    TimestampMicrosecondArray::from(vec![10, 10, 10, 10]).with_timezone("UTC"),
                ),
                Arc::new(StringArray::from(vec!["a", "a", "b", "a"])),
                Arc::new(Int64Array::from(vec![1, 3, 1, 2])),
                Arc::new(StringArray::from(vec!["e1", "e2", "e3", "e4"])),
            ],
        )
        .unwrap()
    }

    #[test]
    fn the_rows_of_a_versioned_entity_sit_together_with_the_highest_revision_first() {
        let batch = versioned_batch();

        assert_eq!(
            storage_sort_column_names(
                batch.schema().as_ref(),
                Some(&DatasetTypeId::builtin(builtin::DATASET_LOG_RECORDS))
            ),
            vec![TS_COL, "view.id", "view.revision", EVENT_ID_FIELD]
        );

        let sorted =
            sort_for_storage(batch, &DatasetTypeId::builtin(builtin::DATASET_LOG_RECORDS)).unwrap();

        let ids = sorted
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let revisions = sorted
            .column(2)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();

        let order = (0..4)
            .map(|row| (ids.value(row), revisions.value(row)))
            .collect::<Vec<_>>();

        assert_eq!(order, vec![("b", 1), ("a", 3), ("a", 2), ("a", 1)]);
    }

    #[test]
    fn a_dataset_without_a_revision_column_is_not_versioned() {
        let schema = Schema::new(vec![
            Field::new(
                TS_COL,
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
            Field::new("view.id", DataType::Utf8, true),
            Field::new(EVENT_ID_FIELD, DataType::Utf8, true),
        ]);

        assert_eq!(
            storage_sort_column_names(
                &schema,
                Some(&DatasetTypeId::builtin(builtin::DATASET_LOG_RECORDS))
            ),
            vec![TS_COL, EVENT_ID_FIELD, "trace_id", "span_id"]
        );
    }

    fn text(values: &StringArray, row: usize) -> Option<&str> {
        (!values.is_null(row)).then(|| values.value(row))
    }

    #[test]
    fn rows_land_in_the_shard_of_their_partition_values() {
        let schema = Arc::new(Schema::new(vec![
            Field::new("service", DataType::Utf8, true),
            Field::new("pod", DataType::Utf8, true),
        ]));
        let services = [
            Some("api"),
            Some("web"),
            Some("api"),
            Some("db"),
            Some("web"),
            None,
        ];
        let pods = [
            Some("api-1"),
            Some("web-2"),
            Some("0f"),
            None,
            Some("Web-3"),
            Some("x"),
        ];

        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(StringArray::from(services.to_vec())),
                Arc::new(StringArray::from(pods.to_vec())),
            ],
        )
        .unwrap();

        let partitions = FieldPartitions::new(vec![
            FieldPartition {
                field: "service".into(),
                scheme: PartitionScheme::Hash { buckets: 4 },
            },
            FieldPartition {
                field: "pod".into(),
                scheme: PartitionScheme::Prefix,
            },
        ]);

        let shards = split_by_shard(&batch, &partitions).unwrap().unwrap();

        assert_eq!(
            shards
                .iter()
                .map(|(_, batch)| batch.num_rows())
                .sum::<usize>(),
            6
        );
        assert!(shards.windows(2).all(|pair| pair[0].0 < pair[1].0));

        for (shard, batch) in &shards {
            let column = |index: usize| {
                batch
                    .column(index)
                    .as_any()
                    .downcast_ref::<StringArray>()
                    .unwrap()
            };

            for row in 0..batch.num_rows() {
                assert_eq!(
                    partitions.shard_of([text(column(0), row), text(column(1), row)]),
                    *shard
                );
            }
        }
    }

    #[test]
    fn a_batch_without_the_field_is_one_shard_and_a_non_text_field_none() {
        let partition = FieldPartitions::new(vec![FieldPartition {
            field: "service".into(),
            scheme: PartitionScheme::Prefix,
        }]);

        let other = RecordBatch::try_new(
            Arc::new(Schema::new(vec![Field::new("level", DataType::Utf8, true)])),
            vec![Arc::new(StringArray::from(vec!["info"]))],
        )
        .unwrap();

        let shards = split_by_shard(&other, &partition).unwrap().unwrap();
        assert_eq!(shards.len(), 1);
        assert_eq!(shards[0].0, 0);

        let numbers = RecordBatch::try_new(
            Arc::new(Schema::new(vec![Field::new(
                "service",
                DataType::Int64,
                true,
            )])),
            vec![Arc::new(arrow::array::Int64Array::from(vec![1]))],
        )
        .unwrap();

        assert!(split_by_shard(&numbers, &partition).unwrap().is_none());
    }
}
