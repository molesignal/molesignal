// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The metrics `generate_metric` steps make, added up in memory until their interval is over.
//!
//! - An event counts in the interval of its own time, not of its arrival. An interval waits
//!   [`GRACE_MICROS`] after it ends for events still on their way, then its samples are written.
//! - An event that comes later than that adds to a new part of its interval, written with the next
//!   ones. Each part of an interval a node writes has a timestamp of its own (see
//!   [`GeneratedMetrics::new`]), so PromQL keeps every part and `increase` adds them up; so does a
//!   part another node wrote for the same interval.
//! - A step keeps at most [`MAX_SERIES`] series apart per interval on a node; the events of the
//!   others go into one series labelled [`OTHER_SERIES`](crate::domain::pipeline::metric::OTHER_SERIES).
//! - What is not written yet is lost when a node crashes; a node that stops writes it first.
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    time::{SystemTime, UNIX_EPOCH},
};

use parking_lot::Mutex;
use serde_json::{Map, Value};

use crate::{
    domain::{
        intake::RawEvent,
        metrics::{
            METRIC_KIND_FIELD, METRIC_MONOTONIC_FIELD, METRIC_NAME_FIELD,
            METRIC_START_TIME_UNIX_NANO_FIELD, METRIC_TEMPORALITY_FIELD,
        },
        pipeline::metric::{MAX_SERIES, MetricSpec, MetricType},
    },
    shared::{ids::Id, time::TimestampMicros},
};

/// How long an interval waits after its end for events still on their way.
pub const GRACE_MICROS: i64 = 5_000_000;
/// Events older than this are not counted: their interval was written long ago, and the stream may
/// no longer keep it.
pub const MAX_AGE_MICROS: i64 = 24 * 3_600 * 1_000_000;
/// Events this far ahead of the node's clock are not counted.
pub const MAX_AHEAD_MICROS: i64 = 5 * 60 * 1_000_000;
/// The parts a node writes of one interval sit in the last half second of it.
const SPREAD_MICROS: i64 = 500_000;

/// The step a series belongs to, for its cap on series.
#[derive(Clone, PartialEq, Eq, Hash)]
struct StepKey {
    pipeline: Id,
    step: String,
    start: i64,
}

/// One series in one interval.
#[derive(Clone, PartialEq, Eq, Hash)]
struct SeriesKey {
    org: Id,
    stream: String,
    name: String,
    /// By label name.
    labels: Vec<(String, String)>,
    start: i64,
    width: i64,
}

/// One metric of one interval, as the parts a node writes are told apart.
type PartKey = (Id, String, String, i64);

enum Tally {
    Sum {
        total: f64,
        monotonic: bool,
    },
    Last {
        at: i64,
        value: f64,
    },
    Histogram {
        bounds: Vec<f64>,
        counts: Vec<u64>,
        sum: f64,
    },
}

/// The samples of one organization's metrics stream.
pub struct MetricBatch {
    pub org: Id,
    pub stream: String,
    pub samples: Vec<RawEvent>,
}

/// What one event adds to the metric of a step, held until its batch is written.
pub struct Counted<'a> {
    org: &'a Id,
    pipeline: &'a Id,
    step: &'a str,
    spec: &'a MetricSpec,
    /// By label name.
    labels: Vec<(String, String)>,
    amount: f64,
    at: i64,
}

impl<'a> Counted<'a> {
    /// What `event`, whose time is `at`, adds to the metric of a step. An event without a number in
    /// the field the metric reads, or with a time too far from `now`, adds nothing.
    #[allow(clippy::too_many_arguments)]
    pub fn of(
        org: &'a Id,
        pipeline: &'a Id,
        step: &'a str,
        spec: &'a MetricSpec,
        event: &Value,
        at: TimestampMicros,
        now: TimestampMicros,
    ) -> Option<Self> {
        let amount = spec.measure(event)?;

        if at.0 < now.0 - MAX_AGE_MICROS || at.0 > now.0 + MAX_AHEAD_MICROS {
            return None;
        }

        let mut labels = spec.labels(event);
        labels.sort();

        Some(Self {
            org,
            pipeline,
            step,
            spec,
            labels,
            amount,
            at: at.0,
        })
    }
}

#[derive(Default)]
struct Open {
    series: HashMap<SeriesKey, (StepKey, Tally)>,
    per_step: HashMap<StepKey, usize>,
    /// How many parts of each metric's interval this node has written.
    written: HashMap<PartKey, i64>,
}

