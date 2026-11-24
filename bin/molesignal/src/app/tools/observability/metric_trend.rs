// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `analyze_metric_trend`: how one PromQL query moved in a window against a baseline stretch, and
//! what the move came from.
//!
//! The series are read with the range query `query_metrics` runs. The whole is their sum at each
//! step; when the series did not all fit in [`POINT_LIMIT`] points, the sum is read again as
//! `sum(…)`. The window's level is held against the baseline's spread with the MAD detector of
//! the anomaly rules; the change point is the one [`stats::change_point`] finds. A series' move
//! compares its mean in the window with its mean in the baseline, or its mean after the change
//! point with its mean before it when there is no baseline to compare with.

mod contributions;
mod stats;

use std::collections::BTreeMap;

use domain::{
    alerting::anomaly::{AnomalyDetector, MadDetector},
    iam::IamContext,
    query::QueryResult,
};
use kernel::{Error, Result, ids::Id, time::TimeRange};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use tool_runtime::ToolResult;

use self::{
    contributions::{
        LabelMove, Labels, SeriesMove, label_moves, series_moves, share, split_labels,
    },
    stats::{change_point, direction, mean, median, num, opt_num, relative, robust_sigma},
};
use super::{
    super::{ToolRuntime, common::*},
    DEFAULT_METRIC_STEPS,
};
use crate::app::query::baseline::Baseline;

/// The most points read of one range query.
const POINT_LIMIT: usize = 20_000;

/// A series is read at no more than this many steps.
const MAX_STEPS: i64 = 500;

const DEFAULT_LIMIT: usize = 10;
const MAX_LIMIT: usize = 50;
const MAX_SPLIT_LABELS: usize = 5;

/// The most values told of one label.
const VALUES_PER_LABEL: usize = 5;

/// The window's level has moved when it is this many robust standard deviations from the
/// baseline's.
const MOVED_SIGMAS: f64 = 3.0;

const METHOD: &str = "The whole is the sum of the series at each step; its level is the median \
and the baseline's spread is 1.4826 times the median absolute deviation. The change point is the \
split of the window whose medians before and after differ the most, in robust standard deviations \
of the values around their own side's median; it is significant from 3. A series' move is its mean \
after less its mean before, a missing side counting as zero.";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Args {
    promql: String,
    time_range: TimeRangeArg,
    #[serde(default)]
    baseline: Baseline,
    #[serde(default)]
    step_seconds: Option<u64>,
    #[serde(default)]
    split_by: Vec<String>,
    #[serde(default)]
    limit: Option<usize>,
}

/// One series of a range query: its labels and its points in time order.
struct Series {
    labels: Labels,
    points: Vec<(i64, f64)>,
}

/// What one range query read: its series, their sum at each step, and whether the point limit
/// cut points away.
struct Reading {
    range: TimeRange,
    series: Vec<Series>,
    whole: Vec<(i64, f64)>,
    truncated: bool,
}

/// What was asked, besides the readings.
struct Ask<'a> {
    promql: &'a str,
    step: i64,
    baseline: Baseline,
    split_by: &'a [String],
    limit: usize,
}

pub(super) async fn analyze(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: Args = parse_args(arguments)?;
    let promql = args.promql.trim();

    if promql.is_empty() {
        return Err(Error::invalid("promql is required"));
    }

    if promql.to_ascii_lowercase().contains("label_values(") {
        return Ok(ToolResult::error(
            "label_values() is not PromQL; use list_metric_label_values",
        ));
    }

    if args.split_by.len() > MAX_SPLIT_LABELS {
        return Err(Error::invalid(format!(
            "splitBy names at most {MAX_SPLIT_LABELS} labels"
        )));
    }

    let window = time_range(args.time_range)?;
    let step = step_for(window, args.step_seconds);
    let current = read(runtime, &auth.org_id, promql, window, step).await?;

    let baseline = match args.baseline.range(window) {
        Some(range) => Some(read(runtime, &auth.org_id, promql, range, step).await?),
        None => None,
    };

    let ask = Ask {
        promql,
        step,
        baseline: args.baseline,
        split_by: &args.split_by,
        limit: args.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT),
    };

    Ok(ToolResult::json(report(&ask, &current, baseline.as_ref())))
}

/// The step the window is read at: the one asked for, or a hundredth of the window; never more
/// than [`MAX_STEPS`] steps, never less than a second.
fn step_for(window: TimeRange, step_seconds: Option<u64>) -> i64 {
    let span = window.end.0.saturating_sub(window.start.0);

    let asked = match step_seconds {
        Some(seconds) => i64::try_from(seconds)
            .unwrap_or(i64::MAX)
            .saturating_mul(1_000_000),
        None => span / DEFAULT_METRIC_STEPS,
    };

    asked.max(span / MAX_STEPS).max(1_000_000)
}

