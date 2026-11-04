// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::atomic::{AtomicUsize, Ordering};

use parking_lot::Mutex as SyncMutex;
use serde_json::json;

use super::*;
use crate::shared::Error;

const NOW: i64 = 1_760_000_000_000;

#[derive(Default)]
struct History {
    rows: SyncMutex<Vec<Map<String, Value>>>,
    asked: SyncMutex<Vec<(String, i64, i64)>>,
    calls: AtomicUsize,
    fail: bool,
}

#[async_trait]
impl SessionHistory for History {
    async fn rows(
        &self,
        _org_id: &Id,
        session_id: &str,
        from_ms: i64,
        to_ms: i64,
    ) -> Result<Vec<Map<String, Value>>> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.asked
            .lock()
            .push((session_id.to_string(), from_ms, to_ms));

        if self.fail {
            return Err(Error::unavailable("storage is down"));
        }

        Ok(self.rows.lock().clone())
    }
}

fn org() -> Id {
    Id::from_string("org-1")
}

fn patch(session: &str, view: &str, revision: i64, start_ms: i64, extra: Value) -> RawEvent {
    let mut fields = json!({
        "session.id": session,
        "view.id": view,
        "view.revision": revision,
        "view.time_spent": 1_000_000_000_i64,
        "view.url_path": format!("/{view}"),
        "view.action.count": 1,
        "application.id": "shop-web",
    })
    .as_object()
    .unwrap()
    .clone();

    fields.extend(extra.as_object().unwrap().clone());

    RawEvent {
        timestamp: TimestampMicros(start_ms * 1000),
        fields,
    }
}

fn merger(history: Arc<History>) -> SessionMerger {
    SessionMerger::new(history)
}

#[tokio::test]
async fn a_new_session_is_built_from_its_views_without_reading_anything() {
    let history = Arc::new(History::default());
    let merger = merger(history.clone());

    let rows = merger
        .merge_at(
            &org(),
            vec![
                patch("s-1", "home", 1, NOW - 4_000, json!({})),
                patch(
                    "s-1",
                    "cart",
                    1,
                    NOW - 2_000,
                    json!({"view.action.count": 3}),
                ),
            ],
            NOW,
        )
        .await
        .unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(history.calls.load(Ordering::Relaxed), 0);

    let row = &rows[0];

    assert_eq!(row.timestamp, TimestampMicros((NOW - 4_000) * 1000));
    assert_eq!(row.fields["session.id"], "s-1");
    assert_eq!(row.fields["session.view.count"], 2);
    assert_eq!(row.fields["session.action.count"], 4);
    assert_eq!(row.fields["application.id"], "shop-web");
    assert!(row.fields["session.revision"].as_i64().unwrap() >= NOW);
}

#[tokio::test]
async fn each_batch_makes_a_row_with_a_higher_revision_and_what_came_before() {
    let merger = merger(Arc::new(History::default()));

    let first = merger
        .merge_at(
            &org(),
            vec![patch("s-1", "home", 1, NOW - 4_000, json!({}))],
            NOW,
        )
        .await
        .unwrap();

    let second = merger
        .merge_at(
            &org(),
            vec![patch("s-1", "cart", 1, NOW - 1_000, json!({}))],
            NOW + 1,
        )
        .await
        .unwrap();

    let revision = |rows: &[RawEvent]| rows[0].fields["session.revision"].as_i64().unwrap();

    assert!(revision(&second) > revision(&first));
    assert_eq!(second[0].fields["session.view.count"], 2);
    assert_eq!(second[0].fields["session.landing_view.url_path"], "/home");
    assert_eq!(second[0].fields["session.exit_view.url_path"], "/cart");
}

