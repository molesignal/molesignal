// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! How many trace summaries a query matches, bar by bar of the explorer
//! histogram.
//!
//! The list stops reading once it has a page. A count reads every summary in
//! the range, so it reads only the columns the query looks at, several files
//! at a time, and keeps nothing but the bars.
//!
//! A summary is one row for a trace. A summary that was written twice, when a
//! write was retried, is two rows and counts twice: telling them apart would
//! mean holding every trace id of the range in memory.

use std::sync::Arc;

use arrow::array::{Array, Int64Array, RecordBatch};
use futures::StreamExt;
use object_store::ObjectStore;

use super::{
    TEXT_COLUMNS, TraceListContext, files::file_may_match_filters, integer_at,
    matches_text_and_filters,
};
use crate::{
    api::{
        AppState,
        http::routes::web::histogram::{BarCount, BucketPlan, within_scan_limit},
    },
    domain::{
        storage::{DatasetTypeId, QueryFile, type_id::builtin},
        stream::StreamType,
    },
    infra::storage::{
        org_scoped_store::scoped_to_org,
        parquet::reader::{ParquetReader, ReadOptions},
    },
    shared::{
        Error, Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
        trace::summary::{TRACE_SUMMARY_ERROR_COUNT_FIELD, TRACE_SUMMARY_START_NS_FIELD},
    },
};

/// What every count reads: when a trace began, and how many of its spans failed.
const COUNTED_COLUMNS: [&str; 2] = [
    TRACE_SUMMARY_START_NS_FIELD,
    TRACE_SUMMARY_ERROR_COUNT_FIELD,
];
/// Files read at once. A range can hold hundreds of small files, and most of
/// the time one takes is spent waiting for the object store.
const FILE_CONCURRENCY: usize = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Tally {
    total: u64,
    errors: u64,
}

/// A column of whole numbers, read row by row. Summaries store them as
/// `Int64`; any other stored type is read the way the list reads it.
enum WholeNumbers<'a> {
    Int64(&'a Int64Array),
    Other(&'a RecordBatch, &'static str),
}

impl<'a> WholeNumbers<'a> {
    fn of(batch: &'a RecordBatch, name: &'static str) -> Self {
        batch
            .column_by_name(name)
            .and_then(|column| column.as_any().downcast_ref::<Int64Array>())
            .map_or(Self::Other(batch, name), Self::Int64)
    }

    fn at(&self, row: usize) -> Option<i64> {
        match self {
            Self::Int64(values) => values.is_valid(row).then(|| values.value(row)),
            Self::Other(batch, name) => integer_at(batch, name, row),
        }
    }
}

/// The bars of `plan` that hold a trace the query matches. `None` when the
/// range holds more summaries than one query may read.
pub(in super::super) async fn bars(
    state: &AppState,
    org_id: &Id,
    stream: &str,
    context: &TraceListContext,
    plan: &BucketPlan,
) -> Result<Option<Vec<BarCount>>> {
    let range = TimeRange::new(TimestampMicros(context.from), TimestampMicros(context.to));
    let summary_type = DatasetTypeId::builtin(builtin::DATASET_TRACE_SUMMARY);

    let snapshot = state
        .storage
        .catalog_query
        .snapshot_by_name(
            org_id,
            stream,
            StreamType::TRACES,
            std::slice::from_ref(&summary_type),
            range,
        )
        .await?;

    let Some(dataset) = snapshot.dataset(&summary_type) else {
        return Ok(Some(Vec::new()));
    };

    let files = dataset
        .files
        .iter()
        .filter(|file| file_may_match_filters(file, context))
        .cloned()
        .collect::<Vec<_>>();

    let candidate_rows = files
        .iter()
        .map(|file| file.rows)
        .chain(
            dataset
                .buffered_batches
                .iter()
                .map(|batch| batch.num_rows() as u64),
        )
        .fold(0_u64, u64::saturating_add);

    if !within_scan_limit(candidate_rows) {
        return Ok(None);
    }

    let mut tallies = vec![Tally::default(); plan.bars()];

    for batch in &dataset.buffered_batches {
        tally_batch(batch, context, plan, &mut tallies);
    }

    let store = scoped_to_org(state.storage.read_store.clone(), org_id);
    let reader = ParquetReader::new(store.clone());
    let columns = read_columns(context);
    let scanned_files = files.len();
    let (reader, store, columns) = (&reader, &store, columns.as_slice());

    // Each read owns its file: a stream of borrowed files does not make a
    // future that can move between threads.
    let mut counted = futures::stream::iter(files)
        .map(|file| async move { tally_file(reader, store, &file, context, plan, columns).await })
        .buffer_unordered(FILE_CONCURRENCY);

    while let Some(file_tallies) = counted.next().await {
        for (tally, counted) in tallies.iter_mut().zip(file_tallies?) {
            tally.total += counted.total;
            tally.errors += counted.errors;
        }
    }

    tracing::debug!(
        scanned_files,
        candidate_rows,
        "trace summary histogram scan completed"
    );

    Ok(Some(
        tallies
            .into_iter()
            .enumerate()
            .map(|(index, tally)| BarCount {
                index: index as i64,
                total: tally.total,
                errors: tally.errors,
            })
            .collect(),
    ))
}

