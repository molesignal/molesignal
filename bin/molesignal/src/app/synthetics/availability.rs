// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Availability and response times over a time range, read with SQL from the organization's
//! `_synthetic_results` stream: in all, by check, by location and in steps of time.
//!
//! A run that passed, flapped or slowed was available; one that failed was not; one that could
//! not be judged or never ran (unknown, skipped) counts toward neither.

use serde::Serialize;
use serde_json::Value;

use super::RESULT_STREAM;
use crate::{
    domain::query::QueryResult,
    shared::{Error, Result, ids::Id},
};

const MINUTE: i64 = 60 * 1_000_000;

/// The steps a range is cut into, the first that gives no more than [`TARGET_BUCKETS`].
const STEPS: [i64; 10] = [
    MINUTE,
    5 * MINUTE,
    15 * MINUTE,
    30 * MINUTE,
    60 * MINUTE,
    3 * 60 * MINUTE,
    6 * 60 * MINUTE,
    12 * 60 * MINUTE,
    24 * 60 * MINUTE,
    7 * 24 * 60 * MINUTE,
];

const TARGET_BUCKETS: i64 = 96;

/// Whatever step is asked for, a range is cut into no more steps than this.
const MAX_BUCKETS: i64 = 500;

/// The longest range asked about at once.
const MAX_RANGE: i64 = 400 * 24 * 60 * MINUTE;

/// What is asked about: a range, the step to cut it in, and the runs of one check or location.
#[derive(Debug, Clone)]
pub struct AvailabilityQuery {
    pub from: i64,
    pub to: i64,
    pub step: i64,
    pub monitor_id: Option<Id>,
    pub location_id: Option<Id>,
    /// Whether the stream has had a duration yet: a column never written cannot be read.
    pub durations: bool,
}

impl AvailabilityQuery {
    pub fn new(
        from: i64,
        to: i64,
        step_seconds: Option<u32>,
        monitor_id: Option<Id>,
        location_id: Option<Id>,
        durations: bool,
    ) -> Result<Self> {
        if from >= to {
            return Err(Error::invalid("the range has to end after it starts"));
        }

        if to - from > MAX_RANGE {
            return Err(Error::invalid("the range can span at most 400 days"));
        }

        for id in monitor_id.iter().chain(&location_id) {
            if id.0.is_empty()
                || id.0.len() > 64
                || !id
                    .0
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte))
            {
                return Err(Error::invalid(format!("`{}` is not an id", id.0)));
            }
        }

        Ok(Self {
            from,
            to,
            step: step(to - from, step_seconds),
            monitor_id,
            location_id,
            durations,
        })
    }

    pub fn total_sql(&self) -> String {
        format!(
            "SELECT {} FROM {RESULT_STREAM} WHERE {}",
            self.measures(),
            self.filter()
        )
    }

    pub fn checks_sql(&self) -> String {
        self.grouped_sql("check_id")
    }

    pub fn locations_sql(&self) -> String {
        self.grouped_sql("location_id")
    }

    /// The whole of what is asked about, step by step.
    pub fn buckets_sql(&self) -> String {
        format!(
            "SELECT {bucket} AS bucket, {} FROM {RESULT_STREAM} WHERE {} \
             GROUP BY {bucket} ORDER BY bucket",
            self.measures(),
            self.filter(),
            bucket = self.bucket(),
        )
    }

    /// Each location, step by step: the runs of one check from each place it runs.
    pub fn location_buckets_sql(&self) -> Option<String> {
        self.monitor_id.as_ref()?;

        Some(format!(
            "SELECT location_id, {bucket} AS bucket, {} FROM {RESULT_STREAM} WHERE {} \
             GROUP BY location_id, {bucket} ORDER BY location_id, bucket",
            self.measures(),
            self.filter(),
            bucket = self.bucket(),
        ))
    }

    fn grouped_sql(&self, key: &str) -> String {
        format!(
            "SELECT {key}, {} FROM {RESULT_STREAM} WHERE {} GROUP BY {key}",
            self.measures(),
            self.filter()
        )
    }

    fn bucket(&self) -> String {
        format!(
            "(CAST(_timestamp AS BIGINT) - {}) / {}",
            self.from, self.step
        )
    }

    fn filter(&self) -> String {
        let mut filter = format!(
            "CAST(_timestamp AS BIGINT) >= {} AND CAST(_timestamp AS BIGINT) < {}",
            self.from, self.to
        );

        // Ids are checked to be plain words before they come anywhere near the statement.
        if let Some(monitor_id) = &self.monitor_id {
            filter.push_str(&format!(" AND check_id = '{}'", monitor_id.0));
        }

        if let Some(location_id) = &self.location_id {
            filter.push_str(&format!(" AND location_id = '{}'", location_id.0));
        }

        filter
    }

    fn measures(&self) -> String {
        let duration = "CAST(duration_ms AS DOUBLE)";

        let durations = if self.durations {
            format!(
                "approx_percentile_cont(0.5) WITHIN GROUP (ORDER BY {duration}) AS p50_ms, \
                 approx_percentile_cont(0.95) WITHIN GROUP (ORDER BY {duration}) AS p95_ms, \
                 max({duration}) AS max_ms"
            )
        } else {
            "CAST(NULL AS DOUBLE) AS p50_ms, CAST(NULL AS DOUBLE) AS p95_ms, \
             CAST(NULL AS DOUBLE) AS max_ms"
                .to_string()
        };

        format!(
            "count(*) AS runs, \
             sum(CASE WHEN outcome IN ('healthy', 'flaky', 'degraded') THEN 1 ELSE 0 END) \
             AS available, \
             sum(CASE WHEN outcome = 'failing' THEN 1 ELSE 0 END) AS failing, \
             {durations}"
        )
    }
}

