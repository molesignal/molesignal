// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The storage order of a clustered log file (see [`Clustering`]): the time slice of each row,
//! newest first, then the clustering fields, then the usual storage order. Every key is
//! descending with nulls last, as in the rest of the storage order.
//!
//! A flush sorts its rows with [`sort_for_storage_clustered`]. Compaction merges clustered files by
//! [`clustered_ordering`], whose first key computes the slice from `_timestamp` the way
//! [`Clustering::slice_of`] does, so a merge never finds a flushed file out of order.

use std::sync::Arc;

use arrow::{
    array::{Int64Array, RecordBatch, TimestampMicrosecondArray},
    compute::{SortColumn, lexsort_to_indices, take_record_batch},
    datatypes::{DataType, Schema},
};
use datafusion::{
    logical_expr::Operator,
    physical_expr::{
        LexOrdering, PhysicalExpr, PhysicalSortExpr,
        expressions::{BinaryExpr, CastExpr, Literal, col},
    },
    scalar::ScalarValue,
};
use domain::{
    rum::event::Versioned,
    storage::{DatasetTypeId, type_id::builtin},
    stream::Clustering,
};
use kernel::{Error, Result};

use super::partition::{
    STORAGE_SORT, sort_for_storage, storage_ordering, storage_sort_column_names,
};
use crate::infra::storage::arrow_schema::TS_COL;

/// The clustering rows of `dataset_type` with `schema` are written in: a stream's clustering
/// applies to its log records only, and not to rows that report one entity again, which must stay
/// next to each other for compaction to keep one of them.
pub(crate) fn applied<'a>(
    schema: &Schema,
    dataset_type: &DatasetTypeId,
    clustering: Option<&'a Clustering>,
) -> Option<&'a Clustering> {
    clustering.filter(|clustering| {
        !clustering.is_empty()
            && dataset_type.as_str() == builtin::DATASET_LOG_RECORDS
            && Versioned::declared_by(|name| schema.index_of(name).is_ok()).is_none()
    })
}

/// Sorts `batch` into the storage order of `dataset_type`, clustered by `clustering` where it
/// applies (see [`applied`]).
pub fn sort_for_storage_clustered(
    batch: RecordBatch,
    dataset_type: &DatasetTypeId,
    clustering: Option<&Clustering>,
) -> Result<RecordBatch> {
    let schema = batch.schema();

    let Some(clustering) = applied(schema.as_ref(), dataset_type, clustering) else {
        return sort_for_storage(batch, dataset_type);
    };

    if batch.num_rows() <= 1 {
        return Ok(batch);
    }

    let timestamps = batch
        .column_by_name(TS_COL)
        .and_then(|column| column.as_any().downcast_ref::<TimestampMicrosecondArray>())
        .ok_or_else(|| Error::internal(format!("batch missing column {TS_COL}")))?;

    let slices: Int64Array = timestamps
        .iter()
        .map(|time| time.map(|time| clustering.slice_of(time)))
        .collect();

    let mut sort_columns = vec![SortColumn {
        values: Arc::new(slices),
        options: Some(STORAGE_SORT),
    }];

    sort_columns.extend(
        key_names(schema.as_ref(), Some(dataset_type), clustering)
            .into_iter()
            .filter_map(|name| batch.column_by_name(name))
            .map(|values| SortColumn {
                values: values.clone(),
                options: Some(STORAGE_SORT),
            }),
    );

    let indices = lexsort_to_indices(&sort_columns, None)
        .map_err(|error| Error::internal(format!("clustered lexsort: {error}")))?;

    take_record_batch(&batch, &indices)
        .map_err(|error| Error::internal(format!("clustered sort take: {error}")))
}

/// The storage order of files clustered by `clustering`, as sort expressions on `schema`; without
/// a clustering, the time order of [`storage_ordering`]. `clustering` is the one the files
/// recorded, and a file records one only where it applied when the file was written.
pub(crate) fn clustered_ordering(
    schema: &Schema,
    dataset_type: Option<&DatasetTypeId>,
    clustering: Option<&Clustering>,
) -> Option<LexOrdering> {
    let Some(clustering) = clustering.filter(|clustering| !clustering.is_empty()) else {
        return storage_ordering(schema, dataset_type);
    };

    let slice: Arc<dyn PhysicalExpr> = Arc::new(BinaryExpr::new(
        Arc::new(CastExpr::new(
            col(TS_COL, schema).ok()?,
            DataType::Int64,
            None,
        )),
        Operator::Divide,
        Arc::new(Literal::new(ScalarValue::Int64(Some(clustering.interval)))),
    ));

    let keys = key_names(schema, dataset_type, clustering)
        .into_iter()
        .filter_map(|name| col(name, schema).ok());

    LexOrdering::new(
        std::iter::once(slice)
            .chain(keys)
            .map(|expression| PhysicalSortExpr::new(expression, STORAGE_SORT)),
    )
}

/// The columns after the slice: the clustering fields, then the usual storage order.
fn key_names<'a>(
    schema: &Schema,
    dataset_type: Option<&DatasetTypeId>,
    clustering: &'a Clustering,
) -> Vec<&'a str> {
    let mut names: Vec<&str> = clustering.fields.iter().map(String::as_str).collect();
    names.extend(storage_sort_column_names(schema, dataset_type));
    names
}

#[cfg(test)]
mod tests {
    use arrow::{
        array::{ArrayRef, AsArray, StringArray},
        datatypes::{Field, Int64Type, TimeUnit, TimestampMicrosecondType},
    };

    use super::*;

    const MINUTE: i64 = 60_000_000;