pub struct GeneratedMetrics {
    open: Mutex<Open>,
    /// Where in the last half second of an interval this process writes its first part.
    base: i64,
}

impl Default for GeneratedMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl GeneratedMetrics {
    /// Every process starts its parts at an offset of its own, drawn as it starts: two nodes, or a
    /// node and itself before a restart, write the same interval at the same microsecond about
    /// once in half a million times.
    pub fn new() -> Self {
        let clock = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos() as u64);
        let mixed = (clock ^ u64::from(std::process::id()).rotate_left(32))
            .wrapping_mul(0x9E37_79B9_7F4A_7C15);

        Self::with_base((mixed % SPREAD_MICROS as u64) as i64)
    }

    pub(crate) fn with_base(base: i64) -> Self {
        Self {
            open: Mutex::new(Open::default()),
            base: base.rem_euclid(SPREAD_MICROS),
        }
    }

    /// Adds what events counted to their metrics. A batch's events are added once the batch is
    /// written, so a batch that fails and is sent again is not counted twice.
    pub fn record<'a>(&self, counted: impl IntoIterator<Item = Counted<'a>>) {
        let mut open = self.open.lock();

        for counted in counted {
            let width = i64::from(counted.spec.interval_secs) * 1_000_000;
            let start = counted.at.div_euclid(width) * width;

            let step = StepKey {
                pipeline: counted.pipeline.clone(),
                step: counted.step.to_owned(),
                start,
            };

            let mut key = SeriesKey {
                org: counted.org.clone(),
                stream: counted.spec.stream.clone(),
                name: counted.spec.name.clone(),
                labels: counted.labels,
                start,
                width,
            };

            if !open.series.contains_key(&key) {
                let count = open.per_step.entry(step.clone()).or_default();

                if *count >= MAX_SERIES {
                    key.labels = counted.spec.other_labels();
                    key.labels.sort();
                } else {
                    *count += 1;
                }
            }

            open.series
                .entry(key)
                .or_insert_with(|| (step, Tally::of(counted.spec)))
                .1
                .add(counted.amount, counted.at);
        }
    }

    /// The samples of every interval that ended [`GRACE_MICROS`] before `now`, which are forgotten.
    pub fn take_due(&self, now: TimestampMicros) -> Vec<MetricBatch> {
        self.take(now, |key| key.start + key.width + GRACE_MICROS <= now.0)
    }

    /// The samples of every interval, over or not: a node that stops writes them all.
    pub fn take_all(&self, now: TimestampMicros) -> Vec<MetricBatch> {
        self.take(now, |_| true)
    }

    fn take(&self, now: TimestampMicros, due: impl Fn(&SeriesKey) -> bool) -> Vec<MetricBatch> {
        let mut open = self.open.lock();

        let keys: Vec<SeriesKey> = open.series.keys().filter(|key| due(key)).cloned().collect();

        if keys.is_empty() {
            return Vec::new();
        }

        // Every part gets its own place in the interval: the same metric and interval written
        // again moves one microsecond earlier.
        let parts: HashSet<PartKey> = keys.iter().map(part_of).collect();
        let mut offsets = HashMap::with_capacity(parts.len());

        for part in parts {
            let written = open.written.entry(part.clone()).or_default();
            offsets.insert(part, self.base + *written);
            *written += 1;
        }

        let mut batches: BTreeMap<(String, String), MetricBatch> = BTreeMap::new();

        for key in keys {
            let Some((step, tally)) = open.series.remove(&key) else {
                continue;
            };

            open.per_step.remove(&step);

            let offset = offsets.get(&part_of(&key)).copied().unwrap_or(self.base);
            let at = key.start + key.width - 1 - offset.min(key.width / 2);

            batches
                .entry((key.org.0.clone(), key.stream.clone()))
                .or_insert_with(|| MetricBatch {
                    org: key.org.clone(),
                    stream: key.stream.clone(),
                    samples: Vec::new(),
                })
                .samples
                .extend(tally.samples(&key, at));
        }

        // A part of an interval that comes after a day is not counted, so its count can go.
        open.written
            .retain(|(_, _, _, start), _| *start >= now.0 - MAX_AGE_MICROS - 3_600_000_000);

        batches.into_values().collect()
    }

    /// Whether nothing waits to be written.
    pub fn is_empty(&self) -> bool {
        self.open.lock().series.is_empty()
    }
}

