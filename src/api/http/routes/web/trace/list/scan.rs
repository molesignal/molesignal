// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Trace summary 的专用 Parquet Top-K 扫描。

use std::cmp::Ordering;

use arrow::{
    array::{Array, Float64Array, Int64Array, RecordBatch, StringArray, UInt64Array},
    datatypes::DataType,
};
use futures::StreamExt;

mod files;

use files::{file_may_match_cursor, file_may_match_filters, order_files, remaining_cannot_beat};

use super::{TraceListContext, TraceListRow, TraceListSort};
#[cfg(test)]
use crate::domain::storage::ParquetFileMeta;
use crate::{
    api::{AppState, http::pagination::cursor::CursorDirection},
    domain::{storage::PhysicalDatasetKind, stream::FieldType},
    infra::storage::parquet::reader::{ParquetReader, ReadOptions},
    shared::{
        Error, Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
        trace::summary::{
            TRACE_SUMMARY_DURATION_NS_FIELD, TRACE_SUMMARY_ERROR_COUNT_FIELD,
            TRACE_SUMMARY_SPAN_COUNT_FIELD, TRACE_SUMMARY_START_NS_FIELD,
        },
    },
};

const COLUMNS: &[&str] = &[
    "_timestamp",
    "trace_id",
    "span_id",
    "parent_span_id",
    "service.name",
    "name",
    "status_code",
    TRACE_SUMMARY_START_NS_FIELD,
    TRACE_SUMMARY_DURATION_NS_FIELD,
    TRACE_SUMMARY_SPAN_COUNT_FIELD,
    TRACE_SUMMARY_ERROR_COUNT_FIELD,
];

pub(super) async fn run(
    state: &AppState,
    org_id: &Id,
    stream: &str,
    context: &TraceListContext,
    fetch_limit: usize,
) -> Result<Vec<TraceListRow>> {
    let range = TimeRange::new(TimestampMicros(context.from), TimestampMicros(context.to));
    let files = state
        .storage
        .parquet_file_meta
        .find_dataset(
            org_id,
            stream,
            crate::domain::stream::StreamType::Traces,
            PhysicalDatasetKind::TraceSummary,
            range,
        )
        .await?;
    let mut files = order_files(files, context);
    let reader = ParquetReader::new(state.storage.object_store.clone());
    let mut top = Vec::with_capacity(fetch_limit);
    let from_ns = context
        .from
        .checked_mul(1_000)
        .ok_or_else(|| Error::invalid("trace time_from is out of range"))?;
    let to_ns = context
        .to
        .checked_mul(1_000)
        .ok_or_else(|| Error::invalid("trace time_to is out of range"))?;
    let mut scanned_files = 0_usize;
    let mut scanned_rows = 0_usize;

    while let Some(file) = files.pop_front() {
        if !file_may_match_cursor(&file, context) || !file_may_match_filters(&file, context) {
            continue;
        }
        let options = ReadOptions::new()
            .with_time_range(context.from, context.to)
            .with_columns(COLUMNS)
            .with_known_size(file.size_bytes);
        let mut batches = match reader
            .stream_from_store(
                state.storage.object_store.clone(),
                &file.object_key,
                options,
            )
            .await
        {
            Ok(stream) => stream,
            Err(Error::NotFound(_)) => {
                tracing::warn!(object_key = %file.object_key, "trace summary parquet is missing");
                continue;
            }
            Err(error) => return Err(error),
        };
        scanned_files += 1;
        while let Some(batch) = batches.next().await {
            let batch = batch?;
            scanned_rows += batch.num_rows();
            if collect_batch(&batch, context, from_ns, to_ns, fetch_limit, &mut top)? {
                break;
            }
        }

        if top.len() >= fetch_limit
            && files
                .front()
                .is_some_and(|next| remaining_cannot_beat(next, context, &top))
        {
            break;
        }
    }
    top.sort_by(|left, right| effective_cmp(left, right, context));
    tracing::debug!(
        scanned_files,
        scanned_rows,
        returned = top.len(),
        sort = ?context.sort,
        "trace summary top-k scan completed"
    );
    Ok(top)
}