async fn read(
    runtime: &ToolRuntime,
    org_id: &Id,
    promql: &str,
    range: TimeRange,
    step: i64,
) -> Result<Reading> {
    let result = run_metric_range(
        runtime,
        org_id,
        promql.to_string(),
        range,
        step,
        POINT_LIMIT,
    )
    .await?;

    let truncated = result.rows.len() >= POINT_LIMIT;
    let series = read_series(&result);

    // A cut reading lacks points of some series at some steps: their sum is read whole instead.
    let whole = if truncated {
        match run_metric_range(
            runtime,
            org_id,
            format!("sum({promql})"),
            range,
            step,
            POINT_LIMIT,
        )
        .await
        {
            Ok(summed) => read_series(&summed)
                .into_iter()
                .next()
                .map(|series| series.points)
                .unwrap_or_default(),
            Err(error) => {
                tracing::debug!(%error, "the sum of a cut metric reading could not be read");
                sum_by_step(&series)
            }
        }
    } else {
        sum_by_step(&series)
    };

    Ok(Reading {
        range,
        series,
        whole,
        truncated,
    })
}

/// The series of a range result, laid out as `[_timestamp, value, ...labels]`.
fn read_series(result: &QueryResult) -> Vec<Series> {
    let position = |name: &str| result.columns.iter().position(|column| column == name);

    let (Some(at_index), Some(value_index)) = (position("_timestamp"), position("value")) else {
        return Vec::new();
    };

    let mut series = BTreeMap::<Labels, Vec<(i64, f64)>>::new();

    for row in &result.rows {
        let at = row.get(at_index).and_then(Value::as_i64);
        let value = row.get(value_index).and_then(Value::as_f64);

        let (Some(at), Some(value)) = (at, value.filter(|value| value.is_finite())) else {
            continue;
        };

        let labels = result
            .columns
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != at_index && *index != value_index)
            .filter_map(|(index, name)| {
                row.get(index)
                    .and_then(Value::as_str)
                    .map(|value| (name.clone(), value.to_string()))
            })
            .collect::<Labels>();

        series.entry(labels).or_default().push((at, value));
    }

    series
        .into_iter()
        .map(|(labels, mut points)| {
            points.sort_by_key(|(at, _)| *at);
            Series { labels, points }
        })
        .collect()
}

fn sum_by_step(series: &[Series]) -> Vec<(i64, f64)> {
    let mut sums = BTreeMap::<i64, f64>::new();

    for (at, value) in series.iter().flat_map(|series| series.points.iter()) {
        *sums.entry(*at).or_default() += value;
    }

    sums.into_iter().collect()
}

/// Each series' mean over its points that `keep` keeps; a series with none kept is left out.
fn means(series: &[Series], keep: impl Fn(i64) -> bool) -> BTreeMap<Labels, f64> {
    series
        .iter()
        .filter_map(|series| {
            let values = series
                .points
                .iter()
                .filter(|(at, _)| keep(*at))
                .map(|(_, value)| *value)
                .collect::<Vec<_>>();

            mean(&values).map(|average| (series.labels.clone(), average))
        })
        .collect()
}

fn values_of(points: &[(i64, f64)]) -> Vec<f64> {
    points.iter().map(|(_, value)| *value).collect()
}

