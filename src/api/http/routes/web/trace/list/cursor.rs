// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Signed keyset cursors for the trace summary list.

use serde::{Deserialize, Serialize};

use super::{TraceFilter, TraceListContext, TraceListRow, TraceListSort};
use crate::{
    api::http::pagination::cursor::{CursorDirection, decode_signed_cursor, encode_signed_cursor},
    app::iam::IamService,
    shared::{Error, Result, ids::Id},
};

const CURSOR_VERSION: u8 = 2;
const CURSOR_PURPOSE: &str = "web.trace-list.v2";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct TraceCursorPosition {
    pub(super) primary: i64,
    pub(super) start_ns: i64,
    pub(super) trace_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct TraceCursorPayload {
    version: u8,
    pub(super) from: i64,
    pub(super) to: i64,
    pub(super) sort: TraceListSort,
    pub(super) page_size: usize,
    pub(super) q: Option<String>,
    pub(super) filters: Vec<TraceFilter>,
    pub(super) direction: CursorDirection,
    pub(super) position: TraceCursorPosition,
}

#[derive(Clone, Debug)]
pub(super) struct TraceCursorBoundary {
    pub(super) direction: CursorDirection,
    pub(super) position: TraceCursorPosition,
}

pub(super) fn decode(iam: &IamService, org_id: &Id, token: &str) -> Result<TraceCursorPayload> {
    let payload = decode_signed_cursor::<TraceCursorPayload>(iam, org_id, CURSOR_PURPOSE, token)?;
    if payload.version != CURSOR_VERSION
        || payload.to <= payload.from
        || !(1..=super::MAX_TRACE_PAGE_SIZE).contains(&payload.page_size)
        || payload.position.trace_id.is_empty()
        || payload.position.trace_id.len() > 128
    {
        return Err(Error::invalid("invalid trace cursor payload"));
    }
    Ok(payload)
}

pub(super) fn encode(
    iam: &IamService,
    org_id: &Id,
    context: &TraceListContext,
    direction: CursorDirection,
    row: &TraceListRow,
) -> Result<String> {
    let payload = TraceCursorPayload {
        version: CURSOR_VERSION,
        from: context.from,
        to: context.to,
        sort: context.sort,
        page_size: context.page_size,
        q: context.q.clone(),
        filters: context.filters.clone(),
        direction,
        position: position_for(row, context.sort),
    };
    encode_signed_cursor(iam, org_id, CURSOR_PURPOSE, payload)
}

fn position_for(row: &TraceListRow, sort: TraceListSort) -> TraceCursorPosition {
    let primary = match sort {
        TraceListSort::Latest | TraceListSort::Earliest => row.item.start_ns,
        TraceListSort::DurationDesc | TraceListSort::DurationAsc => row.duration_ns,
        TraceListSort::SpanCountDesc => row.item.span_count,
        TraceListSort::ErrorsDesc => row.item.error_count,
    };
    TraceCursorPosition {
        primary,
        start_ns: row.item.start_ns,
        trace_id: row.item.trace_id.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_position_contains_primary_time_and_unique_key() {
        let row = TraceListRow {
            item: super::super::TraceListItem {
                trace_id: "trace-z".to_string(),
                service: "api".to_string(),
                operation: "GET /".to_string(),
                start_ns: 88,
                duration_ms: 0.000_099,
                span_count: 7,
                error_count: 1,
            },
            duration_ns: 99,
        };
        let position = position_for(&row, TraceListSort::DurationDesc);
        assert_eq!(position.primary, 99);
        assert_eq!(position.start_ns, 88);
        assert_eq!(position.trace_id, "trace-z");
    }
}
