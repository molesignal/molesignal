// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Trace-summary 文件排序、游标剪枝和 zone-map 剪枝。

use std::{cmp::Ordering, collections::VecDeque};

use super::{TraceListContext, TraceListRow, TraceListSort, effective_cmp};
use crate::{
    api::http::pagination::cursor::CursorDirection,
    domain::{storage::ParquetFileMeta, stream::FieldType},
    shared::trace::summary::{
        TRACE_SUMMARY_DURATION_NS_FIELD, TRACE_SUMMARY_ERROR_COUNT_FIELD,
        TRACE_SUMMARY_SPAN_COUNT_FIELD, TRACE_SUMMARY_START_NS_FIELD,
    },
};

pub(super) fn order_files(
    mut files: Vec<ParquetFileMeta>,
    context: &TraceListContext,
) -> VecDeque<ParquetFileMeta> {
    let descending = effective_primary_desc(context);
    files.sort_by(|left, right| {
        match (
            file_bound(left, context, descending),
            file_bound(right, context, descending),
        ) {
            (None, None) => left.object_key.cmp(&right.object_key),
            (None, Some(_)) => Ordering::Less,
            (Some(_), None) => Ordering::Greater,
            (Some(left), Some(right)) if descending => right.cmp(&left),
            (Some(left), Some(right)) => left.cmp(&right),
        }
    });
    files.into()
}

fn effective_primary_desc(context: &TraceListContext) -> bool {
    let canonical = matches!(
        context.sort,
        TraceListSort::Latest
            | TraceListSort::DurationDesc
            | TraceListSort::SpanCountDesc
            | TraceListSort::ErrorsDesc
    );
    canonical
        ^ (context.boundary.as_ref().map(|value| value.direction) == Some(CursorDirection::Before))
}

fn file_bound(file: &ParquetFileMeta, context: &TraceListContext, maximum: bool) -> Option<i64> {
    let value = match context.sort {
        TraceListSort::Latest | TraceListSort::Earliest => {
            let map = if maximum {
                &file.max_values
            } else {
                &file.min_values
            };
            return map
                .get(TRACE_SUMMARY_START_NS_FIELD)
                .and_then(serde_json::Value::as_i64)
                .or_else(|| {
                    let micros = if maximum {
                        file.time_range.end.0
                    } else {
                        file.time_range.start.0
                    };
                    // `_timestamp` is truncated from nanoseconds. A maximum fallback must include
                    // the entire final microsecond or early-stop could discard a row up to 999 ns
                    // newer than the recorded ParquetFileMeta timestamp.
                    Some(
                        micros
                            .saturating_mul(1_000)
                            .saturating_add(if maximum { 999 } else { 0 }),
                    )
                });
        }
        TraceListSort::DurationDesc | TraceListSort::DurationAsc => TRACE_SUMMARY_DURATION_NS_FIELD,
        TraceListSort::SpanCountDesc => TRACE_SUMMARY_SPAN_COUNT_FIELD,
        TraceListSort::ErrorsDesc => TRACE_SUMMARY_ERROR_COUNT_FIELD,
    };
    let map = if maximum {
        &file.max_values
    } else {
        &file.min_values
    };
    map.get(value).and_then(serde_json::Value::as_i64)
}

pub(super) fn remaining_cannot_beat(
    next: &ParquetFileMeta,
    context: &TraceListContext,
    top: &[TraceListRow],
) -> bool {
    let descending = effective_primary_desc(context);
    let Some(bound) = file_bound(next, context, descending) else {
        return false;
    };
    let Some(worst) = top
        .iter()
        .max_by(|left, right| effective_cmp(left, right, context))
    else {
        return false;
    };
    let worst = match context.sort {
        TraceListSort::Latest | TraceListSort::Earliest => worst.item.start_ns,
        TraceListSort::DurationDesc | TraceListSort::DurationAsc => worst.duration_ns,
        TraceListSort::SpanCountDesc => worst.item.span_count,
        TraceListSort::ErrorsDesc => worst.item.error_count,
    };
    if descending {
        bound < worst
    } else {
        bound > worst
    }
}

pub(super) fn file_may_match_cursor(file: &ParquetFileMeta, context: &TraceListContext) -> bool {
    let Some(boundary) = context.boundary.as_ref() else {
        return true;
    };
    if !matches!(
        context.sort,
        TraceListSort::Latest | TraceListSort::Earliest
    ) {
        return true;
    }
    let min_ns = file.time_range.start.0.saturating_mul(1_000);
    let max_ns = file
        .max_values
        .get(TRACE_SUMMARY_START_NS_FIELD)
        .and_then(serde_json::Value::as_i64)
        .unwrap_or_else(|| {
            file.time_range
                .end
                .0
                .saturating_mul(1_000)
                .saturating_add(999)
        });
    match (context.sort, boundary.direction) {
        (TraceListSort::Latest, CursorDirection::After)
        | (TraceListSort::Earliest, CursorDirection::Before) => {
            min_ns <= boundary.position.start_ns
        }
        (TraceListSort::Latest, CursorDirection::Before)
        | (TraceListSort::Earliest, CursorDirection::After) => max_ns >= boundary.position.start_ns,
        _ => true,
    }
}

/// Zone maps provide a cheap first pass before opening Parquet. Missing or incompatible bounds
/// always retain the file so pruning can never create a false negative.
pub(super) fn file_may_match_filters(file: &ParquetFileMeta, context: &TraceListContext) -> bool {
    context.filters.iter().all(|filter| {
        let Some(field) = filter.summary_column() else {
            return true;
        };
        match filter.data_type {
            FieldType::Utf8 => {
                if filter.op != "=" {
                    return true;
                }
                let Some(minimum) = file
                    .min_values
                    .get(field)
                    .and_then(serde_json::Value::as_str)
                else {
                    return true;
                };
                let Some(maximum) = file
                    .max_values
                    .get(field)
                    .and_then(serde_json::Value::as_str)
                else {
                    return true;
                };
                filter.value.as_str() >= minimum && filter.value.as_str() <= maximum
            }
            FieldType::Int64 | FieldType::Timestamp => {
                let Some(value) = filter.integer_value() else {
                    return true;
                };
                let Some(minimum) = file.min_values.get(field).and_then(json_i64) else {
                    return true;
                };
                let Some(maximum) = file.max_values.get(field).and_then(json_i64) else {
                    return true;
                };
                numeric_range_may_match(minimum, maximum, filter.op.as_str(), value)
            }
            _ => true,
        }
    })
}

fn json_i64(value: &serde_json::Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
}

fn numeric_range_may_match(minimum: i64, maximum: i64, op: &str, value: i64) -> bool {
    match op {
        "=" => minimum <= value && value <= maximum,
        "!=" => minimum != value || maximum != value,
        ">" => maximum > value,
        ">=" => maximum >= value,
        "<" => minimum < value,
        "<=" => minimum <= value,
        _ => true,
    }
}
