// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What the server knows of one session: the latest report of each of its views.
//!
//! A session is never reported by an SDK. Each view row says how many actions, errors and the like
//! its view has had so far, so the session is what the latest revisions of its views add up to.
//! The state is carried from one session row to the next in `session.view_state`, which is also
//! what a node that has lost the session rebuilds it from.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{domain::intake::RawEvent, shared::time::TimestampMicros};

/// Views tracked one by one. A session that goes on beyond this folds its oldest views into
/// totals.
const MAX_TRACKED_VIEWS: usize = 500;

/// The columns of a session that describe who and what it was, taken from the latest view that
/// carries them.
const DIMENSIONS: &[&str] = &[
    "application.id",
    "service",
    "env",
    "version",
    "source",
    "synthetics.test_id",
    "synthetics.result_id",
    "synthetics.location_id",
    "ci_test.test_execution_id",
    "usr.id",
    "usr.name",
    "usr.email",
    "usr.anonymous_id",
    "usr.extra",
    "account.id",
    "browser.name",
    "browser.version",
    "os.name",
    "os.version",
    "device.type",
    "device.model",
    "device.brand",
    "geo.country_iso_code",
    "geo.country",
    "geo.country_subdivision",
    "geo.city",
    "geo.continent_code",
    "network.client.ip",
    "sdk.name",
    "sdk.version",
    "retention.rate",
    "retention.filter",
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Counts {
    #[serde(rename = "a", default)]
    pub actions: i64,
    #[serde(rename = "e", default)]
    pub errors: i64,
    #[serde(rename = "r", default)]
    pub resources: i64,
    #[serde(rename = "l", default)]
    pub long_tasks: i64,
    #[serde(rename = "f", default)]
    pub frustrations: i64,
    #[serde(rename = "c", default)]
    pub crashes: i64,
}

impl Counts {
    fn add(&mut self, other: &Counts) {
        self.actions += other.actions;
        self.errors += other.errors;
        self.resources += other.resources;
        self.long_tasks += other.long_tasks;
        self.frustrations += other.frustrations;
        self.crashes += other.crashes;
    }

    fn from_view(fields: &Map<String, Value>) -> Self {
        let count = |name: &str| fields.get(name).and_then(Value::as_i64).unwrap_or_default();

        Self {
            actions: count("view.action.count"),
            errors: count("view.error.count"),
            resources: count("view.resource.count"),
            long_tasks: count("view.long_task.count"),
            frustrations: count("view.frustration.count"),
            crashes: count("view.crash.count"),
        }
    }
}

/// How a view is told apart on a session's landing and exit.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Place {
    #[serde(rename = "u", default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(rename = "p", default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(rename = "n", default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ViewEntry {
    #[serde(rename = "i")]
    id: String,
    #[serde(rename = "v")]
    revision: i64,
    /// When the view began, in epoch milliseconds.
    #[serde(rename = "s")]
    start_ms: i64,
    /// How long the view had lasted at this revision, in nanoseconds.
    #[serde(rename = "t", default)]
    spent_ns: i64,
    #[serde(flatten)]
    place: Place,
    #[serde(flatten)]
    counts: Counts,
}

impl ViewEntry {
    fn last_activity_ms(&self) -> i64 {
        self.start_ms.saturating_add(self.spent_ns / 1_000_000)
    }
}

/// Views that were folded into totals to keep the state small.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Folded {
    #[serde(rename = "n", default)]
    views: i64,
    #[serde(rename = "t", default)]
    last_activity_ms: i64,
    #[serde(flatten)]
    counts: Counts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Landing {
    #[serde(rename = "s")]
    start_ms: i64,
    #[serde(flatten)]
    place: Place,
}

/// The form `session.view_state` is stored in.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Stored {
    #[serde(default)]
    views: Vec<ViewEntry>,
    #[serde(default)]
    folded: Folded,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    landing: Option<Landing>,
}

#[derive(Debug, Clone)]
pub(super) struct SessionState {
    pub revision: i64,
    start_ms: Option<i64>,
    kind: Option<String>,
    has_replay: bool,
    views: BTreeMap<String, ViewEntry>,
    folded: Folded,
    landing: Option<Landing>,
    dims: Map<String, Value>,
}

impl SessionState {
    pub fn new() -> Self {
        Self {
            revision: 0,
            start_ms: None,
            kind: None,
            has_replay: false,
            views: BTreeMap::new(),
            folded: Folded::default(),
            landing: None,
            dims: Map::new(),
        }
    }

