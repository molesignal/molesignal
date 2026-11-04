// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! One row of each entity that is reported again and again.
//!
//! A RUM view is written anew whenever it changes, and so is a session: the rows share the time and
//! the key of the entity and differ in their revision. The storage order puts them next to each
//! other with the highest revision first, so when files are merged the first row of each run of
//! equal `(time, key)` is the one that counts and the rest can go.

use std::future::ready;

use arrow::{
    array::{Array, ArrayRef, BooleanArray, RecordBatch},
    compute::filter_record_batch,
    datatypes::SchemaRef,
    row::{OwnedRow, RowConverter, SortField},
};
use futures::{StreamExt, stream::BoxStream};

use crate::{
    domain::rum::event::Versioned,
    infra::storage::arrow_schema::TS_COL,
    shared::{Error, Result},
};

/// The columns that tell the rows of one entity from those of another (the time and the key),
/// when the dataset holds rows that are reported again; `None` for every other dataset.
pub(super) fn entity_columns(schema: &SchemaRef) -> Option<Vec<usize>> {
    let versioned = Versioned::declared_by(|name| schema.index_of(name).is_ok())?;

    Some(vec![
        schema.index_of(TS_COL).ok()?,
        schema.index_of(versioned.key).ok()?,
    ])
}

/// Keeps the first row of each run of rows that agree on `columns` (the time and the key). A row
/// whose key is null is nobody's report again and always stays.
pub(super) fn keep_first(
    input: BoxStream<'static, Result<RecordBatch>>,
    schema: &SchemaRef,
    columns: Vec<usize>,
) -> Result<BoxStream<'static, Result<RecordBatch>>> {
    let fields = columns
        .iter()
        .map(|index| SortField::new(schema.field(*index).data_type().clone()))
        .collect();

    let mut runs = Runs {
        converter: RowConverter::new(fields)
            .map_err(|error| Error::internal(format!("compactor dedup: {error}")))?,
        columns,
        last: None,
    };

    Ok(input
        .map(move |batch| runs.thin(batch?))
        .filter_map(|batch| {
            ready(match batch {
                Ok(batch) if batch.num_rows() == 0 => None,
                other => Some(other),
            })
        })
        .boxed())
}

struct Runs {
    converter: RowConverter,
    columns: Vec<usize>,
    last: Option<OwnedRow>,
}

impl Runs {
    fn thin(&mut self, batch: RecordBatch) -> Result<RecordBatch> {
        if batch.num_rows() == 0 {
            return Ok(batch);
        }

        let arrays: Vec<ArrayRef> = self
            .columns
            .iter()
            .map(|index| batch.column(*index).clone())
            .collect();

        let rows = self
            .converter
            .convert_columns(&arrays)
            .map_err(|error| Error::internal(format!("compactor dedup: {error}")))?;

        // The key is the last of the columns; the time before it is never null.
        let key = arrays.last();

        let keep = (0..batch.num_rows())
            .map(|index| {
                let row = rows.row(index);

                if key.is_some_and(|key| key.is_null(index)) {
                    self.last = None;
                    return true;
                }

                let repeated = self.last.as_ref().is_some_and(|last| last.row() == row);

                if !repeated {
                    self.last = Some(row.owned());
                }

                !repeated
            })
            .collect::<Vec<bool>>();

        let keep = BooleanArray::from(keep);

        if keep.true_count() == batch.num_rows() {
            return Ok(batch);
        }

        filter_record_batch(&batch, &keep)
            .map_err(|error| Error::internal(format!("compactor dedup: {error}")))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::{
        array::{Int64Array, StringArray, TimestampMicrosecondArray},
        datatypes::{DataType, Field, Schema, TimeUnit},
    };
    use futures::stream;

    use super::*;

    fn schema() -> SchemaRef {
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

    /// Rows are `(time, key, revision)`.
    fn batch(rows: &[(i64, Option<&str>, i64)]) -> RecordBatch {
        RecordBatch::try_new(
            schema(),
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

    async fn thinned(batches: Vec<RecordBatch>) -> Vec<(i64, Option<String>, i64)> {
        let input = stream::iter(batches.into_iter().map(Ok)).boxed();
        let output = keep_first(input, &schema(), vec![0, 1]).unwrap();
        let mut rows = Vec::new();

        for batch in output.collect::<Vec<_>>().await {
            let batch = batch.unwrap();
            let times = batch
                .column(0)
                .as_any()
                .downcast_ref::<TimestampMicrosecondArray>()
                .unwrap();
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

            for index in 0..batch.num_rows() {
                rows.push((
                    times.value(index),
                    keys.is_valid(index).then(|| keys.value(index).to_string()),
                    revisions.value(index),
                ));
            }
        }

        rows
    }

    #[test]
    fn a_dataset_declares_its_entities_by_its_columns() {
        assert_eq!(entity_columns(&schema()), Some(vec![0, 1]));

        let plain = Arc::new(Schema::new(vec![
            Field::new(
                "_timestamp",
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
            Field::new("view.id", DataType::Utf8, true),
        ]));

        assert_eq!(entity_columns(&plain), None);
    }

    #[tokio::test]
    async fn only_the_first_row_of_each_entity_stays() {
        let rows = thinned(vec![batch(&[
            (20, Some("b"), 1),
            (10, Some("c"), 4),
            (10, Some("c"), 3),
            (10, Some("c"), 1),
            (10, Some("a"), 2),
            (10, Some("a"), 1),
        ])])
        .await;

        assert_eq!(
            rows,
            vec![
                (20, Some("b".into()), 1),
                (10, Some("c".into()), 4),
                (10, Some("a".into()), 2),
            ]
        );
    }

    #[tokio::test]
    async fn a_run_that_crosses_a_batch_boundary_is_still_one_run() {
        let rows = thinned(vec![
            batch(&[(10, Some("a"), 3), (10, Some("a"), 2)]),
            batch(&[(10, Some("a"), 1), (10, Some("b"), 1)]),
            batch(&[(10, Some("b"), 0)]),
        ])
        .await;

        assert_eq!(
            rows,
            vec![(10, Some("a".into()), 3), (10, Some("b".into()), 1)]
        );
    }

    #[tokio::test]
    async fn the_same_key_at_another_time_is_another_entity() {
        let rows = thinned(vec![batch(&[(20, Some("a"), 1), (10, Some("a"), 1)])]).await;

        assert_eq!(rows.len(), 2);
    }

    #[tokio::test]
    async fn rows_without_a_key_are_never_merged() {
        let rows = thinned(vec![batch(&[
            (10, None, 1),
            (10, None, 1),
            (10, Some("a"), 2),
            (10, Some("a"), 1),
        ])])
        .await;

        assert_eq!(rows.len(), 3);
    }

    #[tokio::test]
    async fn a_batch_of_only_repeats_is_not_passed_on_empty() {
        let input = stream::iter(
            vec![
                batch(&[(10, Some("a"), 3)]),
                batch(&[(10, Some("a"), 2), (10, Some("a"), 1)]),
            ]
            .into_iter()
            .map(Ok),
        )
        .boxed();

        let output = keep_first(input, &schema(), vec![0, 1]).unwrap();
        let batches = output.collect::<Vec<_>>().await;

        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].as_ref().unwrap().num_rows(), 1);
    }
}
