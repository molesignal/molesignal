// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Moves of a service's errors, latency or traffic: against the stretch of
//! the same length before, and against the same stretch a week before, so
//! that a daily or weekly rhythm is not taken for a move. A move holds
//! against each stretch with enough requests to judge by, and at least one
//! has them. It began where the buckets of the range first stood past the
//! line halfway between the stricter baseline and now two buckets running,
//! and mostly stayed: a single bad bucket is not a start.

use super::{
    super::{RedSummary, RedTrendPoint, ServiceHealth, health::assess_health},
    model::{Insight, InsightKind, InsightSeverity},
};
use crate::{
    domain::apm::{HealthLines, ServiceIdentity},
    shared::time::TimestampMicros,
};

/// The fewest requests a stretch needs for its error rate or latency to count.
pub(super) const MIN_REQUESTS: u64 = 20;
/// The fewest requests, in the busier stretch, for a change of traffic to count.
const MIN_TRAFFIC_REQUESTS: f64 = 100.0;

/// A service's figures over one stretch, and how long the buckets read cover.
#[derive(Clone, Copy)]
pub(super) struct Stretch<'a> {
    pub red: &'a RedSummary,
    pub seconds: f64,
}

impl Stretch<'_> {
    fn judged(&self) -> bool {
        self.red.request_count >= MIN_REQUESTS
    }

    fn throughput(&self) -> f64 {
        self.red.request_count as f64 / self.seconds.max(1e-9)
    }
}

pub(super) struct ServiceFigures<'a> {
    pub service: &'a ServiceIdentity,
    pub now: Stretch<'a>,
    pub previous: Option<Stretch<'a>>,
    pub last_week: Option<Stretch<'a>>,
    /// The range a bucket at a time, and how long a bucket is.
    pub trend: &'a [RedTrendPoint],
    pub bucket_seconds: f64,
}

#[derive(Clone, Copy, PartialEq)]
enum Metric {
    ErrorRate,
    P95,
    Throughput,
}

impl Metric {
    fn name(self) -> &'static str {
        match self {
            Self::ErrorRate => "error_rate",
            Self::P95 => "p95",
            Self::Throughput => "throughput",
        }
    }

    fn of(self, stretch: &Stretch<'_>) -> Option<f64> {
        match self {
            Self::ErrorRate => stretch.judged().then_some(stretch.red.error_rate),
            Self::P95 => stretch
                .judged()
                .then_some(stretch.red.p95_time)
                .flatten()
                .map(|micros| micros as f64),
            Self::Throughput => Some(stretch.throughput()),
        }
    }

    /// Whether `now` moved against `before`, both said in this figure.
    fn moved(self, now: &Stretch<'_>, before: &Stretch<'_>) -> Option<bool> {
        match self {
            Self::ErrorRate => {
                let (is, was) = (self.of(now)?, self.of(before)?);
                Some(is - was >= 0.02 && is >= was * 2.0)
            }
            Self::P95 => {
                let (is, was) = (self.of(now)?, self.of(before)?);
                Some(is >= was * 1.5 && is - was >= 100_000.0)
            }
            Self::Throughput => {
                let (is, was) = (now.throughput(), before.throughput());
                // The stretch before told in the length of this one.
                let busier = (is * now.seconds).max(was * now.seconds);

                (busier >= MIN_TRAFFIC_REQUESTS && was > 0.0)
                    .then(|| !(0.5..2.0).contains(&(is / was)))
            }
        }
    }

    /// The figure of one bucket, when it has requests to tell it by.
    fn at(self, point: &RedTrendPoint, bucket_seconds: f64) -> Option<f64> {
        match self {
            Self::ErrorRate => (point.red.request_count > 0).then_some(point.red.error_rate),
            Self::P95 => point.red.p95_time.map(|micros| micros as f64),
            Self::Throughput => Some(point.red.request_count as f64 / bucket_seconds.max(1e-9)),
        }
    }
}

/// The services of `figures` whose errors, latency or traffic moved: errors
/// first, then latency, then traffic, the furthest moved first in each.
pub(super) fn anomalies(figures: &[ServiceFigures<'_>], lines: &HealthLines) -> Vec<Insight> {
    let mut found = Vec::new();

    for metric in [Metric::ErrorRate, Metric::P95, Metric::Throughput] {
        let mut moved = figures
            .iter()
            .filter_map(|service| anomaly(service, metric, lines))
            .collect::<Vec<_>>();

        moved.sort_by(|left, right| change(right).total_cmp(&change(left)));
        found.extend(moved);
    }

    found
}

fn change(insight: &Insight) -> f64 {
    let base = insight.previous.or(insight.last_week).unwrap_or(0.0);

    (insight.value - base).abs() / base.abs().max(f64::EPSILON)
}

fn anomaly(service: &ServiceFigures<'_>, metric: Metric, lines: &HealthLines) -> Option<Insight> {
    let baselines = [service.previous, service.last_week];
    let judged = baselines
        .iter()
        .flatten()
        .filter_map(|before| metric.moved(&service.now, before))
        .collect::<Vec<_>>();

    if judged.is_empty() || judged.contains(&false) {
        return None;
    }

    let value = metric.of(&service.now)?;
    let previous = service.previous.and_then(|before| metric.of(&before));
    let last_week = service.last_week.and_then(|before| metric.of(&before));
    let severity = match assess_health(service.now.red, lines).0 {
        ServiceHealth::Critical => InsightSeverity::Critical,
        _ => InsightSeverity::Warning,
    };

    let mut insight = Insight::new(
        InsightKind::Anomaly,
        severity,
        service.service.clone(),
        value,
    );
    insight.metric = Some(metric.name());
    insight.previous = previous;
    insight.last_week = last_week;
    insight.since = onset(
        service.trend,
        service.bucket_seconds,
        metric,
        value,
        [previous, last_week].into_iter().flatten(),
    );

    Some(insight)
}

/// The first bucket past the line halfway between the stricter baseline and
/// `value`, with the next one past it too, from which at least half the
/// buckets with a figure stay past it.
fn onset(
    trend: &[RedTrendPoint],
    bucket_seconds: f64,
    metric: Metric,
    value: f64,
    baselines: impl Iterator<Item = f64>,
) -> Option<TimestampMicros> {
    let baselines = baselines.collect::<Vec<_>>();
    let rising = baselines.iter().all(|base| value >= *base);
    let reference = if rising {
        baselines.iter().copied().fold(f64::MIN, f64::max)
    } else {
        baselines.iter().copied().fold(f64::MAX, f64::min)
    };

    if baselines.is_empty() {
        return None;
    }

    let line = reference + (value - reference) / 2.0;
    let past = |figure: f64| {
        if rising {
            figure >= line
        } else {
            figure <= line
        }
    };
    let figures = trend
        .iter()
        .filter_map(|point| {
            metric
                .at(point, bucket_seconds)
                .map(|figure| (point.bucket_at, figure))
        })
        .collect::<Vec<_>>();

    (0..figures.len()).find_map(|start| {
        let rest = &figures[start..];
        let beyond = rest.iter().filter(|(_, figure)| past(*figure)).count();

        let held = rest.get(1).is_none_or(|next| past(next.1));

        (past(rest[0].1) && held && beyond * 2 >= rest.len()).then_some(rest[0].0)
    })
}
