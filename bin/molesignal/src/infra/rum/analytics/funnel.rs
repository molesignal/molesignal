// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Funnels: of the sessions that did the first step, how many went on to do the second after it,
//! and the third after that. A session counts for a step by the first row that meets the step
//! after the session got to the step before; the time between two steps is read from those rows.
//!
//! A session counts for as many as the share it was kept with says (the first step's row tells
//! it): a funnel of an application that keeps a quarter of its sessions counts each of them for
//! four.

use serde::Serialize;
use serde_json::Value;

use super::{
    MAX_STEPS, MIN_STEPS, Step, checked_range, events, label, real, step_conditions, text, whole,
};
use crate::{
    domain::{rum::event::field, stream::FieldType},
    infra::rum::query::{Filter, WEIGHT, ident},
    shared::{Error, Result},
};

/// How many groups a funnel broken down by a field keeps: the ones with the most sessions.
pub const GROUPS: usize = 10;

/// What to count.
#[derive(Debug, Clone, Copy)]
pub struct FunnelSpec<'a> {
    /// Microseconds; a row counts when `from <= _timestamp < to`.
    pub from: i64,
    pub to: i64,
    /// What every step has to meet besides its own filters: the application, the environment…
    pub shared: &'a [Filter],
    pub steps: &'a [Step],
    /// Longest from the first step to a later one, in microseconds; none is as long as it takes.
    pub window_micros: Option<i64>,
    /// A text field of the first step's rows to count the sessions by.
    pub breakdown: Option<&'a str>,
}

/// The column the sessions are told apart by: a text field of the rows of the first step, as it is
/// named and quoted.
fn breakdown_column(steps: &[Step], name: &str) -> Result<(&'static str, String)> {
    let stream = steps[0].kind.stream();

    match field(stream, name.trim()) {
        Some(column) if column.data_type == FieldType::Utf8 => {
            Ok((column.name, ident(column.name)))
        }
        Some(column) => Err(Error::invalid(format!(
            "`{}` has no text to tell sessions apart by",
            column.name
        ))),
        None => Err(Error::invalid(format!(
            "`{name}` is not a field of {}",
            stream.name()
        ))),
    }
}

/// The statement: a row per group (one row without a breakdown) with `c1…cN`, how many sessions
/// got to each step, and `m2…mN`, the median microseconds from the step before.
pub fn funnel_sql(spec: &FunnelSpec<'_>) -> Result<String> {
    let steps = spec.steps;

    if steps.len() < MIN_STEPS || steps.len() > MAX_STEPS {
        return Err(Error::invalid(format!(
            "a funnel has {MIN_STEPS} to {MAX_STEPS} steps"
        )));
    }

    checked_range(spec.from, spec.to)?;

    if spec.window_micros.is_some_and(|window| window <= 0) {
        return Err(Error::invalid("the window of a funnel must be positive"));
    }

    let breakdown = spec
        .breakdown
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| breakdown_column(steps, name))
        .transpose()?;

    let mut parts = Vec::new();

    for (index, step) in steps.iter().enumerate() {
        let found = step_conditions(spec.shared, step)?;

        let columns = match (&breakdown, index) {
            (Some((_, column)), 0) => format!(
                "\"session.id\" AS sid, CAST(_timestamp AS BIGINT) AS ts, {WEIGHT} AS w, \
                 CAST({column} AS VARCHAR) AS grp"
            ),
            (None, 0) => {
                format!("\"session.id\" AS sid, CAST(_timestamp AS BIGINT) AS ts, {WEIGHT} AS w")
            }
            _ => "\"session.id\" AS sid, CAST(_timestamp AS BIGINT) AS ts".to_string(),
        };

        // The column the first step tells its sessions apart by is read of its rows too.
        let besides = breakdown
            .as_ref()
            .filter(|_| index == 0)
            .map(|(name, _)| *name);

        let relation = step.kind.relation(&found, besides);

        parts.push(format!(
            "e{} AS ({})",
            index + 1,
            events(relation, &columns, spec.from, spec.to, &found)
        ));
    }

    let carried = if breakdown.is_some() {
        ", first_value(grp ORDER BY ts) AS grp"
    } else {
        ""
    };

    parts.push(format!(
        "s1 AS (SELECT sid, min(ts) AS t1, min(ts) AS t, min(w) AS w{carried} FROM e1 GROUP BY sid)"
    ));

    let window = spec
        .window_micros
        .map(|window| format!(" AND e.ts <= p.t1 + {window}"))
        .unwrap_or_default();

    for number in 2..=steps.len() {
        parts.push(format!(
            "s{number} AS (SELECT p.sid AS sid, p.t1 AS t1, p.t AS tp, min(e.ts) AS t \
             FROM e{number} e JOIN s{} p ON e.sid = p.sid AND e.ts > p.t{window} \
             GROUP BY p.sid, p.t1, p.t)",
            number - 1
        ));
    }

    let mut selected = vec![format!(
        "{} AS grp",
        if breakdown.is_some() { "s1.grp" } else { "''" }
    )];

    selected.extend((1..=steps.len()).map(|number| {
        format!(
            "CAST(COALESCE(ROUND(SUM(CASE WHEN s{number}.sid IS NOT NULL THEN s1.w END)), 0) \
             AS BIGINT) AS c{number}"
        )
    }));
    selected.extend((2..=steps.len()).map(|number| {
        format!("approx_percentile_cont(0.5) WITHIN GROUP (ORDER BY s{number}.t - s{number}.tp) AS m{number}")
    }));

    let joins = (2..=steps.len())
        .map(|number| format!(" LEFT JOIN s{number} ON s1.sid = s{number}.sid"))
        .collect::<String>();

    let tail = if breakdown.is_some() {
        format!(" GROUP BY s1.grp ORDER BY c1 DESC LIMIT {GROUPS}")
    } else {
        String::new()
    };

    Ok(format!(
        "WITH {} SELECT {} FROM s1{joins}{tail}",
        parts.join(", "),
        selected.join(", ")
    ))
}

