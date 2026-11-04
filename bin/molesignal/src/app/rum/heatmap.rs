// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Where people click on a page and how far down they go, and a recorded page to draw that over.
//!
//! The recording is the replay of a session that opened the page, as it stood when the page had
//! loaded: what is masked in the replay is masked here too.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    analytics::Answer,
    reader::{Reader, Span, within_budget},
    selection::Selection,
};
use crate::{
    domain::rum::event::RumStream,
    infra::rum::{
        query::{
            ClickMap, Condition, Filter, ListRequest, Scope, ScrollMap, clicks_sql, list_sql,
            read_clicks, read_scroll, rows, scroll_sql,
        },
        replay::RumReplayWriter,
    },
    shared::{Error, Result, ids::Id},
};

/// How many elements a map tells apart unless the question says.
const DEFAULT_ELEMENTS: usize = 100;
const MAX_ELEMENTS: usize = 300;

/// How many views with a recording are offered to draw a map over.
const CANDIDATES: usize = 20;

/// A page that has settled by then is taken as loaded, if the view does not say.
const SETTLED_NANOS: i64 = 3_000_000_000;

/// What a map is of: the rows of a page in a range.
#[derive(Debug, Default, Deserialize)]
pub struct MapRequest {
    pub from: Option<i64>,
    pub to: Option<i64>,
    /// The application, the environment, the page, the size of the window…
    #[serde(default)]
    pub filters: Vec<Filter>,
    pub limit: Option<usize>,
}

fn selection_of(request: &MapRequest, stream: RumStream) -> Result<Selection> {
    Selection::new(
        Some(stream.collection()),
        request.from,
        request.to,
        request.filters.clone(),
        Vec::new(),
    )
}

/// A click, of the clicks and taps a person made.
fn clicks_only() -> Condition {
    Condition::raw(
        "\"action.type\" IN ('click', 'tap')",
        &["action.type"],
        false,
    )
}

pub async fn clicks(reader: &Reader<'_>, request: &MapRequest) -> Result<Answer<ClickMap>> {
    let selection = selection_of(request, RumStream::Actions)?;
    let span = selection.span;
    let limit = request
        .limit
        .unwrap_or(DEFAULT_ELEMENTS)
        .clamp(1, MAX_ELEMENTS);

    let mut conditions = selection.conditions()?;

    conditions.push(clicks_only());

    let scope = Scope {
        stream: RumStream::Actions,
        from: span.from,
        to: span.to,
        conditions: &conditions,
    };

    let (elements, cells) = clicks_sql(&scope, limit);

    let answered = within_budget(async {
        let Some(elements) = reader
            .run_tracked(RumStream::Actions, span, elements)
            .await?
        else {
            return Ok(None);
        };

        let cells = reader.run_tracked(RumStream::Actions, span, cells).await?;

        Ok(Some((elements, cells)))
    })
    .await?;

    let (body, unavailable) = match answered {
        Ok(Some((elements, cells))) => {
            let (columns, rows) = cells
                .map(|cells| (cells.columns, cells.rows))
                .unwrap_or_default();

            (
                read_clicks((&elements.columns, &elements.rows), (&columns, &rows))?,
                None,
            )
        }
        Ok(None) => (ClickMap::default(), None),
        Err(reason) => (ClickMap::default(), Some(reason)),
    };

    Ok(Answer {
        body,
        from: span.from,
        to: span.to,
        unavailable,
    })
}

pub async fn scroll(reader: &Reader<'_>, request: &MapRequest) -> Result<Answer<ScrollMap>> {
    let selection = selection_of(request, RumStream::Views)?;
    let span = selection.span;
    let conditions = selection.conditions()?;

    let statement = scroll_sql(&Scope {
        stream: RumStream::Views,
        from: span.from,
        to: span.to,
        conditions: &conditions,
    });

    let answered = within_budget(reader.run_tracked(RumStream::Views, span, statement)).await?;

    let (body, unavailable) = match answered {
        Ok(Some(result)) => (read_scroll(&result.columns, &result.rows)?, None),
        Ok(None) => (ScrollMap::default(), None),
        Err(reason) => (ScrollMap::default(), Some(reason)),
    };

    Ok(Answer {
        body,
        from: span.from,
        to: span.to,
        unavailable,
    })
}

/// A view a map can be drawn over.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub session_id: String,
    pub view_id: String,
    /// The moment of the recording that shows the page, loaded.
    pub at_time: i64,
    pub width: Option<f64>,
    pub height: Option<f64>,
}

/// A recorded page: the events that build it, up to the moment it shows.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub session_id: String,
    pub view_id: String,
    pub at_time: i64,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub events: Vec<Value>,
}

