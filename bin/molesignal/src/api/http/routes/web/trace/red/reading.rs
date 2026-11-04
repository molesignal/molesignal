// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The statements run one after the other, and the figures read out of what
//! they return.
//!
//! A masked value comes back as text. So a bar that is not a number means
//! time is masked, and a count of failures or a duration that is not one means
//! the field it is read from is: that figure is then not told.

use std::future::Future;

use serde::Serialize;
use serde_json::Value;

use super::statements::{
    self, BAR, BUCKET, ERRORS, Grouping, P50, P95, P99, PRESENT, REQUESTS, Source, VALUE,
};
use crate::{
    api::http::routes::web::histogram::BucketPlan,
    domain::query::QueryResult,
    shared::{Error, Result},
};

/// How many values of a field a breakdown lists, the commonest.
pub(super) const GROUP_LIMIT: usize = 50;

/// The RED figures of a set of requests.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Figures {
    pub(super) requests: u64,
    /// `None` where failures cannot be told: the stream keeps no status, or
    /// the status is masked.
    pub(super) errors: Option<u64>,
    /// How long the requests took, in nanoseconds. `None` without a request,
    /// and where durations are masked.
    pub(super) p50_ns: Option<f64>,
    pub(super) p95_ns: Option<f64>,
    pub(super) p99_ns: Option<f64>,
}

/// The figures of one bar of the range.
#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Bucket {
    pub(super) start_time: i64,
    #[serde(flatten)]
    pub(super) figures: Figures,
}

/// How many requests took from `from_ns` up to `to_ns`.
#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LatencyBar {
    pub(super) from_ns: i64,
    /// `None` for the last bar, which holds every longer duration.
    pub(super) to_ns: Option<i64>,
    pub(super) requests: u64,
    pub(super) errors: Option<u64>,
}

/// The requests that have one value of a field.
#[derive(Debug, PartialEq, Serialize)]
pub(super) struct Group {
    /// `None` for the requests without a value.
    pub(super) value: Option<String>,
    #[serde(flatten)]
    pub(super) figures: Figures,
    /// The bars that hold one of these requests, oldest first.
    pub(super) series: Vec<Point>,
}

/// One bar of a group.
#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Point {
    pub(super) start_time: i64,
    pub(super) requests: u64,
    pub(super) errors: Option<u64>,
    pub(super) p95_ns: Option<f64>,
}

#[derive(Debug, Default, PartialEq)]
pub(super) struct Overview {
    pub(super) total: Figures,
    /// The bars that hold a request, oldest first. `None` when time is masked:
    /// there is nothing to lay them out along.
    pub(super) buckets: Option<Vec<Bucket>>,
    /// The bars of durations that hold a request, shortest first.
    pub(super) latency: Vec<LatencyBar>,
}

#[derive(Debug, PartialEq)]
pub(super) struct Breakdown {
    pub(super) groups: Vec<Group>,
    /// More values than are listed.
    pub(super) more: bool,
}

/// The figures of the range, of each of its bars, and how long its requests took.
pub(super) async fn overview<Run, Running>(
    source: &Source,
    plan: &BucketPlan,
    run: Run,
) -> Result<Overview>
where
    Run: Fn(String) -> Running,
    Running: Future<Output = Result<QueryResult>>,
{
    let totals = run(statements::totals(source)).await?;

    let total = totals
        .rows
        .first()
        .map(|row| figures(&totals.columns, row))
        .transpose()?
        .unwrap_or_default();

    // Nothing to lay out over time or by duration.
    if total.requests == 0 {
        return Ok(Overview {
            total,
            buckets: Some(Vec::new()),
            latency: Vec::new(),
        });
    }

    let series = run(statements::series(source, plan)).await?;
    let buckets = buckets(&series, plan)?;
    let latency = latency(&run(statements::latency(source)).await?)?;

    Ok(Overview {
        total,
        buckets,
        latency,
    })
}

/// The commonest values of a field with their figures, each followed bar by
/// bar. `None` when the field is masked: its values are not the reader's.
pub(super) async fn breakdown<Run, Running>(
    source: &Source,
    grouping: &Grouping,
    plan: &BucketPlan,
    run: Run,
) -> Result<Option<Breakdown>>
where
    Run: Fn(String) -> Running,
    Running: Future<Output = Result<QueryResult>>,
{
    let ranked = run(statements::groups(source, grouping, GROUP_LIMIT + 1)).await?;

    let Some(mut groups) = groups(&ranked)? else {
        return Ok(None);
    };

    let more = groups.len() > GROUP_LIMIT;
    groups.truncate(GROUP_LIMIT);

    if !groups.is_empty() {
        let values = groups
            .iter()
            .map(|group| group.value.clone())
            .collect::<Vec<_>>();

        let series = run(statements::group_series(source, grouping, &values, plan)).await?;
        follow(&mut groups, &series, plan)?;
    }

    Ok(Some(Breakdown { groups, more }))
}

fn figures(columns: &[String], row: &[Value]) -> Result<Figures> {
    let at = |name: &str| position(columns, name).and_then(|index| row.get(index));

    let requests = at(REQUESTS)
        .and_then(count)
        .ok_or_else(|| Error::internal("RED statement returned no count of requests"))?;

    Ok(Figures {
        requests,
        errors: failures(at(ERRORS), requests),
        p50_ns: at(P50).and_then(duration),
        p95_ns: at(P95).and_then(duration),
        p99_ns: at(P99).and_then(duration),
    })
}