fn report(ask: &Ask<'_>, current: &Reading, baseline: Option<&Reading>) -> Value {
    let mut out = Map::new();
    out.insert("promql".into(), json!(ask.promql));

    out.insert(
        "timeRange".into(),
        json!({"startTime": current.range.start.0, "endTime": current.range.end.0}),
    );

    out.insert("stepSeconds".into(), json!(ask.step / 1_000_000));
    out.insert("seriesCount".into(), json!(current.series.len()));
    out.insert("truncated".into(), json!(current.truncated));

    let values = values_of(&current.whole);

    let Some(level) = median(&values) else {
        out.insert(
            "note".into(),
            json!("The query returned no data in the window."),
        );

        return Value::Object(out);
    };

    out.insert("whole".into(), whole(current, &values, level));

    if let Some(reading) = baseline {
        out.insert("baseline".into(), against_baseline(ask, reading, level));
    }

    let point = change_point(&values);

    out.insert(
        "changePoint".into(),
        point.as_ref().map_or(Value::Null, |point| {
            json!({
                "at": current.whole[point.index].0,
                "before": num(point.before),
                "after": num(point.after),
                "delta": num(point.after - point.before),
                "relative": opt_num(relative(point.before, point.after)),
                "direction": direction(point.before, point.after),
                "strength": num(point.strength),
                "significant": point.significant(),
            })
        }),
    );

    // What the move came from: against the baseline when there is one with data, else before
    // and after a significant change point.
    let sides = match baseline.filter(|reading| !reading.series.is_empty()) {
        Some(reading) => Some((
            "baseline",
            means(&reading.series, |_| true),
            means(&current.series, |_| true),
        )),
        None => point
            .as_ref()
            .filter(|point| point.significant())
            .map(|point| {
                let at = current.whole[point.index].0;

                (
                    "before_change",
                    means(&current.series, |time| time < at),
                    means(&current.series, |time| time >= at),
                )
            }),
    };

    let contributions = match sides {
        Some((compared_with, before, after)) => {
            attribute(ask, compared_with, &series_moves(&before, &after))
        }
        None => json!({
            "note": "There is no baseline with data and no significant change point to attribute a move to.",
        }),
    };

    out.insert("contributions".into(), contributions);
    out.insert("method".into(), json!(METHOD));

    Value::Object(out)
}

fn whole(current: &Reading, values: &[f64], level: f64) -> Value {
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);

    json!({
        "aggregate": if current.series.len() > 1 { "sum" } else { "series" },
        "points": values.len(),
        "median": num(level),
        "min": num(min),
        "max": num(max),
        "last": opt_num(values.last().copied()),
    })
}

fn against_baseline(ask: &Ask<'_>, reading: &Reading, level: f64) -> Value {
    let values = values_of(&reading.whole);
    let mut out = Map::new();

    out.insert("kind".into(), json!(ask.baseline.as_str()));
    out.insert("startTime".into(), json!(reading.range.start.0));
    out.insert("endTime".into(), json!(reading.range.end.0));
    out.insert("points".into(), json!(values.len()));

    let Some(center) = median(&values) else {
        out.insert(
            "note".into(),
            json!("The query returned no data in the baseline."),
        );
        return Value::Object(out);
    };

    let spread = robust_sigma(&values, center);
    let decision = MadDetector::new(MOVED_SIGMAS).detect(level, &values);

    out.insert("median".into(), num(center));
    out.insert("spread".into(), num(spread));
    out.insert("delta".into(), num(level - center));
    out.insert("relative".into(), opt_num(relative(center, level)));

    out.insert(
        "sigmas".into(),
        if spread > 0.0 {
            num((level - center).abs() / spread)
        } else {
            Value::Null
        },
    );

    out.insert("moved".into(), json!(decision.firing));
    out.insert("direction".into(), json!(direction(center, level)));

    Value::Object(out)
}

/// How much each series and each label value carried of the move of the whole.
fn attribute(ask: &Ask<'_>, compared_with: &str, moves: &[SeriesMove]) -> Value {
    let total = moves.iter().map(|series| series.delta).sum::<f64>();

    let known = moves
        .iter()
        .flat_map(|series| series.labels.keys())
        .collect::<std::collections::BTreeSet<_>>();

    let (labels, unknown): (Vec<String>, Vec<String>) = if ask.split_by.is_empty() {
        (split_labels(moves), Vec::new())
    } else {
        ask.split_by
            .iter()
            .cloned()
            .partition(|label| known.contains(label))
    };

    let series = moves
        .iter()
        .take(ask.limit)
        .map(|series| {
            json!({
                "labels": series.labels,
                "before": opt_num(series.before),
                "after": opt_num(series.after),
                "delta": num(series.delta),
                "share": opt_num(share(series.delta, total)),
            })
        })
        .collect::<Vec<_>>();

    let by_label = label_moves(moves, &labels, VALUES_PER_LABEL.min(ask.limit))
        .iter()
        .map(|label| label_json(label, total))
        .collect::<Vec<_>>();

    let mut out = Map::new();
    out.insert("comparedWith".into(), json!(compared_with));
    out.insert("delta".into(), num(total));
    out.insert("series".into(), json!(series));
    out.insert("byLabel".into(), json!(by_label));

    if !unknown.is_empty() {
        out.insert("unknownLabels".into(), json!(unknown));
    }

    Value::Object(out)
}

fn label_json(label: &LabelMove, total: f64) -> Value {
    json!({
        "label": label.label,
        "concentration": num(label.concentration),
        "values": label.values.iter().map(|value| json!({
            "value": value.value,
            "before": num(value.before),
            "after": num(value.after),
            "delta": num(value.delta),
            "share": opt_num(share(value.delta, total)),
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests;