#[derive(Debug, Serialize)]
pub struct SnapshotAnswer {
    /// None when no session that opened the page was recorded.
    pub snapshot: Option<Snapshot>,
    /// The others to draw it over, the newest first.
    pub candidates: Vec<Candidate>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotRequest {
    pub from: Option<i64>,
    pub to: Option<i64>,
    #[serde(default)]
    pub filters: Vec<Filter>,
    /// The view to draw it over, one of the candidates; the newest when not said.
    pub view_id: Option<String>,
}

const CANDIDATE_COLUMNS: [&str; 6] = [
    "session.id",
    "view.id",
    "view.loading_time",
    "view.time_spent",
    "display.viewport.width",
    "display.viewport.height",
];

fn candidate_of(row: crate::infra::rum::query::Row) -> Option<Candidate> {
    let text = |name: &str| {
        row.fields
            .get(name)
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let number = |name: &str| row.fields.get(name).and_then(Value::as_f64);

    let spent = number("view.time_spent").map(|nanos| nanos as i64);
    let loaded = number("view.loading_time")
        .map(|nanos| nanos as i64)
        .unwrap_or(SETTLED_NANOS);

    let after = spent.map_or(loaded, |spent| loaded.min(spent));

    Some(Candidate {
        session_id: text("session.id")?,
        view_id: text("view.id")?,
        at_time: row.timestamp_micros + after / 1_000,
        width: number("display.viewport.width"),
        height: number("display.viewport.height"),
    })
}

/// The views of a page that were recorded, the newest first.
pub async fn candidates(
    reader: &Reader<'_>,
    from: Option<i64>,
    to: Option<i64>,
    filters: Vec<Filter>,
) -> Result<Vec<Candidate>> {
    let selection = Selection::new(
        Some(RumStream::Views.collection()),
        from,
        to,
        filters,
        Vec::new(),
    )?;

    let Span { from, to } = selection.span;
    let mut conditions = selection.conditions()?;

    conditions.push(Condition::raw(
        "\"session.has_replay\" = TRUE",
        &["session.has_replay"],
        false,
    ));

    let statement = list_sql(&ListRequest {
        scope: Scope {
            stream: RumStream::Views,
            from,
            to,
            conditions: &conditions,
        },
        columns: &CANDIDATE_COLUMNS,
        limit: CANDIDATES,
        seek: None,
        oldest_first: false,
    });

    let Some(result) = reader
        .run(
            RumStream::Views,
            selection.span,
            statement,
            Some(CANDIDATES),
        )
        .await?
    else {
        return Ok(Vec::new());
    };

    Ok(rows(RumStream::Views, result)
        .into_iter()
        .filter_map(candidate_of)
        .collect())
}

/// A recorded page for a map: of the view asked for, or else of the newest that can show one.
pub async fn snapshot(
    reader: &Reader<'_>,
    replay: &RumReplayWriter,
    org_id: &Id,
    request: SnapshotRequest,
) -> Result<SnapshotAnswer> {
    let found = candidates(reader, request.from, request.to, request.filters).await?;

    let asked = match request.view_id.as_deref().map(str::trim) {
        Some(view) if !view.is_empty() => {
            let candidate = found
                .iter()
                .find(|candidate| candidate.view_id == view)
                .ok_or_else(|| Error::not_found("a recorded view of this page"))?;

            vec![candidate.clone()]
        }
        _ => found.clone(),
    };

    for candidate in asked {
        let Some((at_micros, events)) = replay
            .page_at(org_id, &candidate.session_id, candidate.at_time)
            .await?
        else {
            continue;
        };

        return Ok(SnapshotAnswer {
            snapshot: Some(Snapshot {
                session_id: candidate.session_id,
                view_id: candidate.view_id,
                at_time: at_micros,
                width: candidate.width,
                height: candidate.height,
                events,
            }),
            candidates: found,
        });
    }

    Ok(SnapshotAnswer {
        snapshot: None,
        candidates: found,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, json};

    use super::*;
    use crate::infra::rum::query::Row;

    fn row(fields: Value) -> Row {
        Row {
            timestamp_micros: 1_000_000,
            tie: "v".into(),
            fields: fields.as_object().cloned().unwrap_or_else(Map::new),
        }
    }

    #[test]
    fn a_page_is_shown_when_it_had_loaded_and_not_after_the_view_ended() {
        let loaded = candidate_of(row(json!({
            "session.id": "s",
            "view.id": "v",
            "view.loading_time": 2_000_000_000_i64,
            "view.time_spent": 9_000_000_000_i64,
        })))
        .unwrap();

        assert_eq!(loaded.at_time, 3_000_000);

        let left = candidate_of(row(json!({
            "session.id": "s",
            "view.id": "v",
            "view.time_spent": 1_000_000_000_i64,
        })))
        .unwrap();

        assert_eq!(left.at_time, 2_000_000);
    }

    #[test]
    fn a_row_that_names_no_view_is_no_candidate() {
        assert!(candidate_of(row(json!({"session.id": "s"}))).is_none());
    }
}
