// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Signed keyset cursors for the Span list.

use serde::{Deserialize, Serialize};

use super::{SpanListContext, SpanListItem, SpanListSort};
use crate::{
    api::http::{
        pagination::cursor::{CursorDirection, decode_signed_cursor, encode_signed_cursor},
        routes::web::trace::{
            filter::{TraceCondition, TraceFilter},
            list::MAX_TRACE_PAGE_SIZE,
        },
    },
    app::iam::IamService,
    shared::{Error, Result, ids::Id},
};

const CURSOR_VERSION: u8 = 1;
const CURSOR_PURPOSE: &str = "web.span-list.v1";

/// Where a page begins: the keys of the Span on its edge.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SpanCursorPosition {
    /// The key the list is sorted by first, when that is not the start.
    pub(super) primary: i64,
    pub(super) start_ns: i64,
    pub(super) span_id: String,
    pub(super) trace_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SpanCursorPayload {
    version: u8,
    pub(super) from: i64,
    pub(super) to: i64,
    pub(super) sort: SpanListSort,
    pub(super) page_size: usize,
    pub(super) q: Option<String>,
    pub(super) filters: Vec<TraceFilter>,
    #[serde(default, rename = "where", skip_serializing_if = "Option::is_none")]
    pub(super) condition: Option<TraceCondition>,
    pub(super) direction: CursorDirection,
    pub(super) position: SpanCursorPosition,
}

#[derive(Clone, Debug)]
pub(super) struct SpanCursorBoundary {
    pub(super) direction: CursorDirection,
    pub(super) position: SpanCursorPosition,
}

pub(super) fn decode(iam: &IamService, org_id: &Id, token: &str) -> Result<SpanCursorPayload> {
    let payload = decode_signed_cursor::<SpanCursorPayload>(iam, org_id, CURSOR_PURPOSE, token)?;

    let ids_fit = |id: &str| !id.is_empty() && id.len() <= 128;

    if payload.version != CURSOR_VERSION
        || payload.to <= payload.from
        || !(1..=MAX_TRACE_PAGE_SIZE).contains(&payload.page_size)
        || !ids_fit(&payload.position.span_id)
        || !ids_fit(&payload.position.trace_id)
    {
        return Err(Error::invalid("invalid span cursor payload"));
    }

    Ok(payload)
}

pub(super) fn encode(
    iam: &IamService,
    org_id: &Id,
    context: &SpanListContext,
    direction: CursorDirection,
    item: &SpanListItem,
) -> Result<String> {
    let payload = SpanCursorPayload {
        version: CURSOR_VERSION,
        from: context.from,
        to: context.to,
        sort: context.sort,
        page_size: context.page_size,
        q: context.q.clone(),
        filters: context.filters.clone(),
        condition: context.condition.clone(),
        direction,
        position: position_for(item, context.sort),
    };

    encode_signed_cursor(iam, org_id, CURSOR_PURPOSE, payload)
}

fn position_for(item: &SpanListItem, sort: SpanListSort) -> SpanCursorPosition {
    let primary = match sort {
        SpanListSort::Latest | SpanListSort::Earliest => item.start_ns,
        SpanListSort::DurationDesc | SpanListSort::DurationAsc => item.duration_ns,
    };

    SpanCursorPosition {
        primary,
        start_ns: item.start_ns,
        span_id: item.span_id.clone(),
        trace_id: item.trace_id.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_position_holds_the_sort_key_the_start_and_the_ids() {
        let item = SpanListItem {
            trace_id: "t".into(),
            span_id: "s".into(),
            start_ns: 88,
            duration_ns: 99,
            ..Default::default()
        };

        let by_duration = position_for(&item, SpanListSort::DurationAsc);
        assert_eq!((by_duration.primary, by_duration.start_ns), (99, 88));
        assert_eq!(position_for(&item, SpanListSort::Latest).primary, 88);
        assert_eq!(by_duration.span_id, "s");
        assert_eq!(by_duration.trace_id, "t");
    }
}
