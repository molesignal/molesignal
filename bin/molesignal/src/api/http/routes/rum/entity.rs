// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `GET /rum/{type}/{id}` (`/rum/views/{id}`, `/rum/errors/{id}`…): one row of the explorer with
//! all its columns, for the drawer that opens from it. A session or a view is read at its latest
//! revision.

use axum::{
    Extension, Json,
    extract::{Path, Query, State},
};
use serde::Deserialize;
use serde_json::{Map, Value};

use super::reader::{Reader, Row, Span, rows};
use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{iam::permission, rum::event::RumStream},
    infra::rum::query::{Condition, ListRequest, Scope, all_columns, list_sql},
    shared::{Error, Result, time::TimestampMicros},
};

/// A row is looked for over the last month unless the page says when it happened.
const DEFAULT_LOOKBACK_MICROS: i64 = 30 * 24 * 60 * 60 * 1_000_000;

/// A row may be stamped a little ahead of the clock that asks for it.
const CLOCK_GRACE_MICROS: i64 = 60 * 1_000_000;

#[derive(Debug, Default, Deserialize)]
pub(super) struct EntityQuery {
    /// When the row happened, if the page knows: the lookup then reads that range only.
    from: Option<i64>,
    to: Option<i64>,
}

/// The stream is the one the route was registered for.
#[permission(any("rum.events.read", "sys.telemetry.read"))]
pub(super) async fn entity(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Extension(stream): Extension<RumStream>,
    Path(id): Path<String>,
    Query(query): Query<EntityQuery>,
) -> Result<Json<Map<String, Value>>> {
    let span = lookup_span(&query, TimestampMicros::now().0)?;

    find(&Reader::new(&state, &iam), stream, &id, span)
        .await?
        .map(|row| Json(row.into_item()))
        .ok_or_else(|| Error::not_found("RUM row"))
}

/// The row of a stream that has this id, with every column.
pub(super) async fn find(
    reader: &Reader<'_>,
    stream: RumStream,
    id: &str,
    span: Span,
) -> Result<Option<Row>> {
    let id = id.trim();

    if id.is_empty() || id.len() > 256 {
        return Err(Error::invalid(
            "a RUM row is named by an id of 1 to 256 characters",
        ));
    }

    let conditions = [Condition::equals(stream.id_field(), id)];

    let statement = list_sql(&ListRequest {
        scope: Scope {
            stream,
            from: span.from,
            to: span.to,
            conditions: &conditions,
        },
        columns: &all_columns(stream),
        limit: 1,
        seek: None,
        oldest_first: false,
    });

    let Some(result) = reader.run(stream, span, statement, Some(1)).await? else {
        return Ok(None);
    };

    Ok(rows(stream, result).into_iter().next())
}

pub(super) fn lookup_span(query: &EntityQuery, now: i64) -> Result<Span> {
    let to = query.to.unwrap_or(now.saturating_add(CLOCK_GRACE_MICROS));
    let from = query
        .from
        .unwrap_or_else(|| to.saturating_sub(DEFAULT_LOOKBACK_MICROS));

    if to <= from {
        return Err(Error::invalid("range end must be greater than start"));
    }

    Ok(Span { from, to })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_is_looked_for_over_the_last_month_when_the_page_does_not_know_when() {
        let span = lookup_span(&EntityQuery::default(), 100 * DEFAULT_LOOKBACK_MICROS).unwrap();

        assert_eq!(span.to, 100 * DEFAULT_LOOKBACK_MICROS + CLOCK_GRACE_MICROS);
        assert_eq!(span.to - span.from, DEFAULT_LOOKBACK_MICROS);
    }

    #[test]
    fn a_range_the_page_gives_is_the_range_read() {
        let query = EntityQuery {
            from: Some(10),
            to: Some(20),
        };

        assert_eq!(lookup_span(&query, 0).unwrap(), Span { from: 10, to: 20 });
        assert!(
            lookup_span(
                &EntityQuery {
                    from: Some(20),
                    to: Some(10)
                },
                0
            )
            .is_err()
        );
    }
}
