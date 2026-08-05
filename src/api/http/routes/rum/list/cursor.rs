// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

use super::{
    errors::{ErrorContext, ErrorRow},
    sessions::{SessionContext, SessionRow},
    sql_literal,
};
use crate::{
    api::http::pagination::cursor::{CursorDirection, decode_signed_cursor, encode_signed_cursor},
    app::iam::IamService,
    shared::{
        Error, Result,
        cursor::{CursorSortDirection, CursorValue, lexicographic_seek},
        ids::Id,
    },
};

const VERSION: u8 = 1;
const SESSION_PURPOSE: &str = "rum.session-list.v1";
const ERROR_PURPOSE: &str = "rum.error-list.v1";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct SessionCursorPayload {
    version: u8,
    pub(super) from: i64,
    pub(super) to: i64,
    pub(super) page_size: usize,
    pub(super) query: Option<String>,
    pub(super) country: Option<String>,
    pub(super) browser: Option<String>,
    #[serde(default)]
    pub(super) replay_only: bool,
    pub(super) direction: CursorDirection,
    pub(super) started_at_micros: i64,
    pub(super) session_id: String,
    pub(super) event_id: String,
}

#[derive(Clone, Debug)]
pub(super) struct SessionBoundary {
    pub(super) direction: CursorDirection,
    pub(super) started_at_micros: i64,
    pub(super) session_id: String,
    pub(super) event_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct ErrorCursorPayload {
    version: u8,
    pub(super) from: i64,
    pub(super) to: i64,
    pub(super) page_size: usize,
    pub(super) query: Option<String>,
    pub(super) status: Option<String>,
    pub(super) direction: CursorDirection,
    pub(super) count: i64,
    pub(super) last_seen_micros: i64,
    pub(super) fingerprint: String,
}

#[derive(Clone, Debug)]
pub(super) struct ErrorBoundary {
    pub(super) direction: CursorDirection,
    pub(super) count: i64,
    pub(super) last_seen_micros: i64,
    pub(super) fingerprint: String,
}

pub(super) fn decode_session(
    iam: &IamService,
    org_id: &Id,
    token: &str,
) -> Result<SessionCursorPayload> {
    let payload =
        decode_signed_cursor::<SessionCursorPayload>(iam, org_id, SESSION_PURPOSE, token)?;
    if payload.version != VERSION
        || payload.to <= payload.from
        || payload.session_id.is_empty()
        || payload.event_id.is_empty()
        || !(1..=super::MAX_PAGE_SIZE).contains(&payload.page_size)
    {
        return Err(Error::invalid("invalid RUM session cursor"));
    }
    Ok(payload)
}

pub(super) fn encode_session(
    iam: &IamService,
    org_id: &Id,
    context: &SessionContext,
    direction: CursorDirection,
    row: &SessionRow,
) -> Result<String> {
    encode_signed_cursor(
        iam,
        org_id,
        SESSION_PURPOSE,
        SessionCursorPayload {
            version: VERSION,
            from: context.from,
            to: context.to,
            page_size: context.page_size,
            query: context.query.clone(),
            country: context.country.clone(),
            browser: context.browser.clone(),
            replay_only: context.replay_only,
            direction,
            started_at_micros: row.started_at_micros,
            session_id: row.session_id.clone(),
            event_id: row.event_id.clone(),
        },
    )
}

pub(super) fn session_seek(boundary: &SessionBoundary, timestamp: &str) -> String {
    lexicographic_seek(
        &[
            (
                timestamp,
                CursorValue::Integer(boundary.started_at_micros),
                CursorSortDirection::Desc,
            ),
            (
                "session_id",
                CursorValue::Text(boundary.session_id.clone()),
                CursorSortDirection::Desc,
            ),
            (
                crate::domain::ingestion::EVENT_ID_FIELD,
                CursorValue::Text(boundary.event_id.clone()),
                CursorSortDirection::Desc,
            ),
        ],
        boundary.direction,
        sql_literal,
    )
}

pub(super) fn decode_error(
    iam: &IamService,
    org_id: &Id,
    token: &str,
) -> Result<ErrorCursorPayload> {
    let payload = decode_signed_cursor::<ErrorCursorPayload>(iam, org_id, ERROR_PURPOSE, token)?;
    if payload.version != VERSION
        || payload.to <= payload.from
        || payload.fingerprint.is_empty()
        || !(1..=super::MAX_PAGE_SIZE).contains(&payload.page_size)
    {
        return Err(Error::invalid("invalid RUM error cursor"));
    }
    Ok(payload)
}

pub(super) fn encode_error(
    iam: &IamService,
    org_id: &Id,
    context: &ErrorContext,
    direction: CursorDirection,
    row: &ErrorRow,
) -> Result<String> {
    encode_signed_cursor(
        iam,
        org_id,
        ERROR_PURPOSE,
        ErrorCursorPayload {
            version: VERSION,
            from: context.from,
            to: context.to,
            page_size: context.page_size,
            query: context.query.clone(),
            status: context.status.clone(),
            direction,
            count: row.item.count,
            last_seen_micros: row.item.last_seen_micros,
            fingerprint: row.item.fingerprint.clone(),
        },
    )
}

pub(super) fn error_seek(boundary: &ErrorBoundary) -> String {
    lexicographic_seek(
        &[
            (
                "error_count",
                CursorValue::Integer(boundary.count),
                CursorSortDirection::Desc,
            ),
            (
                "last_seen_micros",
                CursorValue::Integer(boundary.last_seen_micros),
                CursorSortDirection::Desc,
            ),
            (
                "fingerprint",
                CursorValue::Text(boundary.fingerprint.clone()),
                CursorSortDirection::Desc,
            ),
        ],
        boundary.direction,
        sql_literal,
    )
}