    /// The state the rows written for a session so far add up to, newest row first. The rows may
    /// come from several nodes and one of them may know less than another: every view keeps its
    /// highest revision wherever it was seen.
    pub fn from_rows(rows: &[Map<String, Value>]) -> Self {
        let mut state = Self::new();

        for row in rows.iter().rev() {
            state.absorb_row(row);
        }

        state
    }

    fn absorb_row(&mut self, row: &Map<String, Value>) {
        let revision = row
            .get("session.revision")
            .and_then(Value::as_i64)
            .unwrap_or_default();

        // A JSON column comes back from a query as text.
        let stored = row
            .get("session.view_state")
            .and_then(|value| match value {
                Value::String(text) => serde_json::from_str::<Stored>(text).ok(),
                other => Stored::deserialize(other).ok(),
            })
            .unwrap_or_default();

        for entry in stored.views {
            self.keep(entry);
        }

        if stored.folded.views > self.folded.views {
            self.folded = stored.folded;
        }

        if let Some(landing) = stored.landing {
            self.land(landing);
        }

        if revision >= self.revision {
            self.revision = revision;
            self.take_dimensions(row);

            if let Some(kind) = row.get("session.type").and_then(Value::as_str) {
                self.kind = Some(kind.to_string());
            }
        }

        self.has_replay |= row.get("session.has_replay").and_then(Value::as_bool) == Some(true);
    }

    /// Takes in one view row (a patch). A report older than the one already held is ignored.
    pub fn apply(&mut self, patch: &RawEvent) {
        let fields = &patch.fields;

        let Some(id) = fields.get("view.id").and_then(Value::as_str) else {
            return;
        };

        let revision = fields
            .get("view.revision")
            .and_then(Value::as_i64)
            .unwrap_or(1);

        if self
            .views
            .get(id)
            .is_some_and(|held| held.revision > revision)
        {
            return;
        }

        let text = |name: &str| fields.get(name).and_then(Value::as_str).map(str::to_string);

        let entry = ViewEntry {
            id: id.to_string(),
            revision,
            start_ms: patch.timestamp.0 / 1000,
            spent_ns: fields
                .get("view.time_spent")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            place: Place {
                url: text("view.url"),
                path: text("view.url_path"),
                name: text("view.name"),
            },
            counts: Counts::from_view(fields),
        };

        self.land(Landing {
            start_ms: entry.start_ms,
            place: entry.place.clone(),
        });

        self.keep(entry);
        self.take_dimensions(fields);

        if let Some(start) = fields.get("session.start").and_then(Value::as_i64) {
            self.start_ms = Some(self.start_ms.map_or(start, |held| held.min(start)));
        }

        if let Some(kind) = fields.get("session.type").and_then(Value::as_str) {
            self.kind = Some(kind.to_string());
        }

        self.has_replay |= fields.get("session.has_replay").and_then(Value::as_bool) == Some(true);
    }

    fn keep(&mut self, entry: ViewEntry) {
        if self
            .views
            .get(&entry.id)
            .is_some_and(|held| held.revision >= entry.revision)
        {
            return;
        }

        self.start_ms = Some(
            self.start_ms
                .map_or(entry.start_ms, |held| held.min(entry.start_ms)),
        );

        self.views.insert(entry.id.clone(), entry);
        self.fold_oldest();
    }

    fn land(&mut self, landing: Landing) {
        if self
            .landing
            .as_ref()
            .is_none_or(|held| landing.start_ms < held.start_ms)
        {
            self.landing = Some(landing);
        }
    }

    fn take_dimensions(&mut self, source: &Map<String, Value>) {
        for name in DIMENSIONS {
            if let Some(value) = source.get(*name).filter(|value| !value.is_null()) {
                self.dims.insert((*name).to_string(), value.clone());
            }
        }
    }

    fn fold_oldest(&mut self) {
        while self.views.len() > MAX_TRACKED_VIEWS {
            let Some(oldest) = self
                .views
                .values()
                .min_by_key(|entry| entry.start_ms)
                .map(|entry| entry.id.clone())
            else {
                return;
            };

            if let Some(entry) = self.views.remove(&oldest) {
                self.folded.views += 1;
                self.folded.counts.add(&entry.counts);
                self.folded.last_activity_ms =
                    self.folded.last_activity_ms.max(entry.last_activity_ms());
            }
        }
    }