fn collect_batch(
    batch: &RecordBatch,
    context: &TraceListContext,
    from_ns: i64,
    to_ns: i64,
    limit: usize,
    top: &mut Vec<TraceListRow>,
) -> Result<bool> {
    let physically_ordered = physical_order_matches_effective(context);
    for row_index in 0..batch.num_rows() {
        let Some(trace_id) = string_at(batch, "trace_id", row_index) else {
            continue;
        };
        let start_ns = integer_at(batch, TRACE_SUMMARY_START_NS_FIELD, row_index).unwrap_or(0);
        if start_ns < from_ns || start_ns >= to_ns {
            if physically_ordered && start_ns < from_ns {
                return Ok(true);
            }
            continue;
        }
        let duration_ns = integer_at(batch, TRACE_SUMMARY_DURATION_NS_FIELD, row_index)
            .unwrap_or_default()
            .max(0);
        let candidate = TraceListRow {
            item: super::TraceListItem {
                trace_id: trace_id.to_string(),
                service: string_at(batch, "service.name", row_index)
                    .unwrap_or_default()
                    .to_string(),
                operation: string_at(batch, "name", row_index)
                    .unwrap_or_default()
                    .to_string(),
                start_ns,
                duration_ms: duration_ns as f64 / 1_000_000.0,
                span_count: integer_at(batch, TRACE_SUMMARY_SPAN_COUNT_FIELD, row_index)
                    .unwrap_or_default(),
                error_count: integer_at(batch, TRACE_SUMMARY_ERROR_COUNT_FIELD, row_index)
                    .unwrap_or_default(),
            },
            duration_ns,
        };
        if matches_text_and_filters(batch, row_index, context)
            && matches_cursor(&candidate, context)
        {
            push_top_k(top, candidate.clone(), limit, context);
        }
        if physically_ordered
            && top.len() >= limit
            && top
                .iter()
                .max_by(|left, right| effective_cmp(left, right, context))
                .is_some_and(|worst| effective_cmp(&candidate, worst, context) != Ordering::Less)
        {
            // Trace-summary files are physically sorted by the exact
            // `(start_ns, trace_id)` tuple. Once the current row is no better
            // than the retained worst row, no later row in this file can enter
            // the page, so the remaining row groups need not be decoded.
            return Ok(true);
        }
    }
    Ok(false)
}

fn physical_order_matches_effective(context: &TraceListContext) -> bool {
    matches!(
        (
            context.sort,
            context.boundary.as_ref().map(|boundary| boundary.direction),
        ),
        (TraceListSort::Latest, None | Some(CursorDirection::After))
            | (TraceListSort::Earliest, Some(CursorDirection::Before))
    )
}

fn push_top_k(
    top: &mut Vec<TraceListRow>,
    candidate: TraceListRow,
    limit: usize,
    context: &TraceListContext,
) {
    if let Some(existing) = top
        .iter()
        .position(|row| row.item.trace_id == candidate.item.trace_id)
    {
        if effective_cmp(&candidate, &top[existing], context) == Ordering::Less {
            top[existing] = candidate;
        }
        return;
    }
    if top.len() < limit {
        top.push(candidate);
        return;
    }
    let worst = top
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| effective_cmp(left, right, context))
        .map(|(index, _)| index)
        .unwrap_or_default();
    if effective_cmp(&candidate, &top[worst], context) == Ordering::Less {
        top[worst] = candidate;
    }
}

fn canonical_cmp(left: &TraceListRow, right: &TraceListRow, sort: TraceListSort) -> Ordering {
    let primary = match sort {
        TraceListSort::Latest => right.item.start_ns.cmp(&left.item.start_ns),
        TraceListSort::Earliest => left.item.start_ns.cmp(&right.item.start_ns),
        TraceListSort::DurationDesc => right.duration_ns.cmp(&left.duration_ns),
        TraceListSort::DurationAsc => left.duration_ns.cmp(&right.duration_ns),
        TraceListSort::SpanCountDesc => right.item.span_count.cmp(&left.item.span_count),
        TraceListSort::ErrorsDesc => right.item.error_count.cmp(&left.item.error_count),
    };
    if primary != Ordering::Equal {
        return primary;
    }
    match sort {
        TraceListSort::Latest => right.item.trace_id.cmp(&left.item.trace_id),
        TraceListSort::Earliest => left.item.trace_id.cmp(&right.item.trace_id),
        _ => right
            .item
            .start_ns
            .cmp(&left.item.start_ns)
            .then_with(|| right.item.trace_id.cmp(&left.item.trace_id)),
    }
}

fn effective_cmp(
    left: &TraceListRow,
    right: &TraceListRow,
    context: &TraceListContext,
) -> Ordering {
    let canonical = canonical_cmp(left, right, context.sort);
    if context.boundary.as_ref().map(|value| value.direction) == Some(CursorDirection::Before) {
        canonical.reverse()
    } else {
        canonical
    }
}

fn matches_cursor(row: &TraceListRow, context: &TraceListContext) -> bool {
    let Some(boundary) = context.boundary.as_ref() else {
        return true;
    };
    let position = &boundary.position;
    let edge = TraceListRow {
        item: super::TraceListItem {
            trace_id: position.trace_id.clone(),
            service: String::new(),
            operation: String::new(),
            start_ns: position.start_ns,
            duration_ms: 0.0,
            span_count: if context.sort == TraceListSort::SpanCountDesc {
                position.primary
            } else {
                0
            },
            error_count: if context.sort == TraceListSort::ErrorsDesc {
                position.primary
            } else {
                0
            },
        },
        duration_ns: if matches!(
            context.sort,
            TraceListSort::DurationDesc | TraceListSort::DurationAsc
        ) {
            position.primary
        } else {
            0
        },
    };
    let ordering = canonical_cmp(row, &edge, context.sort);
    match boundary.direction {
        CursorDirection::After => ordering == Ordering::Greater,
        CursorDirection::Before => ordering == Ordering::Less,
    }
}

