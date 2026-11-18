// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The rows of a compaction group as one stream in the storage order.
//!
//! Every input file is already in the storage order, so the group is merged the way a
//! sort-preserving merge combines sorted partitions: one batch of each input is held at a time,
//! however large the inputs are. Each input is checked to really be in order; merging a file
//! that is not would write a file out of the order queries rely on.

use std::sync::Arc;

use arrow::{
    array::{ArrayRef, RecordBatch},
    datatypes::SchemaRef,
    row::{OwnedRow, RowConverter, SortField},
};
use datafusion::{
    error::DataFusionError,
    execution::memory_pool::{MemoryConsumer, MemoryPool, UnboundedMemoryPool},
    physical_expr::{LexOrdering, PhysicalSortExpr},
    physical_plan::{
        SendableRecordBatchStream,
        metrics::{BaselineMetrics, ExecutionPlanMetricsSet},
        sorts::streaming_merge::StreamingMergeBuilder,
        stream::RecordBatchStreamAdapter,
    },
};
use futures::{StreamExt, stream::BoxStream};
use kernel::{Error, Result};

use crate::infra::storage::arrow_schema::align_batch_to_schema;

/// Rows per merged batch.
const MERGED_BATCH_ROWS: usize = 8192;

/// Merges `inputs`, each in `ordering`, into one stream in `ordering`. Every input batch is
/// aligned to `schema` first, so files written under older schemas merge with newer ones.
///
/// With `one_row_per` (the columns that name an entity, see [`dedup::entity_columns`]) only the
/// first row of each run of rows that agree on them is kept.
pub(super) fn merge(
    inputs: Vec<BoxStream<'static, Result<RecordBatch>>>,
    schema: SchemaRef,
    ordering: &LexOrdering,
    one_row_per: Option<Vec<usize>>,
) -> Result<BoxStream<'static, Result<RecordBatch>>> {
    let keys: Vec<PhysicalSortExpr> = ordering.iter().cloned().collect();

    let streams = inputs
        .into_iter()
        .map(|input| {
            let mut order = OrderCheck::new(&schema, &keys)?;
            let target = schema.clone();

            let checked = input.map(move |batch| {
                let batch = align_batch_to_schema(&batch?, &target)
                    .map_err(|error| Error::internal(format!("compactor align schema: {error}")))?;

                order.check(&batch)?;
                Ok::<_, Error>(batch)
            });

            let checked = checked
                .map(|batch| batch.map_err(|error| DataFusionError::External(Box::new(error))));

            Ok(
                Box::pin(RecordBatchStreamAdapter::new(schema.clone(), checked))
                    as SendableRecordBatchStream,
            )
        })
        .collect::<Result<Vec<_>>>()?;

    let pool: Arc<dyn MemoryPool> = Arc::new(UnboundedMemoryPool::default());

    let merged = StreamingMergeBuilder::new()
        .with_streams(streams)
        .with_schema(schema.clone())
        .with_expressions(ordering)
        .with_metrics(BaselineMetrics::new(&ExecutionPlanMetricsSet::new(), 0))
        .with_batch_size(MERGED_BATCH_ROWS)
        .with_reservation(MemoryConsumer::new("compactor merge").register(&pool))
        .build()
        .map_err(|error| Error::internal(format!("compactor merge: {error}")))?;

    let merged = merged
        .map(|batch| batch.map_err(|error| Error::internal(format!("compactor merge: {error}"))))
        .boxed();

    match one_row_per {
        Some(columns) => super::dedup::keep_first(merged, &schema, columns),
        None => Ok(merged),
    }
}

/// Confirms that the rows of one input never go backwards in the storage order. The order's keys
/// are columns, and for a clustered file also the time slice computed from `_timestamp`.
struct OrderCheck {
    converter: RowConverter,
    keys: Vec<PhysicalSortExpr>,
    last: Option<OwnedRow>,
}