    /// The row to write for the session, with the time it is written at.
    pub fn row(&self, session_id: &str, now_ms: i64) -> RawEvent {
        let start_ms = self.start_ms.unwrap_or(now_ms);

        let last_activity_ms = self
            .views
            .values()
            .map(ViewEntry::last_activity_ms)
            .chain([self.folded.last_activity_ms, start_ms])
            .max()
            .unwrap_or(start_ms);

        let mut counts = self.folded.counts;

        for entry in self.views.values() {
            counts.add(&entry.counts);
        }

        let mut row = self.dims.clone();

        row.insert("session.id".into(), Value::from(session_id));
        row.insert("session.revision".into(), Value::from(self.revision));
        row.insert(
            "session.type".into(),
            Value::from(self.kind.as_deref().unwrap_or("user")),
        );
        row.insert("session.has_replay".into(), Value::from(self.has_replay));
        row.insert(
            "session.last_activity".into(),
            Value::from(last_activity_ms),
        );
        row.insert(
            "session.duration".into(),
            Value::from(
                last_activity_ms
                    .saturating_sub(start_ms)
                    .saturating_mul(1_000_000),
            ),
        );
        row.insert(
            "session.view.count".into(),
            Value::from(self.folded.views + self.views.len() as i64),
        );

        for (name, value) in [
            ("session.action.count", counts.actions),
            ("session.error.count", counts.errors),
            ("session.resource.count", counts.resources),
            ("session.long_task.count", counts.long_tasks),
            ("session.frustration.count", counts.frustrations),
            ("session.crash.count", counts.crashes),
        ] {
            row.insert(name.into(), Value::from(value));
        }

        if let Some(landing) = &self.landing {
            put_place(&mut row, "session.landing_view", &landing.place);
        }

        if let Some(exit) = self
            .views
            .values()
            .max_by_key(|entry| (entry.start_ms, entry.revision))
        {
            put_place(&mut row, "session.exit_view", &exit.place);
        }

        row.insert("session.view_state".into(), self.stored());

        RawEvent {
            timestamp: TimestampMicros(start_ms.saturating_mul(1000)),
            fields: row,
        }
    }

    fn stored(&self) -> Value {
        let stored = Stored {
            views: self.views.values().cloned().collect(),
            folded: self.folded,
            landing: self.landing.clone(),
        };

        serde_json::to_value(stored).unwrap_or(Value::Null)
    }
}