fn matches_text_and_filters(batch: &RecordBatch, row: usize, context: &TraceListContext) -> bool {
    let values = || {
        ["trace_id", "span_id", "service.name", "name", "status_code"]
            .into_iter()
            .filter_map(|field| string_at(batch, field, row))
    };
    if let Some(query) = context.q.as_deref() {
        let query = query.to_ascii_lowercase();
        if !values().any(|value| value.to_ascii_lowercase().contains(&query)) {
            return false;
        }
    }
    context.filters.iter().all(|filter| {
        let Some(field) = filter.summary_column() else {
            return false;
        };
        match filter.data_type {
            FieldType::Utf8 => {
                let value = string_at(batch, field, row).unwrap_or_default();
                match filter.op.as_str() {
                    "=" => value == filter.value,
                    "!=" => value != filter.value,
                    "contains" => value.contains(&filter.value),
                    _ => false,
                }
            }
            FieldType::Int64 | FieldType::Timestamp => integer_at(batch, field, row)
                .zip(filter.integer_value())
                .is_some_and(|(left, right)| compare_i64(left, filter.op.as_str(), right)),
            FieldType::Float64 => numeric_at(batch, field, row)
                .zip(filter.float_value())
                .is_some_and(|(left, right)| compare_f64(left, filter.op.as_str(), right)),
            FieldType::Bool => bool_at(batch, field, row)
                .zip(filter.bool_value())
                .is_some_and(|(left, right)| match filter.op.as_str() {
                    "=" => left == right,
                    "!=" => left != right,
                    _ => false,
                }),
            FieldType::Json => false,
        }
    })
}

fn compare_i64(left: i64, op: &str, right: i64) -> bool {
    match op {
        "=" => left == right,
        "!=" => left != right,
        ">" => left > right,
        ">=" => left >= right,
        "<" => left < right,
        "<=" => left <= right,
        _ => false,
    }
}

fn compare_f64(left: f64, op: &str, right: f64) -> bool {
    match op {
        "=" => left == right,
        "!=" => left != right,
        ">" => left > right,
        ">=" => left >= right,
        "<" => left < right,
        "<=" => left <= right,
        _ => false,
    }
}