    fn schema() -> Arc<Schema> {
        Arc::new(Schema::new(vec![
            Field::new(
                TS_COL,
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
            Field::new("service", DataType::Utf8, true),
            Field::new("event_id", DataType::Utf8, true),
        ]))
    }

    fn batch(rows: &[(i64, &str)]) -> RecordBatch {
        let times: Vec<i64> = rows.iter().map(|(time, _)| *time).collect();
        let services: Vec<&str> = rows.iter().map(|(_, service)| *service).collect();
        let ids: Vec<String> = (0..rows.len()).map(|row| format!("e{row:03}")).collect();

        RecordBatch::try_new(
            schema(),
            vec![
                Arc::new(TimestampMicrosecondArray::from(times).with_timezone("UTC")),
                Arc::new(StringArray::from(services)),
                Arc::new(StringArray::from(ids)),
            ],
        )
        .unwrap()
    }

    fn by_service() -> Clustering {
        Clustering::new(vec!["service".into()], MINUTE)
    }

    fn rows(batch: &RecordBatch) -> Vec<(i64, String)> {
        let times = batch.column(0).as_primitive::<TimestampMicrosecondType>();
        let services = batch.column(1).as_string::<i32>();

        (0..batch.num_rows())
            .map(|row| (times.value(row), services.value(row).to_string()))
            .collect()
    }

    fn logs() -> DatasetTypeId {
        DatasetTypeId::builtin(builtin::DATASET_LOG_RECORDS)
    }

    #[test]
    fn rows_are_ordered_by_slice_then_by_the_fields_then_newest_first() {
        let unsorted = batch(&[
            (MINUTE + 5, "a"),
            (2 * MINUTE + 1, "a"),
            (MINUTE + 9, "b"),
            (MINUTE + 7, "a"),
            (2 * MINUTE + 3, "b"),
        ]);

        let sorted = sort_for_storage_clustered(unsorted, &logs(), Some(&by_service())).unwrap();

        assert_eq!(
            rows(&sorted),
            vec![
                (2 * MINUTE + 3, "b".to_string()),
                (2 * MINUTE + 1, "a".to_string()),
                (MINUTE + 9, "b".to_string()),
                (MINUTE + 7, "a".to_string()),
                (MINUTE + 5, "a".to_string()),
            ]
        );
    }

    #[test]
    fn other_datasets_and_streams_without_clustering_keep_the_time_order() {
        let rows_in = [(MINUTE + 5, "a"), (2 * MINUTE + 1, "a"), (MINUTE + 9, "b")];
        let metrics = DatasetTypeId::builtin(builtin::DATASET_METRIC_SAMPLES);

        for (dataset_type, clustering) in [
            (logs(), None),
            (logs(), Some(Clustering::default())),
            (metrics.clone(), Some(by_service())),
        ] {
            let clustered =
                sort_for_storage_clustered(batch(&rows_in), &dataset_type, clustering.as_ref())
                    .unwrap();
            let plain = sort_for_storage(batch(&rows_in), &dataset_type).unwrap();

            assert_eq!(rows(&clustered), rows(&plain));
        }
    }

    #[test]
    fn the_merge_ordering_computes_the_slice_the_flush_sorted_by() {
        let sorted = sort_for_storage_clustered(
            batch(&[
                (3 * MINUTE + 1, "a"),
                (MINUTE + 5, "c"),
                (3 * MINUTE + 8, "b"),
                (MINUTE + 2, "a"),
            ]),
            &logs(),
            Some(&by_service()),
        )
        .unwrap();

        let ordering =
            clustered_ordering(schema().as_ref(), Some(&logs()), Some(&by_service())).unwrap();

        let slices = ordering[0]
            .expr
            .evaluate(&sorted)
            .unwrap()
            .into_array(sorted.num_rows())
            .unwrap();

        let expected: Vec<i64> = rows(&sorted)
            .iter()
            .map(|(time, _)| by_service().slice_of(*time))
            .collect();

        assert_eq!(
            slices.as_primitive::<Int64Type>().values().to_vec(),
            expected
        );

        // Sorting the sorted rows again by the merge's ordering leaves them where they are.
        let columns: Vec<SortColumn> = ordering
            .iter()
            .map(|sort| SortColumn {
                values: sort
                    .expr
                    .evaluate(&sorted)
                    .unwrap()
                    .into_array(sorted.num_rows())
                    .unwrap() as ArrayRef,
                options: Some(sort.options),
            })
            .collect();

        let again =
            take_record_batch(&sorted, &lexsort_to_indices(&columns, None).unwrap()).unwrap();

        assert_eq!(rows(&again), rows(&sorted));
    }

    #[test]
    fn without_a_clustering_the_merge_keeps_the_time_order() {
        let plain = storage_ordering(schema().as_ref(), Some(&logs()));

        assert_eq!(
            clustered_ordering(schema().as_ref(), Some(&logs()), None),
            plain
        );
        assert_eq!(
            clustered_ordering(
                schema().as_ref(),
                Some(&logs()),
                Some(&Clustering::default())
            ),
            plain
        );
    }

    #[test]
    fn a_clustering_applies_to_log_records_only() {
        let spans = DatasetTypeId::builtin(builtin::DATASET_TRACE_SPANS);

        assert_eq!(
            applied(schema().as_ref(), &logs(), Some(&by_service())),
            Some(&by_service())
        );
        assert_eq!(
            applied(schema().as_ref(), &spans, Some(&by_service())),
            None
        );
        assert_eq!(
            applied(schema().as_ref(), &logs(), Some(&Clustering::default())),
            None
        );
    }
}