/// One step of an answer.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FunnelStep {
    pub name: String,
    pub sessions: u64,
    /// The share of the sessions of the first step that got here; none when there are none.
    pub conversion: Option<f64>,
    /// The share of the sessions of the step before that got here.
    pub from_previous: Option<f64>,
    /// The middle of the times between the step before and this one, in milliseconds.
    pub median_ms: Option<f64>,
}

/// A funnel of one group of sessions: all of them, or those with one value of the breakdown.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FunnelGroup {
    pub value: Option<String>,
    pub steps: Vec<FunnelStep>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Funnel {
    pub groups: Vec<FunnelGroup>,
}

/// The funnel of a read that found nothing to count: every step has no sessions.
pub fn empty_funnel(steps: &[Step], breakdown: bool) -> Funnel {
    if breakdown {
        return Funnel { groups: Vec::new() };
    }

    read_funnel(steps, false, &[], &[Vec::new()])
}

fn share(part: u64, whole: u64) -> Option<f64> {
    (whole > 0).then(|| part as f64 / whole as f64)
}

/// The funnel an answer of [`funnel_sql`] says.
pub fn read_funnel(
    steps: &[Step],
    breakdown: bool,
    columns: &[String],
    rows: &[Vec<Value>],
) -> Funnel {
    let at = |name: &str| columns.iter().position(|column| column == name);

    let groups = rows
        .iter()
        .map(|row| {
            let cell = |name: &str| at(name).and_then(|index| row.get(index));

            let counts = (1..=steps.len())
                .map(|number| whole(cell(&format!("c{number}"))).max(0) as u64)
                .collect::<Vec<_>>();

            let steps = steps
                .iter()
                .enumerate()
                .map(|(index, step)| FunnelStep {
                    name: label(step, index),
                    sessions: counts[index],
                    conversion: share(counts[index], counts[0]),
                    from_previous: (index > 0)
                        .then(|| share(counts[index], counts[index - 1]))
                        .flatten(),
                    median_ms: (index > 0)
                        .then(|| real(cell(&format!("m{}", index + 1))))
                        .flatten()
                        .map(|micros| micros / 1_000.0),
                })
                .collect();

            FunnelGroup {
                value: if breakdown { text(cell("grp")) } else { None },
                steps,
            }
        })
        .collect();

    Funnel { groups }
}