#[tokio::test]
async fn sessions_of_one_batch_come_out_in_the_order_they_were_first_seen() {
    let merger = merger(Arc::new(History::default()));

    let rows = merger
        .merge_at(
            &org(),
            vec![
                patch("b", "v", 1, NOW - 1_000, json!({})),
                patch("a", "v", 1, NOW - 1_000, json!({})),
                patch("b", "w", 1, NOW - 500, json!({})),
            ],
            NOW,
        )
        .await
        .unwrap();

    let ids = rows
        .iter()
        .map(|row| row.fields["session.id"].as_str().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(ids, vec!["b", "a"]);
    assert_eq!(rows[0].fields["session.view.count"], 2);
}

#[tokio::test]
async fn a_patch_without_a_session_is_dropped() {
    let merger = merger(Arc::new(History::default()));
    let mut loose = patch("s-1", "home", 1, NOW, json!({}));

    loose.fields.remove("session.id");

    assert!(
        merger
            .merge_at(&org(), vec![loose], NOW)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn an_old_session_the_node_does_not_know_is_rebuilt_from_what_was_written() {
    let history = Arc::new(History::default());

    // What another node wrote earlier: the home view, seen at revision 2, and the cart view.
    let mut earlier = SessionState::new();

    earlier.apply(&patch(
        "s-1",
        "home",
        2,
        NOW - 600_000,
        json!({"view.action.count": 5}),
    ));
    earlier.apply(&patch(
        "s-1",
        "cart",
        1,
        NOW - 590_000,
        json!({"view.action.count": 1}),
    ));
    earlier.revision = NOW - 100_000;

    history.rows.lock().push(earlier.row("s-1", NOW).fields);

    let merger = merger(history.clone());

    let rows = merger
        .merge_at(
            &org(),
            vec![patch(
                "s-1",
                "cart",
                2,
                NOW - 590_000,
                json!({"view.action.count": 4}),
            )],
            NOW,
        )
        .await
        .unwrap();

    assert_eq!(history.calls.load(Ordering::Relaxed), 1);

    let (_, from, to) = history.asked.lock()[0].clone();

    assert!(from <= NOW - 600_000 && to > NOW);

    let row = &rows[0].fields;

    assert_eq!(row["session.view.count"], 2);
    assert_eq!(row["session.action.count"], 9);
    assert_eq!(row["session.landing_view.url_path"], "/home");
    assert!(row["session.revision"].as_i64().unwrap() > NOW - 100_000);
}

#[tokio::test]
async fn the_start_the_sdk_gives_narrows_where_history_is_looked_for() {
    let history = Arc::new(History::default());
    let merger = merger(history.clone());
    let start = NOW - 900_000;

    merger
        .merge_at(
            &org(),
            vec![patch(
                "s-1",
                "home",
                3,
                start + 1_000,
                json!({"session.start": start}),
            )],
            NOW,
        )
        .await
        .unwrap();

    let (_, from, _) = history.asked.lock()[0].clone();

    assert_eq!(from, start - 60_000);
}

#[tokio::test]
async fn a_session_is_read_back_only_once() {
    let history = Arc::new(History::default());
    let merger = merger(history.clone());

    for revision in 1..=3 {
        merger
            .merge_at(
                &org(),
                vec![patch("s-1", "home", revision, NOW - 600_000, json!({}))],
                NOW + revision,
            )
            .await
            .unwrap();
    }

    assert_eq!(history.calls.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn unreadable_history_starts_the_session_afresh_instead_of_failing() {
    let history = Arc::new(History {
        fail: true,
        ..History::default()
    });

    let rows = merger(history.clone())
        .merge_at(
            &org(),
            vec![patch("s-1", "home", 1, NOW - 600_000, json!({}))],
            NOW,
        )
        .await
        .unwrap();

    assert_eq!(history.calls.load(Ordering::Relaxed), 1);
    assert_eq!(rows[0].fields["session.view.count"], 1);
}

#[tokio::test]
async fn two_batches_of_one_session_at_once_both_end_up_in_it() {
    let merger = merger(Arc::new(History::default()));
    let org = org();

    let (first, second) = tokio::join!(
        merger.merge_at(
            &org,
            vec![patch("s-1", "home", 1, NOW - 3_000, json!({}))],
            NOW
        ),
        merger.merge_at(
            &org,
            vec![patch("s-1", "cart", 1, NOW - 2_000, json!({}))],
            NOW
        ),
    );

    let counts = [first.unwrap(), second.unwrap()]
        .iter()
        .map(|rows| rows[0].fields["session.view.count"].as_i64().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(counts.iter().max(), Some(&2));
}

#[test]
fn a_session_that_began_moments_ago_is_not_looked_up() {
    let recent = [patch("s", "v", 1, NOW - 29_000, json!({}))];
    let old = [patch("s", "v", 1, NOW - 31_000, json!({}))];

    assert_eq!(lookup_window(&recent, NOW), None);
    assert!(lookup_window(&old, NOW).is_some());
    assert_eq!(lookup_window(&[], NOW), None);
}
