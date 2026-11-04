// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `GET /rum/sessions/{id}/timeline`: everything that happened in one session, in the order it
//! happened, for the session page.

use axum::{
    Extension, Json,
    extract::{Path, Query, State},
};
use serde::Serialize;
use serde_json::{Map, Value};

use super::{
    entity::{EntityQuery, find, lookup_span},
    reader::{Reader, Row, Span, rows},
};
use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{iam::permission, rum::event::RumStream},
    infra::rum::query::{Condition, ListRequest, Scope, list_sql, timeline_columns},
    shared::{Result, time::TimestampMicros},
};

/// Rows are written a little before and after the activity the session reports.
const BEFORE_MICROS: i64 = 60 * 1_000_000;
const AFTER_MICROS: i64 = 5 * 60 * 1_000_000;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Timeline {
    pub session_id: String,
    /// The session itself, `None` while no row of it was derived yet.
    pub session: Option<Map<String, Value>>,
    /// The rows of every type, oldest first; each says its `type`.
    pub items: Vec<Map<String, Value>>,
    /// The types that had more rows than a timeline carries.
    pub truncated: Vec<&'static str>,
}

#[permission(any("rum.sessions.read", "sys.telemetry.read"))]
pub(super) async fn timeline(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Path(id): Path<String>,
    Query(query): Query<EntityQuery>,
) -> Result<Json<Timeline>> {
    read(&Reader::new(&state, &iam), &id, &query)
        .await
        .map(Json)
}

/// Everything one session did, as far as a timeline carries.
pub(super) async fn read(reader: &Reader<'_>, id: &str, query: &EntityQuery) -> Result<Timeline> {
    let lookup = lookup_span(query, TimestampMicros::now().0)?;

    let session = find(reader, RumStream::Sessions, id, lookup).await?;
    let span = session
        .as_ref()
        .map_or(lookup, |row| session_span(row, lookup));

    let loads = RumStream::ALL
        .into_iter()
        .filter(|stream| *stream != RumStream::Sessions)
        .map(|stream| load(reader, stream, id, span));

    let loaded = futures::future::try_join_all(loads).await?;

    let mut truncated = Vec::new();
    let mut ordered = Vec::new();

    for (stream, found, cut) in loaded {
        if cut {
            truncated.push(stream.collection());
        }

        ordered.extend(found.into_iter().map(|row| (rank(stream), stream, row)));
    }

    ordered.sort_by_key(|(rank, _, row)| (row.timestamp_micros, *rank));

    let items = ordered
        .into_iter()
        .map(|(_, stream, row)| {
            let mut found = row.into_item();

            if let Some(kind) = stream.event_type() {
                found.insert("type".into(), kind.as_str().into());
            }

            found
        })
        .collect();

    Ok(Timeline {
        session_id: id.to_string(),
        session: session.map(Row::into_item),
        items,
        truncated,
    })
}

/// Where the rows of a session lie: from its start to its last activity, with the margin rows are
/// stamped within. Without a row for the session, the whole range the page looked in.
fn session_span(session: &Row, lookup: Span) -> Span {
    let from = session.timestamp_micros.saturating_sub(BEFORE_MICROS);

    let to = session
        .fields
        .get("session.last_activity")
        .and_then(Value::as_i64)
        .map_or(lookup.to, |last_ms| {
            last_ms.saturating_mul(1000).saturating_add(AFTER_MICROS)
        });

    if to > from { Span { from, to } } else { lookup }
}

/// How many rows of a type a timeline carries.
fn cap(stream: RumStream) -> usize {
    match stream {
        RumStream::Resources => 3_000,
        RumStream::Actions => 2_000,
        RumStream::Errors | RumStream::LongTasks | RumStream::Logs => 1_000,
        RumStream::Views | RumStream::Vitals | RumStream::Sessions => 500,
    }
}

/// The order of rows of the same moment: a view begins before what happens in it.
fn rank(stream: RumStream) -> u8 {
    match stream {
        RumStream::Sessions => 0,
        RumStream::Views => 1,
        RumStream::Actions => 2,
        RumStream::Resources => 3,
        RumStream::LongTasks => 4,
        RumStream::Vitals => 5,
        RumStream::Errors => 6,
        RumStream::Logs => 7,
    }
}

async fn load(
    reader: &Reader<'_>,
    stream: RumStream,
    session_id: &str,
    span: Span,
) -> Result<(RumStream, Vec<Row>, bool)> {
    let conditions = [Condition::equals("session.id", session_id)];
    let fetch = cap(stream) + 1;

    let statement = list_sql(&ListRequest {
        scope: Scope {
            stream,
            from: span.from,
            to: span.to,
            conditions: &conditions,
        },
        columns: &timeline_columns(stream),
        limit: fetch,
        seek: None,
        oldest_first: true,
    });

    let Some(result) = reader.run(stream, span, statement, Some(fetch)).await? else {
        return Ok((stream, Vec::new(), false));
    };

    let mut found = rows(stream, result);
    let cut = found.len() > cap(stream);

    found.truncate(cap(stream));

    Ok((stream, found, cut))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn session(start: i64, last_activity_ms: Option<i64>) -> Row {
        let mut fields = Map::new();

        if let Some(last) = last_activity_ms {
            fields.insert("session.last_activity".into(), json!(last));
        }

        Row {
            timestamp_micros: start,
            tie: "s-1".into(),
            fields,
        }
    }

    #[test]
    fn rows_are_looked_for_from_the_start_of_the_session_to_its_last_activity() {
        let lookup = Span {
            from: 0,
            to: 10_000_000_000,
        };
        let span = session_span(&session(2_000_000_000, Some(2_300_000)), lookup);

        assert_eq!(span.from, 2_000_000_000 - BEFORE_MICROS);
        assert_eq!(span.to, 2_300_000_000 + AFTER_MICROS);
    }

    #[test]
    fn a_session_without_a_last_activity_is_looked_for_to_the_end_of_the_range() {
        let lookup = Span {
            from: 0,
            to: 10_000_000_000,
        };
        let span = session_span(&session(2_000_000_000, None), lookup);

        assert_eq!(span.to, 10_000_000_000);
    }

    #[test]
    fn a_session_that_ends_before_it_begins_falls_back_to_the_range_looked_in() {
        let lookup = Span { from: 5, to: 50 };

        assert_eq!(
            session_span(&session(2_000_000_000, Some(1)), lookup),
            lookup
        );
    }

    #[test]
    fn what_begins_at_the_same_moment_is_told_in_the_order_it_happens() {
        let mut streams = [
            RumStream::Errors,
            RumStream::Actions,
            RumStream::Views,
            RumStream::Resources,
        ];

        streams.sort_by_key(|stream| rank(*stream));

        assert_eq!(
            streams,
            [
                RumStream::Views,
                RumStream::Actions,
                RumStream::Resources,
                RumStream::Errors
            ]
        );
    }

    #[test]
    fn every_type_of_row_has_a_cap_and_the_most_numerous_have_the_largest() {
        for stream in RumStream::ALL {
            assert!(cap(stream) > 0);
        }

        assert!(cap(RumStream::Resources) > cap(RumStream::Actions));
        assert!(cap(RumStream::Actions) > cap(RumStream::Views));
    }
}