/// The columns a count has to decode: the two it counts by, and whatever the
/// query looks at.
///
/// The trace id is not among them unless the query looks at it. A summary is
/// written from a span of its trace, so it always has one, and the id is by
/// far the widest column of the dataset.
fn read_columns(context: &TraceListContext) -> Vec<&'static str> {
    let mut columns = COUNTED_COLUMNS.to_vec();

    if context.q.is_some() {
        columns.extend(TEXT_COLUMNS);
    }

    columns.extend(
        context
            .filters
            .iter()
            .filter_map(|filter| filter.summary_column()),
    );

    columns.sort_unstable();
    columns.dedup();
    columns
}

async fn tally_file(
    reader: &ParquetReader,
    store: &Arc<dyn ObjectStore>,
    file: &QueryFile,
    context: &TraceListContext,
    plan: &BucketPlan,
    columns: &[&str],
) -> Result<Vec<Tally>> {
    let mut tallies = vec![Tally::default(); plan.bars()];

    let options = ReadOptions::new()
        .with_time_range(context.from, context.to)
        .with_columns(columns)
        .with_known_size(file.size_bytes);

    let mut batches = match reader
        .stream_from_store(store.clone(), &file.object_key, options)
        .await
    {
        Ok(batches) => batches,
        Err(Error::NotFound(_)) => {
            tracing::warn!(object_key = %file.object_key, "trace summary parquet is missing");
            return Ok(tallies);
        }
        Err(error) => return Err(error),
    };

    while let Some(batch) = batches.next().await {
        tally_batch(&batch?, context, plan, &mut tallies);
    }

    Ok(tallies)
}

