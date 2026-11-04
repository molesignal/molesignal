// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The rows of one request, sorted into the streams they are written to.

use std::collections::HashMap;

use serde_json::Value;

use crate::domain::{intake::RawEvent, rum::event::RumStream};

/// One event of the request, as the row of its stream.
pub struct Row {
    /// Where the event stood in the request, to name it if the stream refuses it.
    pub index: usize,
    pub stream: RumStream,
    pub event: RawEvent,
}

/// What is written to one stream, each event with its place in the request.
pub struct StreamRows {
    pub stream: RumStream,
    pub rows: Vec<(usize, RawEvent)>,
}

/// Sorts rows into streams, in the order they are best written.
///
/// A view is reported again as it changes, and a request may carry several revisions of one view:
/// only the highest is kept. Each view that is kept is also sent to `rum_sessions`, as the patch
/// the stream's owner merges into the session the view belongs to.
pub fn assemble(rows: Vec<Row>) -> Vec<StreamRows> {
    let mut by_stream: HashMap<RumStream, Vec<(usize, RawEvent)>> = HashMap::new();

    for row in rows {
        by_stream
            .entry(row.stream)
            .or_default()
            .push((row.index, row.event));
    }

    if let Some(views) = by_stream.remove(&RumStream::Views) {
        let views = latest_revisions(views);

        by_stream.insert(RumStream::Sessions, views.clone());
        by_stream.insert(RumStream::Views, views);
    }

    // Sessions first: a patch written twice because the request is sent again merges to the same
    // session, while a view without its session would not.
    RumStream::ALL
        .into_iter()
        .filter_map(|stream| {
            by_stream
                .remove(&stream)
                .filter(|rows| !rows.is_empty())
                .map(|rows| StreamRows { stream, rows })
        })
        .collect()
}

/// One row per view: the one with the highest revision, the later of two with the same.
fn latest_revisions(views: Vec<(usize, RawEvent)>) -> Vec<(usize, RawEvent)> {
    let mut kept: Vec<(usize, RawEvent)> = Vec::with_capacity(views.len());
    let mut position: HashMap<String, usize> = HashMap::new();

    for (index, event) in views {
        let id = event
            .fields
            .get("view.id")
            .and_then(Value::as_str)
            .map(str::to_string);

        let Some(id) = id else {
            kept.push((index, event));
            continue;
        };

        match position.get(&id).copied() {
            Some(at) if revision(&kept[at].1) > revision(&event) => {}
            Some(at) => kept[at] = (index, event),
            None => {
                position.insert(id, kept.len());
                kept.push((index, event));
            }
        }
    }

    kept
}

fn revision(event: &RawEvent) -> i64 {
    event
        .fields
        .get("view.revision")
        .and_then(Value::as_i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::shared::time::TimestampMicros;

    fn view(index: usize, id: &str, revision: i64) -> Row {
        Row {
            index,
            stream: RumStream::Views,
            event: RawEvent {
                timestamp: TimestampMicros(1_000),
                fields: json!({"view.id": id, "view.revision": revision, "session.id": "s"})
                    .as_object()
                    .unwrap()
                    .clone(),
            },
        }
    }

    fn other(index: usize, stream: RumStream) -> Row {
        Row {
            index,
            stream,
            event: RawEvent {
                timestamp: TimestampMicros(2_000),
                fields: json!({"session.id": "s"}).as_object().unwrap().clone(),
            },
        }
    }

    fn indexes(assembled: &[StreamRows], stream: RumStream) -> Vec<usize> {
        assembled
            .iter()
            .find(|rows| rows.stream == stream)
            .map(|rows| rows.rows.iter().map(|(index, _)| *index).collect())
            .unwrap_or_default()
    }

    #[test]
    fn only_the_highest_revision_of_a_view_is_kept() {
        let assembled = assemble(vec![
            view(0, "a", 1),
            view(1, "b", 1),
            view(2, "a", 3),
            view(3, "a", 2),
            view(4, "b", 1),
        ]);

        assert_eq!(indexes(&assembled, RumStream::Views), vec![2, 4]);
    }

    #[test]
    fn of_two_rows_with_the_same_revision_the_later_one_stays() {
        let assembled = assemble(vec![view(0, "a", 2), view(1, "a", 2)]);

        assert_eq!(indexes(&assembled, RumStream::Views), vec![1]);
    }

    #[test]
    fn every_kept_view_is_also_a_session_patch() {
        let assembled = assemble(vec![view(0, "a", 1), view(1, "a", 2)]);

        assert_eq!(indexes(&assembled, RumStream::Sessions), vec![1]);
        assert_eq!(indexes(&assembled, RumStream::Views), vec![1]);
    }

    #[test]
    fn other_events_are_kept_whole_and_streams_without_rows_are_left_out() {
        let assembled = assemble(vec![
            other(0, RumStream::Resources),
            other(1, RumStream::Resources),
            other(2, RumStream::Actions),
        ]);

        let streams = assembled.iter().map(|rows| rows.stream).collect::<Vec<_>>();

        assert_eq!(streams, vec![RumStream::Actions, RumStream::Resources]);
        assert_eq!(indexes(&assembled, RumStream::Resources), vec![0, 1]);
    }

    #[test]
    fn sessions_are_written_before_the_views_they_come_from() {
        let assembled = assemble(vec![other(0, RumStream::Errors), view(1, "a", 1)]);

        let streams = assembled.iter().map(|rows| rows.stream).collect::<Vec<_>>();

        assert_eq!(
            streams,
            vec![RumStream::Sessions, RumStream::Views, RumStream::Errors]
        );
    }

    #[test]
    fn a_view_without_an_id_is_not_merged_with_anything() {
        let mut first = view(0, "a", 1);
        let mut second = view(1, "a", 1);

        first.event.fields.remove("view.id");
        second.event.fields.remove("view.id");

        let assembled = assemble(vec![first, second]);

        assert_eq!(indexes(&assembled, RumStream::Views), vec![0, 1]);
    }

    #[test]
    fn nothing_in_nothing_out() {
        assert!(assemble(Vec::new()).is_empty());
    }
}