impl OrderCheck {
    fn new(schema: &SchemaRef, keys: &[PhysicalSortExpr]) -> Result<Self> {
        let fields =
            keys.iter()
                .map(|key| {
                    let data_type = key.expr.data_type(schema.as_ref()).map_err(|error| {
                        Error::internal(format!("compactor order check: {error}"))
                    })?;

                    Ok(SortField::new_with_options(data_type, key.options))
                })
                .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            converter: RowConverter::new(fields)
                .map_err(|error| Error::internal(format!("compactor order check: {error}")))?,
            keys: keys.to_vec(),
            last: None,
        })
    }

    fn check(&mut self, batch: &RecordBatch) -> Result<()> {
        if batch.num_rows() == 0 {
            return Ok(());
        }

        let arrays = self
            .keys
            .iter()
            .map(|key| {
                key.expr
                    .evaluate(batch)
                    .and_then(|value| value.into_array(batch.num_rows()))
            })
            .collect::<datafusion::error::Result<Vec<ArrayRef>>>()
            .map_err(|error| Error::internal(format!("compactor order check: {error}")))?;

        let rows = self
            .converter
            .convert_columns(&arrays)
            .map_err(|error| Error::internal(format!("compactor order check: {error}")))?;

        let mut previous = self.last.as_ref().map(OwnedRow::row);

        for row in rows.iter() {
            if previous.is_some_and(|previous| previous > row) {
                return Err(Error::internal(
                    "a compaction input is not in the storage order",
                ));
            }

            previous = Some(row);
        }

        self.last = previous.map(|row| row.owned());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use arrow::{
        array::{Int64Array, StringArray, TimestampMicrosecondArray},
        datatypes::{DataType, Field, Schema, TimeUnit},
    };
    use datafusion::physical_expr::{PhysicalSortExpr, expressions::col};
    use domain::{
        storage::{DatasetTypeId, type_id::builtin},
        stream::Clustering,
    };
    use futures::stream;

    use super::*;
    use crate::infra::storage::parquet::{
        clustering::{clustered_ordering, sort_for_storage_clustered},
        partition::STORAGE_SORT,
    };

    const MINUTE: i64 = 60_000_000;

    fn schema() -> SchemaRef {
        Arc::new(Schema::new(vec![
            Field::new(
                "_timestamp",
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
            Field::new("value", DataType::Int64, true),
        ]))
    }

    fn batch(times: &[i64]) -> RecordBatch {
        RecordBatch::try_new(
            schema(),
            vec![
                Arc::new(TimestampMicrosecondArray::from(times.to_vec()).with_timezone("UTC")),
                Arc::new(Int64Array::from(times.to_vec())),
            ],
        )
        .unwrap()
    }

    fn ordering() -> LexOrdering {
        LexOrdering::new(vec![PhysicalSortExpr::new(
            col("_timestamp", &schema()).unwrap(),
            STORAGE_SORT,
        )])
        .unwrap()
    }

    fn input(batches: Vec<RecordBatch>) -> BoxStream<'static, Result<RecordBatch>> {
        stream::iter(batches.into_iter().map(Ok)).boxed()
    }

    async fn collect(stream: BoxStream<'static, Result<RecordBatch>>) -> Result<Vec<i64>> {
        let batches = stream.collect::<Vec<_>>().await;
        let mut times = Vec::new();

        for batch in batches {
            let batch = batch?;
            let values = batch
                .column(1)
                .as_any()
                .downcast_ref::<Int64Array>()
                .unwrap();
            times.extend(values.iter().flatten());
        }

        Ok(times)
    }

    #[tokio::test]
    async fn merges_sorted_inputs_into_one_descending_stream() {
        let merged = merge(
            vec![
                input(vec![batch(&[90, 70]), batch(&[50, 10])]),
                input(vec![batch(&[80, 60, 20])]),
                input(vec![batch(&[100]), batch(&[30])]),
            ],
            schema(),
            &ordering(),
            None,
        )
        .unwrap();

        assert_eq!(
            collect(merged).await.unwrap(),
            vec![100, 90, 80, 70, 60, 50, 30, 20, 10]
        );
    }

    #[tokio::test]
    async fn refuses_an_input_that_is_out_of_order() {
        for unordered in [vec![batch(&[10, 20])], vec![batch(&[50, 40]), batch(&[45])]] {
            let merged = merge(
                vec![input(unordered), input(vec![batch(&[30])])],
                schema(),
                &ordering(),
                None,
            )
            .unwrap();

            assert!(collect(merged).await.is_err());
        }
    }

    fn entity_schema() -> SchemaRef {
        Arc::new(Schema::new(vec![
            Field::new(
                "_timestamp",
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
            Field::new("view.id", DataType::Utf8, true),
            Field::new("view.revision", DataType::Int64, true),
        ]))
    }

    fn entity_batch(rows: &[(i64, &str, i64)]) -> RecordBatch {
        RecordBatch::try_new(
            entity_schema(),
            vec![
                Arc::new(
                    TimestampMicrosecondArray::from(
                        rows.iter().map(|row| row.0).collect::<Vec<_>>(),
                    )
                    .with_timezone("UTC"),
                ),
                Arc::new(StringArray::from(
                    rows.iter().map(|row| row.1).collect::<Vec<_>>(),
                )),
                Arc::new(Int64Array::from(
                    rows.iter().map(|row| row.2).collect::<Vec<_>>(),
                )),
            ],
        )
        .unwrap()
    }

    #[tokio::test]
    async fn merging_keeps_the_highest_revision_of_an_entity_across_inputs() {
        let schema = entity_schema();

        let ordering = LexOrdering::new(
            ["_timestamp", "view.id", "view.revision"]
                .into_iter()
                .map(|name| PhysicalSortExpr::new(col(name, &schema).unwrap(), STORAGE_SORT)),
        )
        .unwrap();

        let merged = merge(
            vec![
                input(vec![entity_batch(&[(10, "b", 1), (10, "a", 2)])]),
                input(vec![entity_batch(&[(10, "b", 3), (10, "a", 1)])]),
            ],
            schema,
            &ordering,
            Some(vec![0, 1]),
        )
        .unwrap();

        let mut rows = Vec::new();

        for batch in merged.collect::<Vec<_>>().await {
            let batch = batch.unwrap();
            let keys = batch
                .column(1)
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let revisions = batch
                .column(2)
                .as_any()
                .downcast_ref::<Int64Array>()
                .unwrap();

            for row in 0..batch.num_rows() {
                rows.push((keys.value(row).to_string(), revisions.value(row)));
            }
        }

        assert_eq!(rows, vec![("b".to_string(), 3), ("a".to_string(), 2)]);
    }

    fn log_schema() -> SchemaRef {
        Arc::new(Schema::new(vec![
            Field::new(
                "_timestamp",
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
            Field::new("service", DataType::Utf8, true),
        ]))
    }

    fn log_batch(rows: &[(i64, &str)]) -> RecordBatch {
        RecordBatch::try_new(
            log_schema(),
            vec![
                Arc::new(
                    TimestampMicrosecondArray::from(
                        rows.iter().map(|row| row.0).collect::<Vec<_>>(),
                    )
                    .with_timezone("UTC"),
                ),
                Arc::new(StringArray::from(
                    rows.iter().map(|row| row.1).collect::<Vec<_>>(),
                )),
            ],
        )
        .unwrap()
    }

    async fn log_rows(
        stream: BoxStream<'static, Result<RecordBatch>>,
    ) -> Result<Vec<(i64, String)>> {
        let mut rows = Vec::new();

        for batch in stream.collect::<Vec<_>>().await {
            let batch = batch?;
            let times = batch
                .column(0)
                .as_any()
                .downcast_ref::<TimestampMicrosecondArray>()
                .unwrap();
            let services = batch
                .column(1)
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();

            for row in 0..batch.num_rows() {
                rows.push((times.value(row), services.value(row).to_string()));
            }
        }

        Ok(rows)
    }

    fn by_service() -> (Clustering, LexOrdering, DatasetTypeId) {
        let clustering = Clustering::new(vec!["service".into()], MINUTE);
        let logs = DatasetTypeId::builtin(builtin::DATASET_LOG_RECORDS);
        let ordering =
            clustered_ordering(log_schema().as_ref(), Some(&logs), Some(&clustering)).unwrap();

        (clustering, ordering, logs)
    }

    #[tokio::test]
    async fn merges_clustered_inputs_by_slice_then_field_then_time() {
        let (clustering, ordering, logs) = by_service();

        let sorted = |rows: &[(i64, &str)]| {
            sort_for_storage_clustered(log_batch(rows), &logs, Some(&clustering)).unwrap()
        };

        let merged = merge(
            vec![
                input(vec![sorted(&[
                    (MINUTE + 5, "a"),
                    (2 * MINUTE + 1, "b"),
                    (MINUTE + 9, "b"),
                ])]),
                input(vec![sorted(&[(2 * MINUTE + 7, "a"), (MINUTE + 1, "a")])]),
            ],
            log_schema(),
            &ordering,
            None,
        )
        .unwrap();

        let expected = [
            (2 * MINUTE + 1, "b"),
            (2 * MINUTE + 7, "a"),
            (MINUTE + 9, "b"),
            (MINUTE + 5, "a"),
            (MINUTE + 1, "a"),
        ];

        assert_eq!(
            log_rows(merged).await.unwrap(),
            expected
                .iter()
                .map(|(time, service)| (*time, service.to_string()))
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn refuses_a_file_in_time_order_where_the_group_is_clustered() {
        let (_, ordering, _) = by_service();

        let merged = merge(
            vec![input(vec![log_batch(&[
                (2 * MINUTE + 7, "a"),
                (2 * MINUTE + 1, "b"),
            ])])],
            log_schema(),
            &ordering,
            None,
        )
        .unwrap();

        assert!(log_rows(merged).await.is_err());
    }
}
