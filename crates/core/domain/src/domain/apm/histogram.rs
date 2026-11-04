// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

use crate::shared::{Error, Result};

/// The version of the fixed power-of-two buckets histograms were first stored
/// with. Rows written with it are still read: their counts are shared out over
/// the current buckets ([`LatencyHistogram::converted`]).
pub const APM_HISTOGRAM_LEGACY_SCHEMA_VERSION: u16 = 1;

pub const APM_HISTOGRAM_LEGACY_UPPER_BOUNDS_MICROS: [u64; 18] = [
    1_000,
    2_000,
    4_000,
    8_000,
    16_000,
    32_000,
    64_000,
    128_000,
    256_000,
    512_000,
    1_000_000,
    2_000_000,
    4_000_000,
    8_000_000,
    16_000_000,
    30_000_000,
    60_000_000,
    u64::MAX,
];

/// The version of the layout new histograms are written with by default.
pub const APM_HISTOGRAM_SCHEMA_VERSION: u16 = 2;

/// Buckets to each doubling of latency by default: neighbouring bounds are 9%
/// apart, so a percentile read from them is within about 4% of the true value.
pub const APM_HISTOGRAM_BUCKETS_PER_OCTAVE: u16 = 8;

/// The most buckets a doubling can be split into.
pub const APM_HISTOGRAM_MAX_BUCKETS_PER_OCTAVE: u16 = 16;

/// The finite bounds of a logarithmic layout run over these powers of two of a
/// millisecond: from 125 µs to about 131 s. Anything longer is in +Inf.
const LOWEST_OCTAVE: i32 = -3;
const HIGHEST_OCTAVE: i32 = 17;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistogramSchema {
    pub version: u16,
    pub upper_bounds_time: Vec<u64>,
}

impl HistogramSchema {
    /// The fixed power-of-two buckets of version 1.
    pub fn v1() -> Self {
        Self {
            version: APM_HISTOGRAM_LEGACY_SCHEMA_VERSION,
            upper_bounds_time: APM_HISTOGRAM_LEGACY_UPPER_BOUNDS_MICROS.to_vec(),
        }
    }

    /// Buckets that each grow by the same ratio, `buckets_per_octave` to every
    /// doubling, from 125 µs to about 131 s, and +Inf after them.
    pub fn logarithmic(version: u16, buckets_per_octave: u16) -> Self {
        let per_octave =
            i32::from(buckets_per_octave.clamp(1, APM_HISTOGRAM_MAX_BUCKETS_PER_OCTAVE));

        let mut upper_bounds_micros = (LOWEST_OCTAVE * per_octave..=HIGHEST_OCTAVE * per_octave)
            .map(|step| {
                (1_000.0 * 2_f64.powf(f64::from(step) / f64::from(per_octave))).round() as u64
            })
            .collect::<Vec<_>>();

        upper_bounds_micros.push(u64::MAX);

        Self {
            version,
            upper_bounds_time: upper_bounds_micros,
        }
    }

    /// The layout new histograms are written with unless set otherwise.
    pub fn current() -> Self {
        Self::logarithmic(
            APM_HISTOGRAM_SCHEMA_VERSION,
            APM_HISTOGRAM_BUCKETS_PER_OCTAVE,
        )
    }

    /// The layout a stored histogram was written with, told from its version
    /// and its number of buckets: version 1 is the fixed buckets, any other a
    /// logarithmic layout whose size gives its buckets to an octave. `None`
    /// for a shape no layout has.
    pub fn of_stored(version: u16, bucket_count: usize) -> Option<Self> {
        if version == APM_HISTOGRAM_LEGACY_SCHEMA_VERSION {
            return (bucket_count == APM_HISTOGRAM_LEGACY_UPPER_BOUNDS_MICROS.len()).then(Self::v1);
        }

        // A layout has `octaves * n + 1` finite bounds and +Inf.
        let octaves = usize::try_from(HIGHEST_OCTAVE - LOWEST_OCTAVE).ok()?;
        let steps = bucket_count.checked_sub(2)?;

        if version == 0 || steps == 0 || steps % octaves != 0 {
            return None;
        }

        let per_octave = u16::try_from(steps / octaves).ok()?;

        (per_octave <= APM_HISTOGRAM_MAX_BUCKETS_PER_OCTAVE)
            .then(|| Self::logarithmic(version, per_octave))
    }

