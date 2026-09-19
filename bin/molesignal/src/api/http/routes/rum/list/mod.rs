// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Server-side RUM list projections with stable cursor pagination.

use axum::{Router, routing::get};
use serde::Deserialize;

use crate::{
    api::AppState,
    shared::{Error, Result, time::TimestampMicros},
};

mod cursor;
mod errors;
mod sessions;

const DEFAULT_WINDOW_MICROS: i64 = 24 * 60 * 60 * 1_000_000;
const DEFAULT_PAGE_SIZE: usize = 20;
pub(super) const MAX_PAGE_SIZE: usize = 100;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/rum/sessions", get(sessions::list))
        .route("/rum/errors", get(errors::list))
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(super) struct ListQuery {
    pub(super) from: Option<i64>,
    pub(super) to: Option<i64>,
    pub(super) q: Option<String>,
    pub(super) country: Option<String>,
    pub(super) browser: Option<String>,
    pub(super) replay_available: Option<bool>,
    pub(super) status: Option<String>,
    pub(super) limit: Option<usize>,
    pub(super) cursor: Option<String>,
}

pub(super) fn initial_page_context(request: &ListQuery) -> Result<(i64, i64, usize)> {
    let now = TimestampMicros::now().0;
    let from = request
        .from
        .unwrap_or_else(|| now.saturating_sub(DEFAULT_WINDOW_MICROS));
    let to = request.to.unwrap_or(now);
    if to <= from {
        return Err(Error::invalid("RUM range end must be greater than start"));
    }
    Ok((
        from,
        to,
        request
            .limit
            .unwrap_or(DEFAULT_PAGE_SIZE)
            .clamp(1, MAX_PAGE_SIZE),
    ))
}

pub(super) fn shared_cursor_mismatch(
    request: &ListQuery,
    from: i64,
    to: i64,
    page_size: usize,
) -> bool {
    request.from.is_some_and(|value| value != from)
        || request.to.is_some_and(|value| value != to)
        || request
            .limit
            .is_some_and(|value| value.clamp(1, MAX_PAGE_SIZE) != page_size)
}

pub(super) fn normalize_text(value: Option<String>, max: usize) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim();
        (!value.is_empty() && !value.contains('\0')).then(|| value.chars().take(max).collect())
    })
}