/// `None` when time is masked.
fn buckets(result: &QueryResult, plan: &BucketPlan) -> Result<Option<Vec<Bucket>>> {
    let bucket = required(&result.columns, BUCKET)?;
    let mut buckets = Vec::with_capacity(result.rows.len());

    for row in &result.rows {
        let index = match row.get(bucket) {
            Some(Value::Number(index)) => index.as_i64(),
            Some(Value::Null) | None => continue,
            Some(_) => return Ok(None),
        };

        let Some(start_micros) = index.and_then(|index| plan.start_of(index)) else {
            continue;
        };

        buckets.push(Bucket {
            start_time: start_micros,
            figures: figures(&result.columns, row)?,
        });
    }

    buckets.sort_by_key(|bucket| bucket.start_time);

    Ok(Some(buckets))
}

/// Empty when durations are masked: no bar can be told.
fn latency(result: &QueryResult) -> Result<Vec<LatencyBar>> {
    let edges = statements::latency_edges();
    let bar_at = required(&result.columns, BAR)?;
    let requests_at = required(&result.columns, REQUESTS)?;
    let errors_at = position(&result.columns, ERRORS);
    let mut bars = Vec::with_capacity(result.rows.len());

    for row in &result.rows {
        let Some(index) = row
            .get(bar_at)
            .and_then(Value::as_u64)
            .and_then(|index| usize::try_from(index).ok())
        else {
            return Ok(Vec::new());
        };

        let (Some(&from), Some(requests)) =
            (edges.get(index), row.get(requests_at).and_then(count))
        else {
            continue;
        };

        bars.push(LatencyBar {
            // The first bar holds whatever is shorter than its end too.
            from_ns: if index == 0 { 0 } else { from },
            to_ns: edges.get(index + 1).copied(),
            requests,
            errors: failures(errors_at.and_then(|at| row.get(at)), requests),
        });
    }

    bars.sort_by_key(|bar| bar.from_ns);

    Ok(bars)
}

/// `None` when the field is masked.
fn groups(result: &QueryResult) -> Result<Option<Vec<Group>>> {
    let value_at = required(&result.columns, VALUE)?;
    let present_at = required(&result.columns, PRESENT)?;
    let mut groups = Vec::with_capacity(result.rows.len());

    for row in &result.rows {
        // A count taken over the field that is not a number came back masked.
        if row
            .get(present_at)
            .is_some_and(|present| !present.is_null() && count(present).is_none())
        {
            return Ok(None);
        }

        groups.push(Group {
            value: text(row.get(value_at)),
            figures: figures(&result.columns, row)?,
            series: Vec::new(),
        });
    }

    Ok(Some(groups))
}

/// Puts each bar of `result` with the group of its value. A group whose bars
/// cannot be laid out over time (time is masked) is left without them.
fn follow(groups: &mut [Group], result: &QueryResult, plan: &BucketPlan) -> Result<()> {
    let value_at = required(&result.columns, VALUE)?;
    let bucket_at = required(&result.columns, BUCKET)?;
    let requests_at = required(&result.columns, REQUESTS)?;
    let errors_at = position(&result.columns, ERRORS);
    let slowest_at = position(&result.columns, P95);

    for row in &result.rows {
        let Some(start_micros) = row
            .get(bucket_at)
            .and_then(Value::as_i64)
            .and_then(|index| plan.start_of(index))
        else {
            continue;
        };

        let named = text(row.get(value_at));

        let (Some(group), Some(requests)) = (
            groups.iter_mut().find(|group| group.value == named),
            row.get(requests_at).and_then(count),
        ) else {
            continue;
        };

        group.series.push(Point {
            start_time: start_micros,
            requests,
            errors: failures(errors_at.and_then(|at| row.get(at)), requests),
            p95_ns: slowest_at.and_then(|at| row.get(at)).and_then(duration),
        });
    }

    for group in groups {
        group.series.sort_by_key(|point| point.start_time);
    }

    Ok(())
}

/// Failures as counted. A sum over no request is no number, and none failed;
/// a sum that is not a number otherwise came back masked.
fn failures(value: Option<&Value>, requests: u64) -> Option<u64> {
    match value? {
        Value::Null => (requests == 0).then_some(0),
        value => count(value).map(|errors| errors.min(requests)),
    }
}

/// A whole count, however the engine typed it.
fn count(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| {
        value
            .as_f64()
            .filter(|number| number.is_finite() && *number >= 0.0 && number.fract() == 0.0)
            .map(|number| number as u64)
    })
}

fn duration(value: &Value) -> Option<f64> {
    value.as_f64().filter(|number| number.is_finite())
}

/// A value as a group names it. Text is itself; a number or a flag that
/// came back as one is written out.
fn text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        other => Some(other.to_string()),
    }
}

fn position(columns: &[String], name: &str) -> Option<usize> {
    columns.iter().position(|column| column == name)
}

fn required(columns: &[String], name: &str) -> Result<usize> {
    position(columns, name)
        .ok_or_else(|| Error::internal(format!("RED statement returned no `{name}` column")))
}