/// The step a range is cut into: the one asked for, or the first that gives a readable number
/// of steps; never so short that there are more than [`MAX_BUCKETS`].
fn step(span: i64, asked_seconds: Option<u32>) -> i64 {
    let fewest = span / MAX_BUCKETS + i64::from(span % MAX_BUCKETS != 0);

    let chosen = match asked_seconds {
        Some(seconds) => i64::from(seconds.max(60)) * 1_000_000,
        None => STEPS
            .into_iter()
            .find(|step| span / step <= TARGET_BUCKETS)
            .unwrap_or(STEPS[STEPS.len() - 1]),
    };

    chosen.max(fewest)
}

/// How a set of runs went.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub runs: u64,
    pub available: u64,
    pub failing: u64,
    /// Runs that could not be judged or never ran.
    pub undecided: u64,
    /// Available runs of those judged; none when no run was.
    pub availability: Option<f64>,
    pub p50_ms: Option<f64>,
    pub p95_ms: Option<f64>,
    pub max_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckSummary {
    pub check_id: String,
    #[serde(flatten)]
    pub summary: Summary,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationSummary {
    pub location_id: String,
    #[serde(flatten)]
    pub summary: Summary,
}

#[derive(Debug, Clone, Serialize)]
pub struct Bucket {
    /// When the step starts, in microseconds.
    pub start: i64,
    #[serde(flatten)]
    pub summary: Summary,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationBucket {
    pub location_id: String,
    pub start: i64,
    #[serde(flatten)]
    pub summary: Summary,
}

/// What a range says, as the availability endpoint answers it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Availability {
    pub from: i64,
    pub to: i64,
    pub step: i64,
    pub total: Summary,
    pub checks: Vec<CheckSummary>,
    pub locations: Vec<LocationSummary>,
    pub buckets: Vec<Bucket>,
    /// Each location step by step, when one check is asked about.
    pub location_buckets: Vec<LocationBucket>,
}

impl Availability {
    /// A range nothing ran in.
    pub fn empty(query: &AvailabilityQuery) -> Self {
        Self {
            from: query.from,
            to: query.to,
            step: query.step,
            total: Summary::default(),
            checks: Vec::new(),
            locations: Vec::new(),
            buckets: Vec::new(),
            location_buckets: Vec::new(),
        }
    }
}

/// The rows of a query, each as its summary and the values of `keys` before it.
pub fn summaries(result: &QueryResult, keys: &[&str]) -> Vec<(Vec<Value>, Summary)> {
    let column = |name: &str| result.columns.iter().position(|column| column == name);
    let keyed = keys.iter().map(|key| column(key)).collect::<Vec<_>>();

    let (runs, available, failing) = (column("runs"), column("available"), column("failing"));
    let (p50, p95, max) = (column("p50_ms"), column("p95_ms"), column("max_ms"));

    result
        .rows
        .iter()
        .map(|row| {
            let read =
                |index: Option<usize>| index.and_then(|index| row.get(index)).and_then(number);
            let count = |index| read(index).unwrap_or(0.0).max(0.0) as u64;

            let runs = count(runs);
            let available = count(available);
            let failing = count(failing);
            let decided = available + failing;

            let keys = keyed
                .iter()
                .map(|index| {
                    index
                        .and_then(|index| row.get(index))
                        .cloned()
                        .unwrap_or(Value::Null)
                })
                .collect();

            let summary = Summary {
                runs,
                available,
                failing,
                undecided: runs.saturating_sub(decided),
                availability: (decided > 0).then(|| available as f64 / decided as f64),
                p50_ms: read(p50),
                p95_ms: read(p95),
                max_ms: read(max),
            };

            (keys, summary)
        })
        .collect()
}

/// A number the query engine answered, whether as a number or as its text.
fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.parse().ok(),
        _ => None,
    }
}

/// A key value as text: an id, whatever type the engine gave it.
pub fn text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// The start of a step, from its index.
pub fn bucket_start(query: &AvailabilityQuery, value: &Value) -> i64 {
    let index = number(value).unwrap_or(0.0) as i64;
    query.from.saturating_add(index.saturating_mul(query.step))
}

#[cfg(test)]
mod tests;
