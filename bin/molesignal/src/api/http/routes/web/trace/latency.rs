// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Durations counted into the bars of the R10 series, and percentiles read
//! back from those bars.
//!
//! RED lays its duration distribution out in these bars. The trace histogram
//! counts every trace's duration into them, bar by bar of time, and reads the
//! median and the 95th and 99th percentile back: a count of a hundred bars
//! per bar of time, however many traces it holds. A percentile read so is
//! within one bar of the true value, about a quarter of it.

/// The preferred numbers of the R10 series in hundreds: ten steps to a power
/// of ten, each about a quarter longer than the one before, all of them round.
pub(super) const PREFERRED: [i64; 10] = [100, 125, 160, 200, 250, 315, 400, 500, 630, 800];
/// How many powers of ten the bars cover: from 100 ns to 1,000 s.
const DECADES: u32 = 10;

/// Where the bars of the duration distribution begin, in nanoseconds: the R10
/// series in every power of ten from 100 ns up to 1,000 s. Bar `i` holds the
/// durations from edge `i` up to edge `i + 1`; the first also holds whatever
/// is shorter, the last whatever is longer.
pub(super) fn latency_edges() -> Vec<i64> {
    let mut edges = (0..DECADES)
        .flat_map(|decade| PREFERRED.map(|edge| edge * 10_i64.pow(decade)))
        .collect::<Vec<_>>();

    edges.push(PREFERRED[0] * 10_i64.pow(DECADES));
    edges
}

/// The median and the 95th and 99th percentile of some durations, in nanoseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Percentiles {
    pub(super) p50_ns: i64,
    pub(super) p95_ns: i64,
    pub(super) p99_ns: i64,
}

/// Durations counted into the bars of [`latency_edges`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct LatencyTally {
    /// How many durations each bar holds; empty until the first is counted.
    counts: Vec<u32>,
}

impl LatencyTally {
    /// Counts one duration into its bar.
    pub(super) fn record(&mut self, edges: &[i64], duration_ns: i64) {
        if self.counts.is_empty() {
            self.counts = vec![0; edges.len()];
        }

        // The first edge above the duration closes its bar.
        let bar = edges
            .partition_point(|edge| *edge <= duration_ns)
            .saturating_sub(1);

        if let Some(count) = self.counts.get_mut(bar) {
            *count = count.saturating_add(1);
        }
    }

    /// Adds what another tally counted.
    pub(super) fn merge(&mut self, other: &Self) {
        if other.counts.is_empty() {
            return;
        }

        if self.counts.is_empty() {
            self.counts = other.counts.clone();
            return;
        }

        for (count, added) in self.counts.iter_mut().zip(&other.counts) {
            *count = count.saturating_add(*added);
        }
    }

    /// The three percentiles; `None` when nothing was counted.
    pub(super) fn percentiles(&self, edges: &[i64]) -> Option<Percentiles> {
        Some(Percentiles {
            p50_ns: self.percentile(edges, 0.5)?,
            p95_ns: self.percentile(edges, 0.95)?,
            p99_ns: self.percentile(edges, 0.99)?,
        })
    }

    /// The duration a share of the counted ones took no longer than, read
    /// between the edges of its bar along their ratio, as the bars grow.
    fn percentile(&self, edges: &[i64], share: f64) -> Option<i64> {
        let total = self
            .counts
            .iter()
            .map(|count| u64::from(*count))
            .sum::<u64>();

        if total == 0 {
            return None;
        }

        // The rank of the duration asked for, counted from one.
        let rank = ((share * total as f64).ceil() as u64).clamp(1, total);
        let mut below = 0_u64;

        for (bar, count) in self.counts.iter().enumerate() {
            let count = u64::from(*count);

            if below + count < rank {
                below += count;
                continue;
            }

            let low = edges[bar] as f64;

            // The last bar has no end: it holds whatever is longer.
            let Some(high) = edges.get(bar + 1).map(|edge| *edge as f64) else {
                return Some(edges[bar]);
            };

            let within = ((rank - below) as f64 - 0.5) / count as f64;

            return Some((low * (high / low).powf(within.clamp(0.0, 1.0))).round() as i64);
        }

        edges.last().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_edges_run_from_a_hundred_nanoseconds_to_a_thousand_seconds() {
        let edges = latency_edges();

        assert_eq!(edges.len(), 101);
        assert_eq!(edges[0], 100);
        assert_eq!(edges[1], 125);
        assert_eq!(edges[10], 1_000);
        assert_eq!(*edges.last().unwrap(), 1_000_000_000_000);
    }

    #[test]
    fn reads_percentiles_within_the_bar_of_the_true_value() {
        let edges = latency_edges();
        let mut tally = LatencyTally::default();

        // A hundred durations: 1 ms to 100 ms, one of each.
        for ms in 1..=100 {
            tally.record(&edges, ms * 1_000_000);
        }

        let read = tally.percentiles(&edges).unwrap();
        let within = |read: i64, truth: i64| (read as f64 / truth as f64 - 1.0).abs() < 0.26;

        assert!(within(read.p50_ns, 50_000_000), "{read:?}");
        assert!(within(read.p95_ns, 95_000_000), "{read:?}");
        assert!(within(read.p99_ns, 99_000_000), "{read:?}");
        assert!(read.p50_ns <= read.p95_ns && read.p95_ns <= read.p99_ns);
    }

    #[test]
    fn counts_the_shortest_and_longest_into_the_outer_bars() {
        let edges = latency_edges();
        let mut tally = LatencyTally::default();

        tally.record(&edges, 1);
        tally.record(&edges, i64::MAX);

        assert_eq!(tally.counts[0], 1);
        assert_eq!(tally.counts[edges.len() - 1], 1);
        assert_eq!(tally.percentile(&edges, 1.0), Some(1_000_000_000_000));
    }

    #[test]
    fn merging_adds_the_counts_of_both() {
        let edges = latency_edges();
        let (mut first, mut second) = (LatencyTally::default(), LatencyTally::default());

        first.record(&edges, 2_000);
        second.record(&edges, 2_000);
        second.record(&edges, 3_000_000);
        first.merge(&second);
        first.merge(&LatencyTally::default());

        assert_eq!(first.counts.iter().sum::<u32>(), 3);
        assert!(LatencyTally::default().percentiles(&edges).is_none());
    }
}
