// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The questions of product analytics, asked for a reader: funnels, paths and retention over the
//! sessions of an application. Each is one statement (`infra::rum::analytics`) run through the
//! reader, inside the time budget of every count, so what the engine masks stays masked and a
//! question that takes too long says so instead of failing.

use serde::{Deserialize, Serialize};

use super::reader::{Reader, Span, within_budget};
use crate::{
    domain::{query::QueryResult, rum::event::RumStream},
    infra::rum::{
        analytics::{
            self, Funnel, FunnelSpec, MAX_DEPTH, MAX_PERIODS, Paths, PathsSpec, Period, Retention,
            RetentionSpec, Step,
        },
        query::{Filter, Unavailable},
    },
    shared::{Error, Result, time::TimestampMicros},
};

const HOUR_MICROS: i64 = 3_600_000_000;
const DAY_MICROS: i64 = 24 * HOUR_MICROS;

/// How far back a funnel or the paths look unless the question says.
const DEFAULT_DAYS: i64 = 7;

/// How many periods after the first retention reads unless the question says.
const DEFAULT_PERIODS: usize = 7;

/// How many pages after (before) the page the paths go unless the question says.
const DEFAULT_DEPTH: usize = 3;

/// The pages of a way a question asks for: from 1 to the most, 3 when it says none.
fn depth_of(asked: Option<usize>) -> Result<usize> {
    let depth = asked.unwrap_or(DEFAULT_DEPTH);

    if !(1..=MAX_DEPTH).contains(&depth) {
        return Err(Error::invalid(format!(
            "depth must be between 1 and {MAX_DEPTH} pages"
        )));
    }

    Ok(depth)
}

/// The periods retention reads after the first: from 1 to the most, 7 when the question says none.
fn periods_of(asked: Option<usize>) -> Result<usize> {
    let periods = asked.unwrap_or(DEFAULT_PERIODS);

    if !(1..=MAX_PERIODS).contains(&periods) {
        return Err(Error::invalid(format!(
            "periods must be between 1 and {MAX_PERIODS}"
        )));
    }

    Ok(periods)
}

/// What an answer says besides its body: the range it is of, and why it has no numbers if it has none.
#[derive(Debug, Serialize)]
pub struct Answer<T: Serialize> {
    #[serde(flatten)]
    pub body: T,
    pub from: i64,
    pub to: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<Unavailable>,
}

/// The range of a question: the ends given, else the `days` before now.
fn span_of(from: Option<i64>, to: Option<i64>, days: i64) -> Span {
    let to = to.unwrap_or_else(|| TimestampMicros::now().0);

    Span {
        from: from.unwrap_or_else(|| to.saturating_sub(days * DAY_MICROS)),
        to,
    }
}

/// Runs a statement for a stream, inside the time budget: nothing if the stream was never
/// written to, or why there is no answer.
async fn ask(
    reader: &Reader<'_>,
    stream: RumStream,
    span: Span,
    sql: String,
) -> Result<std::result::Result<Option<QueryResult>, Unavailable>> {
    within_budget(reader.run_tracked(stream, span, sql)).await
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunnelRequest {
    pub from: Option<i64>,
    pub to: Option<i64>,
    /// What every step has to meet: the application, the environment…
    #[serde(default)]
    pub filters: Vec<Filter>,
    pub steps: Vec<Step>,
    /// The longest from the first step to a later one, in seconds.
    pub window_secs: Option<i64>,
    /// A text field of the first step's rows to count the sessions by.
    pub breakdown: Option<String>,
}

pub async fn funnel(reader: &Reader<'_>, request: &FunnelRequest) -> Result<Answer<Funnel>> {
    let span = span_of(request.from, request.to, DEFAULT_DAYS);
    let breakdown = request
        .breakdown
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty());

    let window_micros = request
        .window_secs
        .map(|seconds| {
            seconds
                .checked_mul(1_000_000)
                .ok_or_else(|| Error::invalid("the window of a funnel is too long"))
        })
        .transpose()?;

    let sql = analytics::funnel_sql(&FunnelSpec {
        from: span.from,
        to: span.to,
        shared: &request.filters,
        steps: &request.steps,
        window_micros,
        breakdown,
    })?;

    // A step of a stream nobody has written to has no sessions, and so has the funnel.
    for step in &request.steps {
        if !reader.has(step.kind.stream()).await? {
            return Ok(Answer {
                body: analytics::empty_funnel(&request.steps, breakdown.is_some()),
                from: span.from,
                to: span.to,
                unavailable: None,
            });
        }
    }

    let first = request.steps[0].kind.stream();

    Ok(match ask(reader, first, span, sql).await? {
        Ok(Some(result)) => Answer {
            body: analytics::read_funnel(
                &request.steps,
                breakdown.is_some(),
                &result.columns,
                &result.rows,
            ),
            from: span.from,
            to: span.to,
            unavailable: None,
        },
        Ok(None) => Answer {
            body: analytics::empty_funnel(&request.steps, breakdown.is_some()),
            from: span.from,
            to: span.to,
            unavailable: None,
        },
        Err(reason) => Answer {
            body: analytics::empty_funnel(&request.steps, breakdown.is_some()),
            from: span.from,
            to: span.to,
            unavailable: Some(reason),
        },
    })
}

