// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Retention: of the people who were first seen in a period, how many were seen again in each of
//! the periods after. A person is told by their user id, or where a visitor has none by the
//! anonymous id the SDK keeps; a period is a day or a week of UTC. Being first seen means first
//! seen in the range that was asked for: the range is the history that is looked at.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{checked_range, events, shared_conditions, whole};
use crate::{
    domain::rum::event::RumStream,
    infra::rum::query::{Condition, Filter},
    shared::{Error, Result},
};

const DAY_MICROS: i64 = 86_400_000_000;

/// Most periods after the first that are read.
pub const MAX_PERIODS: usize = 30;

/// How long a period is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Period {
    Day,
    Week,
}

impl Period {
    /// The SQL that numbers the periods since the epoch, from a time in microseconds. A week
    /// begins on Monday; the epoch was a Thursday.
    fn index(self) -> String {
        match self {
            Self::Day => format!("CAST(_timestamp AS BIGINT) / {DAY_MICROS}"),
            Self::Week => format!("(CAST(_timestamp AS BIGINT) / {DAY_MICROS} + 3) / 7"),
        }
    }

    /// When period `index` begins, in microseconds.
    pub(super) fn start(self, index: i64) -> i64 {
        match self {
            Self::Day => index * DAY_MICROS,
            Self::Week => (index * 7 - 3) * DAY_MICROS,
        }
    }

    pub(super) fn length(self) -> i64 {
        match self {
            Self::Day => DAY_MICROS,
            Self::Week => 7 * DAY_MICROS,
        }
    }
}

/// What to count.
#[derive(Debug, Clone, Copy)]
pub struct RetentionSpec<'a> {
    pub from: i64,
    pub to: i64,
    pub shared: &'a [Filter],
    pub period: Period,
    /// How many periods after the first are read.
    pub periods: usize,
}

/// The statement: a row per cohort and period after it (`cohort` the number of the period the
/// cohort was first seen in, `k` how many periods later, `users` how many were seen then).
pub fn retention_sql(spec: &RetentionSpec<'_>) -> Result<String> {
    checked_range(spec.from, spec.to)?;

    if spec.periods == 0 || spec.periods > MAX_PERIODS {
        return Err(Error::invalid(format!(
            "retention reads 1 to {MAX_PERIODS} periods after the first"
        )));
    }

    // The sessions of the range, one per person and period they were seen in; a session is read
    // at its latest revision.
    let mut found = shared_conditions(RumStream::Sessions, spec.shared)?;

    found.push(Condition::raw(
        "COALESCE(NULLIF(\"usr.id\", ''), \"usr.anonymous_id\") IS NOT NULL",
        &["usr.id", "usr.anonymous_id"],
        false,
    ));

    let seen = events(
        "rum_sessions_latest",
        &format!(
            "COALESCE(NULLIF(\"usr.id\", ''), \"usr.anonymous_id\") AS uid, {} AS p",
            spec.period.index()
        ),
        spec.from,
        spec.to,
        &found,
    );

    Ok(format!(
        "WITH act AS (SELECT DISTINCT uid, p FROM ({seen}) AS seen WHERE uid <> ''), \
         c AS (SELECT uid, min(p) AS cohort FROM act GROUP BY uid) \
         SELECT c.cohort AS cohort, act.p - c.cohort AS k, count(DISTINCT act.uid) AS users \
         FROM act JOIN c ON act.uid = c.uid WHERE act.p - c.cohort <= {} \
         GROUP BY c.cohort, act.p - c.cohort ORDER BY c.cohort, k",
        spec.periods
    ))
}

/// The people first seen in one period, and how many of them were seen in each period after.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cohort {
    /// When the period they were first seen in begins, in microseconds.
    pub start_time: i64,
    pub users: u64,
    /// How many were seen `k` periods later, for `k` from 1; none for a period that has not ended
    /// by the end of the range.
    pub returned: Vec<Option<u64>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Retention {
    pub period: Period,
    pub periods: usize,
    pub cohorts: Vec<Cohort>,
}

/// The cohorts an answer of [`retention_sql`] says.
pub fn read_retention(
    spec: &RetentionSpec<'_>,
    columns: &[String],
    rows: &[Vec<Value>],
) -> Retention {
    let at = |name: &str| columns.iter().position(|column| column == name);
    let (cohort, k, users) = (at("cohort"), at("k"), at("users"));

    let mut counts = std::collections::BTreeMap::<i64, Vec<u64>>::new();

    for row in rows {
        let cell = |index: Option<usize>| index.and_then(|index| row.get(index));
        let (index, later) = (whole(cell(cohort)), whole(cell(k)));

        if later < 0 || later as usize > spec.periods {
            continue;
        }

        let slots = counts
            .entry(index)
            .or_insert_with(|| vec![0; spec.periods + 1]);

        slots[later as usize] = whole(cell(users)).max(0) as u64;
    }

    let cohorts = counts
        .into_iter()
        .map(|(index, slots)| {
            let start = spec.period.start(index);

            Cohort {
                start_time: start,
                users: slots[0],
                returned: (1..=spec.periods)
                    .map(|later| {
                        let ends = start + (later as i64 + 1) * spec.period.length();

                        (ends <= spec.to).then_some(slots[later])
                    })
                    .collect(),
            }
        })
        .collect();

    Retention {
        period: spec.period,
        periods: spec.periods,
        cohorts,
    }
}
