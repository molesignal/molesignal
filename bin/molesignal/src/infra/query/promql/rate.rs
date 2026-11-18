// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::*;

/// 范围选择器在一个求值时刻读的窗口 `(end - range, end]`，微秒。`end` 已按选择器的 `@` 与
/// `offset` 平移。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RangeWindow {
    pub end_us: i64,
    pub range_us: i64,
}

impl RangeWindow {
    pub fn new(end_us: i64, range: Duration) -> Self {
        Self {
            end_us,
            range_us: (range.as_micros() as i64).max(1),
        }
    }

    fn start_us(self) -> i64 {
        self.end_us.saturating_sub(self.range_us)
    }

    fn seconds(self) -> f64 {
        self.range_us as f64 / 1_000_000.0
    }
}

pub fn apply_rate_like(func: &str, series: Vec<Series>, window: RangeWindow) -> InstantVector {
    let items = series
        .into_iter()
        .filter_map(|s| {
            let metadata = s.metadata_slice(0, s.samples.len());
            let value =
                rate_window_value_with_metadata(func, s.samples.as_slice(), metadata, window)?;
            Some((s.labels, value))
        })
        .collect();

    InstantVector { items }
}

pub(super) fn apply_rate_like_range(
    func: &str,
    series: Vec<Series>,
    range: Duration,
    steps: Steps,
) -> RangeVector {
    let range_us = (range.as_micros() as i64).max(1);
    let mut points = Vec::new();

    for s in series {
        each_step_window_indices(&s.samples, steps, range_us, |t, end, lo, hi| {
            let samples = &s.samples[lo..hi];
            let metadata = s.metadata_slice(lo, hi);
            let window = RangeWindow::new(end, range);

            let Some(value) = rate_window_value_with_metadata(func, samples, metadata, window)
            else {
                return;
            };

            points.push(RangePoint {
                ts_us: t,
                labels: s.labels.clone(),
                value,
            });
        });
    }

    RangeVector { points }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RateSemantics {
    CumulativeCounter,
    CumulativeGauge,
    DeltaCounter,
    DeltaGauge,
}

impl RateSemantics {
    fn from_metadata(metadata: MetricSampleMetadata) -> Self {
        match (metadata.temporality(), metadata.is_monotonic()) {
            (MetricTemporality::Delta, true) => Self::DeltaCounter,
            (MetricTemporality::Delta, false) => Self::DeltaGauge,
            (MetricTemporality::Cumulative, true) => Self::CumulativeCounter,
            (MetricTemporality::Cumulative, false) => Self::CumulativeGauge,
        }
    }

    fn is_delta(self) -> bool {
        matches!(self, Self::DeltaCounter | Self::DeltaGauge)
    }

    fn is_monotonic(self) -> bool {
        matches!(self, Self::CumulativeCounter | Self::DeltaCounter)
    }
}

/// Use only the latest contiguous aggregation mode so a temporality change is
/// not mistaken for either a counter reset or a DELTA sample.
pub(super) fn rate_window_value_with_metadata(
    func: &str,
    samples: &[(i64, f64)],
    metadata: &[MetricSampleMetadata],
    window: RangeWindow,
) -> Option<f64> {
    let (samples, metadata, semantics) = semantic_suffix(samples, metadata);

    if semantics.is_delta() {
        return delta_rate_value(
            func,
            samples,
            metadata,
            semantics.is_monotonic(),
            window.seconds(),
        );
    }

    cumulative_rate_value(func, samples, semantics.is_monotonic(), window)
}

fn semantic_suffix<'a>(
    samples: &'a [(i64, f64)],
    metadata: &'a [MetricSampleMetadata],
) -> (&'a [(i64, f64)], &'a [MetricSampleMetadata], RateSemantics) {
    if samples.is_empty() || metadata.len() != samples.len() {
        return (samples, &[], RateSemantics::CumulativeCounter);
    }

    let semantics = RateSemantics::from_metadata(metadata[metadata.len() - 1]);
    let mut start = metadata.len() - 1;

    while start > 0 && RateSemantics::from_metadata(metadata[start - 1]) == semantics {
        start -= 1;
    }

    (&samples[start..], &metadata[start..], semantics)
}

fn cumulative_rate_value(
    func: &str,
    samples: &[(i64, f64)],
    monotonic: bool,
    window: RangeWindow,
) -> Option<f64> {
    match func {
        "rate" => extrapolated(samples, monotonic, window, true),
        "increase" => extrapolated(samples, monotonic, window, false),
        "irate" => {
            let change = Change::of(&samples[samples.len().checked_sub(2)?..], monotonic)?;
            Some(change.delta / change.seconds())
        }
        _ => None,
    }
}