    pub fn validate(&self) -> Result<()> {
        if self.version == 0 || self.upper_bounds_time.len() < 2 {
            return Err(Error::invalid(
                "APM histogram version and bucket count must be non-zero",
            ));
        }

        if self.upper_bounds_time.last() != Some(&u64::MAX) {
            return Err(Error::invalid(
                "APM histogram must end with the +Inf bucket",
            ));
        }

        if self
            .upper_bounds_time
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            return Err(Error::invalid(
                "APM histogram upper bounds must be strictly increasing",
            ));
        }

        Ok(())
    }

    /// Where a bucket begins and ends; +Inf ends at `u64::MAX`.
    fn range_of(&self, index: usize) -> (u64, u64) {
        let lower = index
            .checked_sub(1)
            .map_or(0, |previous| self.upper_bounds_time[previous]);

        (lower, self.upper_bounds_time[index])
    }
}

/// A bucket that holds observations, narrowed to the least and greatest of
/// them: what a latency distribution is drawn from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistogramBucket {
    pub lower_time: u64,
    pub upper_time: u64,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatencyHistogram {
    pub schema_version: u16,
    pub counts: Vec<u64>,
    pub sum_time: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_time: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_time: Option<u64>,
}

impl LatencyHistogram {
    pub fn empty(schema: &HistogramSchema) -> Self {
        Self {
            schema_version: schema.version,
            counts: vec![0; schema.upper_bounds_time.len()],
            sum_time: 0,
            min_time: None,
            max_time: None,
        }
    }

    pub fn count(&self) -> u64 {
        self.counts.iter().copied().sum()
    }

    pub fn observe(&mut self, schema: &HistogramSchema, duration_micros: u64) -> Result<()> {
        self.ensure_schema(schema)?;

        let bucket = schema
            .upper_bounds_time
            .iter()
            .position(|upper| duration_micros <= *upper)
            .ok_or_else(|| Error::internal("APM histogram is missing +Inf"))?;

        self.counts[bucket] = self.counts[bucket].saturating_add(1);
        self.sum_time = self.sum_time.saturating_add(duration_micros);
        self.min_time = Some(
            self.min_time
                .map_or(duration_micros, |current| current.min(duration_micros)),
        );
        self.max_time = Some(
            self.max_time
                .map_or(duration_micros, |current| current.max(duration_micros)),
        );

        Ok(())
    }