fn part_of(key: &SeriesKey) -> PartKey {
    (
        key.org.clone(),
        key.stream.clone(),
        key.name.clone(),
        key.start,
    )
}

impl Tally {
    fn of(spec: &MetricSpec) -> Self {
        match spec.kind {
            MetricType::Count => Self::Sum {
                total: 0.0,
                monotonic: spec.counts_events(),
            },
            MetricType::Gauge => Self::Last {
                at: i64::MIN,
                value: 0.0,
            },
            MetricType::Histogram => Self::Histogram {
                bounds: spec.buckets.clone(),
                counts: vec![0; spec.buckets.len() + 1],
                sum: 0.0,
            },
        }
    }

    fn add(&mut self, amount: f64, at: i64) {
        match self {
            Self::Sum { total, .. } => *total += amount,
            Self::Last { at: latest, value } => {
                if at >= *latest {
                    *latest = at;
                    *value = amount;
                }
            }
            Self::Histogram {
                bounds,
                counts,
                sum,
            } => {
                let index = bounds
                    .iter()
                    .position(|bound| amount <= *bound)
                    .unwrap_or(bounds.len());

                counts[index] += 1;
                *sum += amount;
            }
        }
    }

    /// The samples of a series: one for a count or a gauge, a histogram's buckets, count and sum.
    /// A delta sample covers its interval and stands at `at`, in its last half second; a gauge
    /// stands where its latest event did.
    fn samples(&self, key: &SeriesKey, at: i64) -> Vec<RawEvent> {
        match self {
            Self::Sum { total, monotonic } => {
                let kind = if *monotonic {
                    "counter"
                } else {
                    "up_down_counter"
                };

                vec![delta(key, &key.name, kind, *total, *monotonic, None, at)]
            }
            Self::Last { at, value } => {
                let mut fields = base(key, &key.name, "gauge", *value, None);
                fields.remove(METRIC_TEMPORALITY_FIELD);

                vec![RawEvent {
                    timestamp: TimestampMicros(*at),
                    fields,
                }]
            }
            Self::Histogram {
                bounds,
                counts,
                sum,
            } => {
                let bucket = format!("{}_bucket", key.name);
                let mut samples = Vec::with_capacity(bounds.len() + 3);
                let mut cumulative = 0;

                for (index, count) in counts.iter().enumerate() {
                    cumulative += count;

                    let le = bounds
                        .get(index)
                        .map_or_else(|| "+Inf".to_owned(), f64::to_string);

                    samples.push(delta(
                        key,
                        &bucket,
                        "histogram_bucket",
                        cumulative as f64,
                        true,
                        Some(le),
                        at,
                    ));
                }

                let count = format!("{}_count", key.name);
                let total = format!("{}_sum", key.name);

                samples.push(delta(
                    key,
                    &count,
                    "histogram_count",
                    cumulative as f64,
                    true,
                    None,
                    at,
                ));

                samples.push(delta(key, &total, "histogram_sum", *sum, false, None, at));

                samples
            }
        }
    }
}

/// A sample that counts what happened in its interval.
fn delta(
    key: &SeriesKey,
    name: &str,
    kind: &str,
    value: f64,
    monotonic: bool,
    le: Option<String>,
    at: i64,
) -> RawEvent {
    let mut fields = base(key, name, kind, value, le);
    fields.insert(METRIC_MONOTONIC_FIELD.into(), Value::Bool(monotonic));

    fields.insert(
        METRIC_START_TIME_UNIX_NANO_FIELD.into(),
        Value::from(key.start.saturating_mul(1_000)),
    );

    RawEvent {
        timestamp: TimestampMicros(at),
        fields,
    }
}

fn base(
    key: &SeriesKey,
    name: &str,
    kind: &str,
    value: f64,
    le: Option<String>,
) -> Map<String, Value> {
    let mut fields = Map::new();

    for (label, text) in &key.labels {
        fields.insert(label.clone(), Value::String(text.clone()));
    }

    if let Some(le) = le {
        fields.insert("le".into(), Value::String(le));
    }

    fields.insert(METRIC_NAME_FIELD.into(), Value::String(name.to_owned()));
    fields.insert(METRIC_KIND_FIELD.into(), Value::String(kind.to_owned()));
    fields.insert(
        METRIC_TEMPORALITY_FIELD.into(),
        Value::String("delta".into()),
    );
    fields.insert("value".into(), Value::from(value));
    fields
}

#[cfg(test)]
mod tests;
