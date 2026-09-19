// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

use super::{LogFilter, LogListContext, LogListRow, sql_literal};
use crate::{
    api::http::pagination::cursor::{CursorDirection, decode_signed_cursor, encode_signed_cursor},
    app::iam::IamService,
    shared::{
        Error, Result,
        cursor::{CursorSortDirection, CursorValue, lexicographic_seek},
        ids::Id,
    },
};

const VERSION: u8 = 2;
const PURPOSE: &str = "web.log-list.v2";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct LogCursorPayload {
    version: u8,
    pub(super) stream: String,
    pub(super) from: i64,
    pub(super) to: i64,
    pub(super) page_size: usize,
    pub(super) filters: Vec<LogFilter>,
    pub(super) free_text: Vec<String>,
    pub(super) direction: CursorDirection,
    pub(super) timestamp_micros: i64,
    pub(super) event_id: String,
}

#[derive(Clone, Debug)]
pub(super) struct LogCursorBoundary {
    pub(super) direction: CursorDirection,
    pub(super) timestamp_micros: i64,
    pub(super) event_id: String,
}

pub(super) fn decode(iam: &IamService, org_id: &Id, token: &str) -> Result<LogCursorPayload> {
    let payload = decode_signed_cursor::<LogCursorPayload>(iam, org_id, PURPOSE, token)?;
    if payload.version != VERSION
        || payload.stream.is_empty()
        || payload.to <= payload.from
        || !(1..=super::MAX_PAGE_SIZE).contains(&payload.page_size)
        || payload.event_id.len() > 256
    {
        return Err(Error::invalid("invalid log cursor payload"));
    }
    Ok(payload)
}

pub(super) fn encode(
    iam: &IamService,
    org_id: &Id,
    context: &LogListContext,
    direction: CursorDirection,
    row: &LogListRow,
) -> Result<String> {
    encode_signed_cursor(
        iam,
        org_id,
        PURPOSE,
        LogCursorPayload {
            version: VERSION,
            stream: context.stream.clone(),
            from: context.from,
            to: context.to,
            page_size: context.page_size,
            filters: context.filters.clone(),
            free_text: context.free_text.clone(),
            direction,
            timestamp_micros: row.timestamp_micros,
            event_id: row.event_id.clone(),
        },
    )
}

pub(super) fn seek_predicate(boundary: &LogCursorBoundary, event_id_expression: &str) -> String {
    lexicographic_seek(
        &[
            (
                "CAST(_timestamp AS BIGINT)",
                CursorValue::Integer(boundary.timestamp_micros),
                CursorSortDirection::Desc,
            ),
            (
                event_id_expression,
                CursorValue::Text(boundary.event_id.clone()),
                CursorSortDirection::Desc,
            ),
        ],
        boundary.direction,
        sql_literal,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_seek_uses_timestamp_and_unique_event_id() {
        let boundary = LogCursorBoundary {
            direction: CursorDirection::After,
            timestamp_micros: 42,
            event_id: "batch:2".into(),
        };
        assert_eq!(
            seek_predicate(&boundary, "_event_id"),
            "((CAST(_timestamp AS BIGINT) < 42) OR (CAST(_timestamp AS BIGINT) = 42 AND _event_id < 'batch:2'))"
        );
    }
}