/// `delta`：窗口里 gauge 的变化，与 `increase` 一样外推到窗口两端，但不按 counter 折算重置。
pub(super) fn extrapolated_delta(samples: &[(i64, f64)], window: RangeWindow) -> Option<f64> {
    extrapolated(samples, false, window, false)
}

/// Prometheus 的 `extrapolatedRate`，计算次序也相同。第一个、最后一个样本离窗口那一端不超过
/// 样本平均间隔的 1.1 倍时，变化按比例延到那一端；离得更远就只延半个平均间隔，当作序列在窗口
/// 里开始或结束。counter 不往前延到负值。`per_second` 时再除以窗口的秒数。
fn extrapolated(
    samples: &[(i64, f64)],
    counter: bool,
    window: RangeWindow,
    per_second: bool,
) -> Option<f64> {
    let change = Change::of(samples, counter)?;
    let sampled = change.seconds();
    let average = sampled / change.steps as f64;
    let threshold = average * 1.1;
    let mut to_start = (change.first_ts - window.start_us()) as f64 / 1_000_000.0;
    let mut to_end = (window.end_us - change.last_ts) as f64 / 1_000_000.0;

    if to_start >= threshold {
        to_start = average / 2.0;
    }

    // 照这个斜率，counter 在窗口起点之后才从 0 涨起时，只延到 0 的那一刻。
    if counter && change.delta > 0.0 && change.first_value >= 0.0 {
        to_start = to_start.min(sampled * (change.first_value / change.delta));
    }

    if to_end >= threshold {
        to_end = average / 2.0;
    }

    let mut factor = (sampled + to_start + to_end) / sampled;

    if per_second {
        factor /= window.seconds();
    }

    Some(change.delta * factor)
}

/// 窗口里第一个到最后一个样本的变化。只看有限值，时间不前进的样本跳过；counter 的值变小是重置，
/// 重置前的值也算进变化。
struct Change {
    first_ts: i64,
    first_value: f64,
    last_ts: i64,
    /// 计入的相邻样本对数。
    steps: usize,
    delta: f64,
}

impl Change {
    fn of(samples: &[(i64, f64)], counter: bool) -> Option<Self> {
        let mut finite = samples
            .iter()
            .copied()
            .filter(|(_, value)| value.is_finite());

        let (first_ts, first_value) = finite.next()?;
        let (mut last_ts, mut last_value) = (first_ts, first_value);
        let mut resets = 0.0;
        let mut steps = 0;

        for (timestamp, value) in finite {
            if timestamp <= last_ts {
                continue;
            }

            if counter && value < last_value {
                resets += last_value;
            }

            (last_ts, last_value) = (timestamp, value);
            steps += 1;
        }

        (steps > 0).then_some(Self {
            first_ts,
            first_value,
            last_ts,
            steps,
            delta: last_value - first_value + resets,
        })
    }

    fn seconds(&self) -> f64 {
        (self.last_ts - self.first_ts) as f64 / 1_000_000.0
    }
}

fn delta_rate_value(
    func: &str,
    samples: &[(i64, f64)],
    metadata: &[MetricSampleMetadata],
    monotonic: bool,
    range_secs: f64,
) -> Option<f64> {
    match func {
        "rate" | "increase" => {
            let mut seen = false;

            let delta = samples
                .iter()
                .zip(metadata)
                .filter_map(|((_, value), _)| {
                    if value.is_finite() && (!monotonic || *value >= 0.0) {
                        seen = true;
                        Some(*value)
                    } else {
                        None
                    }
                })
                .sum::<f64>();

            if !seen {
                return None;
            }

            (func == "rate")
                .then_some(delta / range_secs)
                .or(Some(delta))
        }
        "irate" => delta_irate(samples, metadata, monotonic),
        _ => None,
    }
}

fn delta_irate(
    samples: &[(i64, f64)],
    metadata: &[MetricSampleMetadata],
    monotonic: bool,
) -> Option<f64> {
    for index in (0..samples.len()).rev() {
        let (timestamp, value) = samples[index];

        if !value.is_finite() || (monotonic && value < 0.0) {
            continue;
        }

        let start = metadata[index]
            .start_time_us()
            .filter(|start| *start < timestamp)
            .or_else(|| {
                samples[..index]
                    .iter()
                    .rev()
                    .map(|(previous, _)| *previous)
                    .find(|previous| *previous < timestamp)
            })?;

        let elapsed_secs = (timestamp - start) as f64 / 1_000_000.0;
        return Some(value / elapsed_secs.max(1e-9));
    }

    None
}
