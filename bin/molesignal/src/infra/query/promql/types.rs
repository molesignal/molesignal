// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

/// `LabelSet`：稳定排序的 label 集合，作为 series 分组键 + hash 友好。
pub type LabelSet = BTreeMap<String, String>;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MetricTemporality {
    #[default]
    Cumulative,
    Delta,
}

/// Query-only metric semantics aligned with one sample.
///
/// The top two bits encode cumulative/delta and monotonic/non-monotonic; the
/// remaining bits encode an optional delta start timestamp. Keeping this at
/// eight bytes bounds DELTA query metadata overhead on the matrix hot path.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MetricSampleMetadata(u64);

impl MetricSampleMetadata {
    const SEMANTICS_SHIFT: u32 = 62;
    const START_MASK: u64 = (1_u64 << Self::SEMANTICS_SHIFT) - 1;

    pub fn new(
        temporality: MetricTemporality,
        monotonic: Option<bool>,
        start_time_us: Option<i64>,
    ) -> Self {
        let semantics = match (temporality, monotonic) {
            (MetricTemporality::Cumulative, Some(false)) => 1_u64,
            (MetricTemporality::Delta, Some(false)) => 3_u64,
            (MetricTemporality::Delta, _) => 2_u64,
            (MetricTemporality::Cumulative, _) => 0_u64,
        };
        let start = if temporality == MetricTemporality::Delta {
            start_time_us
                .and_then(|value| u64::try_from(value).ok())
                .filter(|value| *value < Self::START_MASK)
                .map(|value| value + 1)
                .unwrap_or_default()
        } else {
            0
        };
        Self((semantics << Self::SEMANTICS_SHIFT) | start)
    }

    pub fn temporality(self) -> MetricTemporality {
        match self.0 >> Self::SEMANTICS_SHIFT {
            2 | 3 => MetricTemporality::Delta,
            _ => MetricTemporality::Cumulative,
        }
    }

    pub fn is_monotonic(self) -> bool {
        !matches!(self.0 >> Self::SEMANTICS_SHIFT, 1 | 3)
    }

    pub fn start_time_us(self) -> Option<i64> {
        let encoded = self.0 & Self::START_MASK;
        (encoded != 0).then(|| (encoded - 1) as i64)
    }
}

/// 单个时间序列：标签集 + (ts_us, value) 列表。
#[derive(Debug, Clone, Default)]
pub struct Series {
    pub labels: LabelSet,
    pub samples: Vec<(i64, f64)>,
    /// Empty means cumulative counter semantics for every sample. Otherwise
    /// this vector has exactly the same length and ordering as `samples`.
    pub sample_metadata: Vec<MetricSampleMetadata>,
}

impl Series {
    pub(super) fn push_sample(&mut self, sample: (i64, f64), metadata: MetricSampleMetadata) {
        if self.sample_metadata.is_empty() && metadata == MetricSampleMetadata::default() {
            self.samples.push(sample);
            return;
        }
        if self.sample_metadata.is_empty() {
            self.sample_metadata
                .resize(self.samples.len(), MetricSampleMetadata::default());
        }
        self.samples.push(sample);
        self.sample_metadata.push(metadata);
    }

    pub(super) fn metadata_slice(&self, lo: usize, hi: usize) -> &[MetricSampleMetadata] {
        if self.sample_metadata.len() == self.samples.len() {
            &self.sample_metadata[lo..hi]
        } else {
            &[]
        }
    }
}

/// instant 求值结果：每条返回一个 label 集 + 单个 value。
#[derive(Debug, Clone, Default)]
pub struct InstantVector {
    pub items: Vec<(LabelSet, f64)>,
}

/// range 求值结果：每条返回一个时间点、label 集和 value。
#[derive(Debug, Clone)]
pub(super) struct RangePoint {
    pub(super) ts_us: i64,
    pub(super) labels: LabelSet,
    pub(super) value: f64,
}

#[derive(Debug, Clone, Default)]
pub(super) struct RangeVector {
    pub(super) points: Vec<RangePoint>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_sample_metadata_is_compact_and_round_trips_delta_fields() {
        assert_eq!(std::mem::size_of::<MetricSampleMetadata>(), 8);

        let metadata = MetricSampleMetadata::new(
            MetricTemporality::Delta,
            Some(false),
            Some(1_700_000_000_123_456),
        );
        assert_eq!(metadata.temporality(), MetricTemporality::Delta);
        assert!(!metadata.is_monotonic());
        assert_eq!(metadata.start_time_us(), Some(1_700_000_000_123_456));

        let default = MetricSampleMetadata::default();
        assert_eq!(default.temporality(), MetricTemporality::Cumulative);
        assert!(default.is_monotonic());
        assert_eq!(default.start_time_us(), None);
    }
}