/// Counts the summaries of a batch that the query matches into their bars,
/// with the conditions the list matches a summary by.
fn tally_batch(
    batch: &RecordBatch,
    context: &TraceListContext,
    plan: &BucketPlan,
    tallies: &mut [Tally],
) {
    let selective = context.q.is_some() || !context.filters.is_empty();
    let starts = WholeNumbers::of(batch, TRACE_SUMMARY_START_NS_FIELD);
    let errors = WholeNumbers::of(batch, TRACE_SUMMARY_ERROR_COUNT_FIELD);

    for row in 0..batch.num_rows() {
        let start_ns = starts.at(row).unwrap_or(0);

        // A trace begins on a nanosecond and the range is in microseconds. A
        // start inside the range in the one unit is inside it in the other.
        let Some(index) = plan.index_of(start_ns.div_euclid(1_000)) else {
            continue;
        };

        if selective && !matches_text_and_filters(batch, row, context) {
            continue;
        }

        let Some(tally) = usize::try_from(index)
            .ok()
            .and_then(|index| tallies.get_mut(index))
        else {
            continue;
        };

        tally.total += 1;

        if errors.at(row).unwrap_or(0) > 0 {
            tally.errors += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use arrow::{
        array::{StringArray, UInt64Array},
        datatypes::{DataType, Field, Schema},
    };

    use super::*;
    use crate::{
        api::http::routes::web::trace::list::{TraceListSort, filter},
        shared::trace::{
            normalization::canonical_trace_schema, summary::TRACE_SUMMARY_SPAN_COUNT_FIELD,
        },
    };

    /// Bars of a millisecond from 10 ms, holding what began in `[10.5 ms, 13 ms)`.
    fn plan() -> BucketPlan {
        BucketPlan::new(Some(10_500), Some(13_000), Some(1_000), Some(10_000)).expect("plan")
    }

    fn context(q: Option<&str>, filters: Option<&str>) -> TraceListContext {
        TraceListContext {
            from: 10_500,
            to: 13_000,
            sort: TraceListSort::Latest,
            page_size: 20,
            q: q.map(String::from),
            filters: filter::parse(filters, &canonical_trace_schema(), 32).expect("filters"),
            boundary: None,
        }
    }

    fn tally(total: u64, errors: u64) -> Tally {
        Tally { total, errors }
    }

    /// Six summaries: one before the range, one at its end, four inside.
    fn summaries() -> RecordBatch {
        let schema = Arc::new(Schema::new(vec![
            Field::new("trace_id", DataType::Utf8, false),
            Field::new("service.name", DataType::Utf8, true),
            Field::new(TRACE_SUMMARY_START_NS_FIELD, DataType::Int64, false),
            Field::new(TRACE_SUMMARY_SPAN_COUNT_FIELD, DataType::Int64, false),
            Field::new(TRACE_SUMMARY_ERROR_COUNT_FIELD, DataType::Int64, false),
        ]));

        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(StringArray::from(vec!["a", "b", "c", "d", "e", "f"])),
                Arc::new(StringArray::from(vec![
                    "web", "web", "checkout", "web", "checkout", "web",
                ])),
                // In nanoseconds: the second begins on the first nanosecond of
                // the range, the last on the first nanosecond after it.
                Arc::new(Int64Array::from(vec![
                    10_499_999, 10_500_000, 10_999_999, 11_000_000, 12_999_999, 13_000_000,
                ])),
                Arc::new(Int64Array::from(vec![1, 1, 4, 2, 3, 1])),
                Arc::new(Int64Array::from(vec![1, 0, 2, 0, 1, 1])),
            ],
        )
        .expect("record batch")
    }

    fn tallied(context: &TraceListContext, batch: &RecordBatch) -> Vec<Tally> {
        let plan = plan();
        let mut tallies = vec![Tally::default(); plan.bars()];
        tally_batch(batch, context, &plan, &mut tallies);
        tallies
    }

    #[test]
    fn summaries_are_counted_into_the_bar_they_began_in_with_errors_apart() {
        assert_eq!(
            tallied(&context(None, None), &summaries()),
            vec![tally(2, 1), tally(1, 0), tally(1, 1)]
        );
    }

    #[test]
    fn free_text_and_summary_filters_narrow_what_is_counted() {
        assert_eq!(
            tallied(&context(Some("CHECK"), None), &summaries()),
            vec![tally(1, 1), tally(0, 0), tally(1, 1)]
        );
        assert_eq!(
            tallied(
                &context(
                    None,
                    Some(r#"[{"field":"span_count","op":">=","value":"2"}]"#)
                ),
                &summaries()
            ),
            vec![tally(1, 1), tally(1, 0), tally(1, 1)]
        );
    }

    #[test]
    fn a_start_stored_as_another_type_is_still_read() {
        let schema = Arc::new(Schema::new(vec![
            Field::new(TRACE_SUMMARY_START_NS_FIELD, DataType::UInt64, false),
            Field::new(TRACE_SUMMARY_ERROR_COUNT_FIELD, DataType::Utf8, true),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(UInt64Array::from(vec![10_600_000, 12_000_000])),
                Arc::new(StringArray::from(vec![Some("3"), None])),
            ],
        )
        .expect("record batch");

        assert_eq!(
            tallied(&context(None, None), &batch),
            vec![tally(1, 1), tally(0, 0), tally(1, 0)]
        );
    }

    #[test]
    fn only_the_columns_the_query_looks_at_are_read() {
        assert_eq!(
            read_columns(&context(None, None)),
            vec![
                TRACE_SUMMARY_ERROR_COUNT_FIELD,
                TRACE_SUMMARY_START_NS_FIELD
            ]
        );

        let narrowed = read_columns(&context(
            Some("checkout"),
            Some(r#"[{"field":"span_count","op":">=","value":"2"}]"#),
        ));

        for column in TEXT_COLUMNS {
            assert!(narrowed.contains(&column));
        }

        assert!(narrowed.contains(&TRACE_SUMMARY_SPAN_COUNT_FIELD));
        assert!(narrowed.contains(&TRACE_SUMMARY_START_NS_FIELD));
    }
}