    pub fn merge(&mut self, other: &Self) -> Result<()> {
        if self.schema_version != other.schema_version || self.counts.len() != other.counts.len() {
            return Err(Error::invalid(
                "cannot merge incompatible APM histogram schemas",
            ));
        }

        for (target, value) in self.counts.iter_mut().zip(&other.counts) {
            *target = target.saturating_add(*value);
        }

        self.sum_time = self.sum_time.saturating_add(other.sum_time);
        self.min_time = match (self.min_time, other.min_time) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (left, right) => left.or(right),
        };
        self.max_time = match (self.max_time, other.max_time) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (left, right) => left.or(right),
        };

        Ok(())
    }

    /// The latency `quantile` of the observations stay under: the bucket the
    /// rank falls in, and within it a straight line between its bounds, which
    /// are narrowed to the least and greatest observation.
    pub fn quantile(&self, schema: &HistogramSchema, quantile: f64) -> Result<Option<u64>> {
        self.ensure_schema(schema)?;

        if !quantile.is_finite() || !(0.0..=1.0).contains(&quantile) {
            return Err(Error::invalid("APM histogram quantile must be in [0, 1]"));
        }

        let count = self.count();

        if count == 0 {
            return Ok(None);
        }

        let rank = count as f64 * quantile;
        let mut before = 0_u64;

        for (index, value) in self.counts.iter().enumerate() {
            if *value == 0 {
                continue;
            }

            let through = before.saturating_add(*value);

            if rank <= through as f64 {
                let (lower, upper) = self.narrowed(schema, index);
                let share = ((rank - before as f64) / *value as f64).clamp(0.0, 1.0);

                return Ok(Some(
                    lower + ((upper - lower) as f64 * share).round() as u64,
                ));
            }

            before = through;
        }

        Ok(self.max_time)
    }

    /// How many observations took at most `threshold_micros`: the buckets
    /// wholly under it, and the share of the bucket it falls in that lies
    /// under it. What Apdex and a latency objective count as good.
    pub fn count_at_or_below(
        &self,
        schema: &HistogramSchema,
        threshold_micros: u64,
    ) -> Result<f64> {
        self.ensure_schema(schema)?;

        let mut below = 0.0_f64;

        for (index, value) in self.counts.iter().enumerate() {
            if *value == 0 {
                continue;
            }

            let (lower, upper) = self.narrowed(schema, index);

            if upper <= threshold_micros {
                below += *value as f64;
            } else if lower < threshold_micros {
                let share = (threshold_micros - lower) as f64 / (upper - lower) as f64;
                below += *value as f64 * share.clamp(0.0, 1.0);
            }
        }

        Ok(below)
    }

    /// The same observations over another layout's buckets. Each count is
    /// shared among the buckets its own overlaps, in proportion to the
    /// overlap; the total, the sum and the extremes stay as they were.
    pub fn converted(&self, from: &HistogramSchema, to: &HistogramSchema) -> Result<Self> {
        self.ensure_schema(from)?;

        let mut converted = Self::empty(to);
        converted.sum_time = self.sum_time;
        converted.min_time = self.min_time;
        converted.max_time = self.max_time;

        for (index, count) in self.counts.iter().enumerate() {
            if *count == 0 {
                continue;
            }

            let (lower, upper) = self.narrowed(from, index);
            let weights = (0..to.upper_bounds_time.len())
                .map(|target| {
                    let (start, end) = to.range_of(target);
                    (target, overlap(lower, upper, start, end))
                })
                .filter(|(_, weight)| *weight > 0.0)
                .collect::<Vec<_>>();

            if weights.is_empty() {
                // A bucket narrowed to a single value lands where that value does.
                let target = to
                    .upper_bounds_time
                    .iter()
                    .position(|bound| upper <= *bound)
                    .unwrap_or(to.upper_bounds_time.len() - 1);
                converted.counts[target] = converted.counts[target].saturating_add(*count);
                continue;
            }

            share_out(*count, &weights, &mut converted.counts);
        }

        Ok(converted)
    }

    /// The buckets that hold observations, each narrowed to the least and
    /// greatest observation.
    pub fn filled_buckets(&self, schema: &HistogramSchema) -> Result<Vec<HistogramBucket>> {
        self.ensure_schema(schema)?;

        Ok(self
            .counts
            .iter()
            .enumerate()
            .filter(|(_, count)| **count > 0)
            .map(|(index, count)| {
                let (lower_micros, upper_micros) = self.narrowed(schema, index);

                HistogramBucket {
                    lower_time: lower_micros,
                    upper_time: upper_micros,
                    count: *count,
                }
            })
            .collect())
    }

    /// A bucket's bounds kept within the observations: no observation is
    /// shorter than the least or longer than the greatest.
    fn narrowed(&self, schema: &HistogramSchema, index: usize) -> (u64, u64) {
        let (lower, upper) = schema.range_of(index);
        let lower = self.min_time.map_or(lower, |least| lower.max(least));
        let upper = self.max_time.map_or(upper, |greatest| upper.min(greatest));

        // Past the last finite bound and with no greatest known, the bucket is read as its start.
        let upper = if upper == u64::MAX { lower } else { upper };

        (lower, upper.max(lower))
    }

    fn ensure_schema(&self, schema: &HistogramSchema) -> Result<()> {
        if self.schema_version != schema.version
            || self.counts.len() != schema.upper_bounds_time.len()
        {
            return Err(Error::invalid(
                "APM histogram does not match the requested schema",
            ));
        }

        Ok(())
    }
}

/// How much of `[lower, upper]` lies in the bucket `(start, end]`.
fn overlap(lower: u64, upper: u64, start: u64, end: u64) -> f64 {
    let from = lower.max(start);
    let to = upper.min(end);

    if to > from { (to - from) as f64 } else { 0.0 }
}

/// Shares a count out by weight: each target gets its whole part, and what
/// the rounding leaves goes to the largest remainders.
fn share_out(count: u64, weights: &[(usize, f64)], counts: &mut [u64]) {
    let total = weights.iter().map(|(_, weight)| weight).sum::<f64>();

    let mut shares = weights
        .iter()
        .map(|(target, weight)| {
            let exact = count as f64 * weight / total;
            (*target, exact.floor() as u64, exact - exact.floor())
        })
        .collect::<Vec<_>>();

    let given = shares.iter().map(|(_, whole, _)| whole).sum::<u64>();
    let mut left = count.saturating_sub(given);

    shares.sort_by(|a, b| b.2.total_cmp(&a.2));

    for (_, whole, _) in shares.iter_mut() {
        if left == 0 {
            break;
        }

        *whole += 1;
        left -= 1;
    }

    for (target, whole, _) in shares {
        counts[target] = counts[target].saturating_add(whole);
    }
}
