// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The order a Span-filter statement lists traces in, and where a page after
//! the first begins: the keys of the sort, compared with the cursor's.

use super::{qualified, sql_literal};
use crate::{
    api::http::{
        pagination::cursor::CursorDirection,
        routes::web::trace::list::{TraceListContext, TraceListSort},
    },
    shared::{
        cursor::{CursorSortDirection, CursorValue, lexicographic_seek},
        trace::summary::{
            TRACE_SUMMARY_DURATION_NS_FIELD, TRACE_SUMMARY_ERROR_COUNT_FIELD,
            TRACE_SUMMARY_SPAN_COUNT_FIELD, TRACE_SUMMARY_START_NS_FIELD,
        },
    },
};

pub(super) fn cursor_predicate(context: &TraceListContext, direction: CursorDirection) -> String {
    let position = &context
        .boundary
        .as_ref()
        .expect("boundary checked")
        .position;

    let fields = match context.sort {
        TraceListSort::Latest => vec![
            (
                qualified("s", TRACE_SUMMARY_START_NS_FIELD),
                CursorValue::Integer(position.start_ns),
                CursorSortDirection::Desc,
            ),
            (
                qualified("s", "trace_id"),
                CursorValue::Text(position.trace_id.clone()),
                CursorSortDirection::Desc,
            ),
        ],
        TraceListSort::Earliest => vec![
            (
                qualified("s", TRACE_SUMMARY_START_NS_FIELD),
                CursorValue::Integer(position.start_ns),
                CursorSortDirection::Asc,
            ),
            (
                qualified("s", "trace_id"),
                CursorValue::Text(position.trace_id.clone()),
                CursorSortDirection::Asc,
            ),
        ],
        sort => {
            let primary = match sort {
                TraceListSort::DurationDesc | TraceListSort::DurationAsc => {
                    TRACE_SUMMARY_DURATION_NS_FIELD
                }
                TraceListSort::SpanCountDesc => TRACE_SUMMARY_SPAN_COUNT_FIELD,
                TraceListSort::ErrorsDesc => TRACE_SUMMARY_ERROR_COUNT_FIELD,
                TraceListSort::Latest | TraceListSort::Earliest => unreachable!(),
            };

            let primary_direction = if sort == TraceListSort::DurationAsc {
                CursorSortDirection::Asc
            } else {
                CursorSortDirection::Desc
            };

            vec![
                (
                    qualified("s", primary),
                    CursorValue::Integer(position.primary),
                    primary_direction,
                ),
                (
                    qualified("s", TRACE_SUMMARY_START_NS_FIELD),
                    CursorValue::Integer(position.start_ns),
                    CursorSortDirection::Desc,
                ),
                (
                    qualified("s", "trace_id"),
                    CursorValue::Text(position.trace_id.clone()),
                    CursorSortDirection::Desc,
                ),
            ]
        }
    };

    let borrowed = fields
        .iter()
        .map(|(name, value, order)| (name.as_str(), value.clone(), *order))
        .collect::<Vec<_>>();

    lexicographic_seek(&borrowed, direction, sql_literal)
}

pub(super) fn order_by(context: &TraceListContext) -> String {
    let fields = match context.sort {
        TraceListSort::Latest => vec![
            (TRACE_SUMMARY_START_NS_FIELD, CursorSortDirection::Desc),
            ("trace_id", CursorSortDirection::Desc),
        ],
        TraceListSort::Earliest => vec![
            (TRACE_SUMMARY_START_NS_FIELD, CursorSortDirection::Asc),
            ("trace_id", CursorSortDirection::Asc),
        ],
        TraceListSort::DurationDesc => vec![
            (TRACE_SUMMARY_DURATION_NS_FIELD, CursorSortDirection::Desc),
            (TRACE_SUMMARY_START_NS_FIELD, CursorSortDirection::Desc),
            ("trace_id", CursorSortDirection::Desc),
        ],
        TraceListSort::DurationAsc => vec![
            (TRACE_SUMMARY_DURATION_NS_FIELD, CursorSortDirection::Asc),
            (TRACE_SUMMARY_START_NS_FIELD, CursorSortDirection::Desc),
            ("trace_id", CursorSortDirection::Desc),
        ],
        TraceListSort::SpanCountDesc => vec![
            (TRACE_SUMMARY_SPAN_COUNT_FIELD, CursorSortDirection::Desc),
            (TRACE_SUMMARY_START_NS_FIELD, CursorSortDirection::Desc),
            ("trace_id", CursorSortDirection::Desc),
        ],
        TraceListSort::ErrorsDesc => vec![
            (TRACE_SUMMARY_ERROR_COUNT_FIELD, CursorSortDirection::Desc),
            (TRACE_SUMMARY_START_NS_FIELD, CursorSortDirection::Desc),
            ("trace_id", CursorSortDirection::Desc),
        ],
    };

    let reverse = context
        .boundary
        .as_ref()
        .is_some_and(|boundary| boundary.direction == CursorDirection::Before);

    fields
        .into_iter()
        .map(|(field, direction)| {
            let direction = match (direction, reverse) {
                (CursorSortDirection::Asc, false) | (CursorSortDirection::Desc, true) => "ASC",
                (CursorSortDirection::Desc, false) | (CursorSortDirection::Asc, true) => "DESC",
            };

            format!("{} {direction}", qualified("s", field))
        })
        .collect::<Vec<_>>()
        .join(", ")
}
