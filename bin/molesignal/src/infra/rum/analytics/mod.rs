// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The questions of product analytics asked of real sessions: how many sessions went through a
//! series of steps (a funnel), where they went from a page (paths), and how many of the people
//! who came first on a day came back (retention).
//!
//! Each is one statement over the current rows of the RUM streams (`rum_views_latest`,
//! `rum_sessions_latest`, `rum_actions`), run by the engine that answers everything else, so what
//! the engine masks or prunes holds here too. This module is only the text of the statements and
//! the reading of their answers; `app::rum::analytics` runs them.

mod funnel;
mod paths;
mod retention;
#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

pub use self::{
    funnel::{
        Funnel, FunnelGroup, FunnelSpec, FunnelStep, GROUPS, empty_funnel, funnel_sql, read_funnel,
    },
    paths::{
        MAX_DEPTH, OTHER, PAGE_FIELDS, PER_LEVEL, PathLink, PathNode, Paths, PathsSpec, paths_sql,
        read_paths,
    },
    retention::{
        Cohort, MAX_PERIODS, Period, Retention, RetentionSpec, read_retention, retention_sql,
    },
};
use super::query::{Condition, Filter, asks_about_traffic, conditions, people_only};
use crate::{
    domain::rum::event::RumStream,
    shared::{Error, Result},
};

/// Fewest and most steps a funnel has.
pub const MIN_STEPS: usize = 2;
pub const MAX_STEPS: usize = 8;

/// What a step looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    /// A page or a screen that was opened.
    Views,
    /// Something a person did: a click, a tap, a scroll.
    Actions,
}

impl StepKind {
    pub fn stream(self) -> RumStream {
        match self {
            Self::Views => RumStream::Views,
            Self::Actions => RumStream::Actions,
        }
    }

    /// What a statement reads: the views as they were written when what it asks of them is the
    /// same in every revision of a view (see [`views_relation`]), else the latest revision of each;
    /// the rows of an action.
    pub(crate) fn relation<'a>(
        self,
        conditions: &'a [Condition],
        besides: impl IntoIterator<Item = &'a str>,
    ) -> &'static str {
        match self {
            Self::Views => views_relation(conditions, besides),
            Self::Actions => "rum_actions",
        }
    }
}

/// Columns that say the same in every revision of a view: the application and the release it was
/// of, the session it belongs to, the page it is, and the client that opened it.
const STABLE_VIEW_COLUMNS: &[&str] = &[
    "session.type",
    "application.id",
    "env",
    "service",
    "version",
    "source",
    "session.id",
    "view.id",
    "view.name",
    "view.url",
    "view.url_host",
    "view.url_path",
    "view.url_path_group",
    "view.referrer",
    "browser.name",
    "os.name",
    "device.type",
    "geo.country_iso_code",
    "geo.country",
];

/// What to read the views of a statement from.
///
/// A view is written again each time its numbers change, and what is asked of a view is asked of
/// the newest of these (`rum_views_latest`). A statement that asks of a view only what is the same
/// in all of them, and takes only the time and the session of it, can read the rows as they were
/// written (`rum_views`): a view is then in them more than once, at one and the same time, and the
/// sessions that did a step are the same. That spares the work of choosing the newest revision of
/// every view, which is most of the cost of a funnel or of paths over many sessions.
pub(crate) fn views_relation<'a>(
    conditions: &'a [Condition],
    besides: impl IntoIterator<Item = &'a str>,
) -> &'static str {
    let mut asked = conditions
        .iter()
        .flat_map(|condition| condition.columns.iter().map(String::as_str))
        .chain(besides);

    if asked.all(|column| STABLE_VIEW_COLUMNS.contains(&column)) {
        "rum_views"
    } else {
        "rum_views_latest"
    }
}

/// One step of a funnel: the rows of a stream that count as the session having got there.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Step {
    #[serde(rename = "type")]
    pub kind: StepKind,
    /// What the step is called in the answer; the value it looks for by default.
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub filters: Vec<Filter>,
}

/// What a step is called: its name, or the value of its first filter, or its place.
pub(crate) fn label(step: &Step, index: usize) -> String {
    step.name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .or_else(|| {
            step.filters
                .first()
                .map(|filter| filter.value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_else(|| format!("#{}", index + 1))
}

/// The conditions of one step: the filters every step shares, then its own, asked of its stream.
pub(crate) fn step_conditions(shared: &[Filter], step: &Step) -> Result<Vec<Condition>> {
    let filters = shared
        .iter()
        .chain(&step.filters)
        .cloned()
        .collect::<Vec<_>>();

    of_people(step.kind.stream(), &filters)
}

/// The conditions every row of a stream has to meet, as filters of the explorer say them.
pub(crate) fn shared_conditions(stream: RumStream, shared: &[Filter]) -> Result<Vec<Condition>> {
    of_people(stream, shared)
}

/// The conditions of filters, of the sessions people ran unless the filters ask about what ran them.
fn of_people(stream: RumStream, filters: &[Filter]) -> Result<Vec<Condition>> {
    let mut found = conditions(stream, filters)?;

    if !asks_about_traffic(filters) {
        found.push(people_only());
    }

    Ok(found)
}

/// Every row of a relation in the range that belongs to a session and meets the conditions.
pub(crate) fn events(
    relation: &str,
    columns: &str,
    from: i64,
    to: i64,
    conditions: &[Condition],
) -> String {
    let mut found = vec![
        format!("CAST(_timestamp AS BIGINT) >= {from}"),
        format!("CAST(_timestamp AS BIGINT) < {to}"),
        "\"session.id\" IS NOT NULL".to_string(),
    ];

    found.extend(conditions.iter().map(|condition| condition.sql.clone()));

    format!(
        "SELECT {columns} FROM {relation} WHERE {}",
        found.join(" AND ")
    )
}

/// The range of a read: both ends given and the end after the start.
pub(crate) fn checked_range(from: i64, to: i64) -> Result<()> {
    if to <= from {
        return Err(Error::invalid("range end must be greater than start"));
    }

    Ok(())
}

/// A number out of a cell of an answer, whatever the engine made of it.
pub(crate) fn whole(value: Option<&serde_json::Value>) -> i64 {
    value
        .and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_f64().map(|float| float as i64))
                .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
        })
        .unwrap_or_default()
}

pub(crate) fn text(value: Option<&serde_json::Value>) -> Option<String> {
    match value? {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Null => None,
        other => Some(other.to_string()),
    }
}

pub(crate) fn real(value: Option<&serde_json::Value>) -> Option<f64> {
    value.and_then(|value| {
        value
            .as_f64()
            .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
    })
}