fn put_place(row: &mut Map<String, Value>, prefix: &str, place: &Place) {
    for (suffix, value) in [
        ("url", &place.url),
        ("url_path", &place.path),
        ("name", &place.name),
    ] {
        if let Some(value) = value {
            row.insert(format!("{prefix}.{suffix}"), Value::from(value.as_str()));
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const T0: i64 = 1_760_000_000_000;

    fn patch(view: &str, revision: i64, start_ms: i64, fields: Value) -> RawEvent {
        let mut row = json!({
            "session.id": "s-1",
            "view.id": view,
            "view.revision": revision,
            "view.time_spent": 2_000_000_000_i64,
            "view.url": format!("https://shop.example.com/{view}"),
            "view.url_path": format!("/{view}"),
            "view.name": view,
        })
        .as_object()
        .unwrap()
        .clone();

        row.extend(fields.as_object().unwrap().clone());

        RawEvent {
            timestamp: TimestampMicros(start_ms * 1000),
            fields: row,
        }
    }

    fn built(patches: &[RawEvent]) -> SessionState {
        let mut state = SessionState::new();

        for patch in patches {
            state.apply(patch);
        }

        state.revision += 1;
        state
    }

    #[test]
    fn a_session_is_what_its_views_add_up_to() {
        let state = built(&[
            patch(
                "home",
                2,
                T0,
                json!({"view.action.count": 3, "view.error.count": 1, "view.resource.count": 10}),
            ),
            patch(
                "cart",
                1,
                T0 + 5_000,
                json!({"view.action.count": 2, "view.frustration.count": 1, "view.crash.count": 1}),
            ),
        ]);

        let row = state.row("s-1", T0 + 60_000).fields;

        assert_eq!(row["session.view.count"], 2);
        assert_eq!(row["session.action.count"], 5);
        assert_eq!(row["session.error.count"], 1);
        assert_eq!(row["session.resource.count"], 10);
        assert_eq!(row["session.frustration.count"], 1);
        assert_eq!(row["session.crash.count"], 1);
        assert_eq!(row["session.landing_view.url_path"], "/home");
        assert_eq!(row["session.exit_view.url_path"], "/cart");
        assert_eq!(row["session.exit_view.name"], "cart");
    }

    #[test]
    fn a_session_lasts_until_its_latest_view_stopped() {
        let state = built(&[
            patch("home", 1, T0, json!({})),
            patch("cart", 1, T0 + 5_000, json!({})),
        ]);

        let event = state.row("s-1", T0 + 60_000);

        assert_eq!(event.timestamp, TimestampMicros(T0 * 1000));
        assert_eq!(event.fields["session.last_activity"], T0 + 5_000 + 2_000);
        assert_eq!(event.fields["session.duration"], 7_000_000_000_i64);
    }

    #[test]
    fn a_later_revision_replaces_an_earlier_one_and_an_older_report_is_ignored() {
        let mut state = built(&[patch("home", 3, T0, json!({"view.action.count": 6}))]);

        state.apply(&patch("home", 2, T0, json!({"view.action.count": 4})));
        assert_eq!(state.row("s", T0).fields["session.action.count"], 6);

        state.apply(&patch("home", 4, T0, json!({"view.action.count": 9})));
        assert_eq!(state.row("s", T0).fields["session.action.count"], 9);
        assert_eq!(state.row("s", T0).fields["session.view.count"], 1);
    }

    #[test]
    fn the_session_is_described_by_the_latest_view_that_says_so() {
        let state = built(&[
            patch(
                "home",
                1,
                T0,
                json!({"geo.country_iso_code": "DE", "usr.anonymous_id": "anon"}),
            ),
            patch(
                "cart",
                1,
                T0 + 1_000,
                json!({"usr.id": "u-42", "geo.country_iso_code": null}),
            ),
        ]);

        let row = state.row("s-1", T0).fields;

        assert_eq!(row["usr.id"], "u-42");
        assert_eq!(row["usr.anonymous_id"], "anon");
        assert_eq!(row["geo.country_iso_code"], "DE");
    }

    #[test]
    fn a_replay_in_any_view_makes_a_session_with_a_replay() {
        let state = built(&[
            patch("home", 1, T0, json!({})),
            patch("cart", 1, T0 + 1_000, json!({"session.has_replay": true})),
            patch("pay", 1, T0 + 2_000, json!({"session.has_replay": false})),
        ]);

        assert_eq!(state.row("s", T0).fields["session.has_replay"], true);
    }

    #[test]
    fn the_start_the_sdk_gives_wins_when_it_is_earlier() {
        let state = built(&[patch("home", 1, T0 + 3_000, json!({"session.start": T0}))]);

        assert_eq!(state.row("s", T0).timestamp, TimestampMicros(T0 * 1000));
    }

    #[test]
    fn rows_written_before_rebuild_the_state_whichever_node_knew_more() {
        let first = built(&[
            patch("home", 2, T0, json!({"view.action.count": 3})),
            patch("cart", 1, T0 + 1_000, json!({"view.action.count": 1})),
        ]);
        let later = {
            let mut state = SessionState::new();

            state.apply(&patch(
                "cart",
                2,
                T0 + 1_000,
                json!({"view.action.count": 2}),
            ));
            state.revision = first.revision + 1;
            state
        };

        let rows = [later.row("s-1", T0).fields, first.row("s-1", T0).fields];

        let rebuilt = SessionState::from_rows(&rows);
        let row = rebuilt.row("s-1", T0).fields;

        assert_eq!(row["session.view.count"], 2);
        assert_eq!(row["session.action.count"], 5);
        assert_eq!(row["session.landing_view.url_path"], "/home");
        assert_eq!(rebuilt.revision, first.revision + 1);
    }

    #[test]
    fn a_state_stored_as_text_is_read_as_well() {
        let state = built(&[patch("home", 2, T0, json!({"view.action.count": 3}))]);

        let mut row = state.row("s-1", T0).fields;
        let stored = row["session.view_state"].to_string();

        row.insert("session.view_state".into(), Value::String(stored));

        let rebuilt = SessionState::from_rows(&[row]);

        assert_eq!(rebuilt.row("s-1", T0).fields["session.action.count"], 3);
    }

    #[test]
    fn views_beyond_the_limit_are_folded_into_totals() {
        let mut state = SessionState::new();

        for index in 0..(MAX_TRACKED_VIEWS as i64 + 20) {
            state.apply(&patch(
                &format!("v{index}"),
                1,
                T0 + index,
                json!({"view.action.count": 1}),
            ));
        }

        let row = state.row("s-1", T0).fields;

        assert_eq!(row["session.view.count"], MAX_TRACKED_VIEWS as i64 + 20);
        assert_eq!(row["session.action.count"], MAX_TRACKED_VIEWS as i64 + 20);
        assert_eq!(row["session.landing_view.name"], "v0");

        let stored: Stored = serde_json::from_value(row["session.view_state"].clone()).unwrap();

        assert_eq!(stored.views.len(), MAX_TRACKED_VIEWS);
        assert_eq!(stored.folded.views, 20);
    }

    #[test]
    fn a_patch_without_a_view_changes_nothing() {
        let mut state = SessionState::new();
        let mut loose = patch("home", 1, T0, json!({}));

        loose.fields.remove("view.id");
        state.apply(&loose);

        assert_eq!(state.row("s", T0).fields["session.view.count"], 0);
    }
}