#[derive(Debug, Default, Deserialize)]
pub struct PathsRequest {
    pub from: Option<i64>,
    pub to: Option<i64>,
    #[serde(default)]
    pub filters: Vec<Filter>,
    /// The page the paths start from, or end at going backward.
    pub anchor: Option<String>,
    #[serde(default)]
    pub backward: bool,
    /// How many pages after (before) the anchor; 3 unless given.
    pub depth: Option<usize>,
    /// The field a page is told by; the path with the changing parts grouped unless given.
    pub by: Option<String>,
}

pub async fn paths(reader: &Reader<'_>, request: &PathsRequest) -> Result<Answer<Paths>> {
    let span = span_of(request.from, request.to, DEFAULT_DAYS);
    let depth = depth_of(request.depth)?;
    let by = request
        .by
        .as_deref()
        .map(str::trim)
        .filter(|by| !by.is_empty())
        .unwrap_or("view.url_path_group");

    let spec = PathsSpec {
        from: span.from,
        to: span.to,
        shared: &request.filters,
        anchor: request.anchor.as_deref(),
        backward: request.backward,
        depth,
        by,
    };

    let sql = analytics::paths_sql(&spec)?;
    let empty = || analytics::read_paths(depth, request.backward, &[], &[]);

    if !reader.has(RumStream::Views).await? {
        return Ok(Answer {
            body: empty(),
            from: span.from,
            to: span.to,
            unavailable: None,
        });
    }

    Ok(match ask(reader, RumStream::Views, span, sql).await? {
        Ok(Some(result)) => Answer {
            body: analytics::read_paths(depth, request.backward, &result.columns, &result.rows),
            from: span.from,
            to: span.to,
            unavailable: None,
        },
        Ok(None) => Answer {
            body: empty(),
            from: span.from,
            to: span.to,
            unavailable: None,
        },
        Err(reason) => Answer {
            body: empty(),
            from: span.from,
            to: span.to,
            unavailable: Some(reason),
        },
    })
}

#[derive(Debug, Default, Deserialize)]
pub struct RetentionRequest {
    pub from: Option<i64>,
    pub to: Option<i64>,
    #[serde(default)]
    pub filters: Vec<Filter>,
    pub period: Option<Period>,
    /// How many periods after the first; 7 unless given.
    pub periods: Option<usize>,
}

pub async fn retention(
    reader: &Reader<'_>,
    request: &RetentionRequest,
) -> Result<Answer<Retention>> {
    let period = request.period.unwrap_or(Period::Day);
    let periods = periods_of(request.periods)?;

    // Enough history to have cohorts to look at: the periods asked for, and as many again.
    let days = match period {
        Period::Day => (2 * periods as i64).max(14),
        Period::Week => 7 * (2 * periods as i64).max(8),
    };

    let span = span_of(request.from, request.to, days);

    let spec = RetentionSpec {
        from: span.from,
        to: span.to,
        shared: &request.filters,
        period,
        periods,
    };

    let sql = analytics::retention_sql(&spec)?;
    let empty = || analytics::read_retention(&spec, &[], &[]);

    if !reader.has(RumStream::Sessions).await? {
        return Ok(Answer {
            body: empty(),
            from: span.from,
            to: span.to,
            unavailable: None,
        });
    }

    Ok(match ask(reader, RumStream::Sessions, span, sql).await? {
        Ok(Some(result)) => Answer {
            body: analytics::read_retention(&spec, &result.columns, &result.rows),
            from: span.from,
            to: span.to,
            unavailable: None,
        },
        Ok(None) => Answer {
            body: empty(),
            from: span.from,
            to: span.to,
            unavailable: None,
        },
        Err(reason) => Answer {
            body: empty(),
            from: span.from,
            to: span.to,
            unavailable: Some(reason),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_question_without_a_range_looks_at_the_days_before_now() {
        let span = span_of(None, None, 7);

        assert_eq!(span.to - span.from, 7 * DAY_MICROS);
    }

    #[test]
    fn the_depth_of_paths_and_the_periods_of_retention_are_between_one_and_the_most() {
        assert_eq!(depth_of(None).unwrap(), 3);
        assert_eq!(depth_of(Some(1)).unwrap(), 1);
        assert_eq!(depth_of(Some(MAX_DEPTH)).unwrap(), MAX_DEPTH);
        assert!(depth_of(Some(0)).is_err());
        assert!(depth_of(Some(MAX_DEPTH + 1)).is_err());

        assert_eq!(periods_of(None).unwrap(), 7);
        assert_eq!(periods_of(Some(30)).unwrap(), 30);
        assert!(periods_of(Some(0)).is_err());
        assert!(periods_of(Some(MAX_PERIODS + 1)).is_err());
        assert!(periods_of(Some(usize::MAX)).is_err());
    }

    #[test]
    fn the_ends_given_are_kept() {
        let span = span_of(Some(10), Some(20), 7);

        assert_eq!((span.from, span.to), (10, 20));

        let open = span_of(None, Some(8 * DAY_MICROS), 7);

        assert_eq!(open.from, DAY_MICROS);
    }
}