fn string_at<'a>(batch: &'a RecordBatch, name: &str, row: usize) -> Option<&'a str> {
    batch
        .column_by_name(name)?
        .as_any()
        .downcast_ref::<StringArray>()?
        .is_valid(row)
        .then(|| {
            batch
                .column_by_name(name)
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap()
                .value(row)
        })
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
        DataType::Float64 => {
            Some(column.as_any().downcast_ref::<Float64Array>()?.value(row) as i64)
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

fn numeric_at(batch: &RecordBatch, name: &str, row: usize) -> Option<f64> {
    let column = batch.column_by_name(name)?;
    if column.is_null(row) {
        return None;
    }
    match column.data_type() {
        DataType::Float64 => Some(column.as_any().downcast_ref::<Float64Array>()?.value(row)),
        _ => integer_at(batch, name, row).map(|value| value as f64),
    }
}

fn bool_at(batch: &RecordBatch, name: &str, row: usize) -> Option<bool> {
    let column = batch.column_by_name(name)?;
    if column.is_null(row) {
        return None;
    }
    column
        .as_any()
        .downcast_ref::<arrow::array::BooleanArray>()
        .map(|values| values.value(row))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::datatypes::{Field, Schema};

    use super::*;
    use crate::api::http::pagination::cursor::CursorDirection;

    fn context(sort: TraceListSort) -> TraceListContext {
        TraceListContext {
            from: 0,
            to: 1_000,
            sort,
            page_size: 2,
            q: None,
            filters: Vec::new(),
            boundary: None,
        }
    }

    fn row(
        trace_id: &str,
        start_ns: i64,
        duration_ns: i64,
        span_count: i64,
        error_count: i64,
    ) -> TraceListRow {
        TraceListRow {
            item: super::super::TraceListItem {
                trace_id: trace_id.to_string(),
                service: "api".to_string(),
                operation: "GET /".to_string(),
                start_ns,
                duration_ms: duration_ns as f64 / 1_000_000.0,
                span_count,
                error_count,
            },
            duration_ns,
        }
    }

    fn ids(rows: &[TraceListRow]) -> Vec<&str> {
        rows.iter().map(|row| row.item.trace_id.as_str()).collect()
    }

    #[test]
    fn latest_top_k_uses_trace_id_as_stable_tie_breaker() {
        let context = context(TraceListSort::Latest);
        let mut top = Vec::new();
        for candidate in [
            row("a", 10, 1, 1, 0),
            row("b", 20, 1, 1, 0),
            row("c", 20, 1, 1, 0),
        ] {
            push_top_k(&mut top, candidate, 2, &context);
        }
        top.sort_by(|left, right| effective_cmp(left, right, &context));
        assert_eq!(ids(&top), vec!["c", "b"]);
    }

    #[test]
    fn duration_top_k_uses_time_then_trace_id_after_primary() {
        let context = context(TraceListSort::DurationDesc);
        let mut top = vec![
            row("a", 10, 100, 1, 0),
            row("b", 20, 100, 1, 0),
            row("c", 20, 100, 1, 0),
            row("d", 30, 90, 1, 0),
        ];
        top.sort_by(|left, right| effective_cmp(left, right, &context));
        assert_eq!(ids(&top), vec!["c", "b", "a", "d"]);
    }

    #[test]
    fn top_k_collapses_retried_trace_summary_rows() {
        let context = context(TraceListSort::Latest);
        let mut top = Vec::new();
        push_top_k(&mut top, row("same", 10, 1, 1, 0), 2, &context);
        push_top_k(&mut top, row("same", 20, 2, 2, 1), 2, &context);
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].item.start_ns, 20);
    }

    #[test]
    fn cursor_strictly_excludes_boundary_for_both_directions() {
        let boundary_row = row("b", 20, 100, 2, 1);
        let position = super::super::cursor::TraceCursorPosition {
            primary: 20,
            start_ns: 20,
            trace_id: "b".to_string(),
        };
        let mut after = context(TraceListSort::Latest);
        after.boundary = Some(super::super::cursor::TraceCursorBoundary {
            direction: CursorDirection::After,
            position: position.clone(),
        });
        assert!(!matches_cursor(&boundary_row, &after));
        assert!(matches_cursor(&row("a", 10, 1, 1, 0), &after));
        assert!(!matches_cursor(&row("c", 20, 1, 1, 0), &after));

        let mut before = context(TraceListSort::Latest);
        before.boundary = Some(super::super::cursor::TraceCursorBoundary {
            direction: CursorDirection::Before,
            position,
        });
        assert!(!matches_cursor(&boundary_row, &before));
        assert!(matches_cursor(&row("c", 20, 1, 1, 0), &before));
        assert!(!matches_cursor(&row("a", 10, 1, 1, 0), &before));
    }

    #[test]
    fn summary_numeric_filters_use_typed_comparisons() {
        let schema = Arc::new(Schema::new(vec![
            Field::new("trace_id", DataType::Utf8, false),
            Field::new(TRACE_SUMMARY_SPAN_COUNT_FIELD, DataType::Int64, false),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(StringArray::from(vec!["trace-a", "trace-b"])),
                Arc::new(Int64Array::from(vec![1, 3])),
            ],
        )
        .unwrap();
        let mut context = context(TraceListSort::Latest);
        context.filters = super::super::filter::parse(
            Some(r#"[{"field":"span_count","op":">=","value":"2"}]"#),
            &crate::shared::trace::normalization::canonical_trace_schema(),
            32,
        )
        .unwrap();

        assert!(!matches_text_and_filters(&batch, 0, &context));
        assert!(matches_text_and_filters(&batch, 1, &context));
    }

    #[test]
    fn exact_filter_rejects_file_only_when_zone_map_proves_no_match() {
        let mut context = context(TraceListSort::Latest);
        context.filters = super::super::filter::parse(
            Some(r#"[{"field":"trace_id","op":"=","value":"trace-z"}]"#),
            &crate::shared::trace::normalization::canonical_trace_schema(),
            32,
        )
        .unwrap();
        let mut file = ParquetFileMeta {
            id: Id::from_string("file"),
            org_id: Id::from_string("org"),
            stream: "default".into(),
            stream_type: crate::domain::stream::StreamType::Traces,
            dataset_kind: PhysicalDatasetKind::TraceSummary,
            object_key: "file.parquet".into(),
            time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(1)),
            rows: 1,
            size_bytes: 1,
            min_values: serde_json::Map::new(),
            max_values: serde_json::Map::new(),
            deleted: false,
        };
        file.min_values.insert("trace_id".into(), "trace-a".into());
        file.max_values.insert("trace_id".into(), "trace-m".into());
        assert!(!file_may_match_filters(&file, &context));

        file.max_values.insert("trace_id".into(), "trace-z".into());
        assert!(file_may_match_filters(&file, &context));
        file.max_values.clear();
        assert!(file_may_match_filters(&file, &context));
    }
}
