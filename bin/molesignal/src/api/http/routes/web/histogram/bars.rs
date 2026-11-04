// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The bars of a histogram already counted, for the log and the trace explorers.
//!
//! A bar that lies wholly inside a counted range and ended more than
//! [`QUERY_FRESH_WINDOW_MICROS`] ago, so that late results have arrived, keeps its count. Such
//! bars are kept under their query, which holds the filters, the bar length and where bars
//! begin, and a later histogram of the same query only counts the bars not kept. An explorer
//! that refreshes the last fifteen minutes then counts the newest bar or two.
//!
//! Every histogram still counts at least its newest bar, and takes from that count whether time
//! and the level are masked for the reader: a count kept from before a field was masked is never
//! shown without that check.

use std::{
    collections::BTreeMap,
    sync::{Arc, OnceLock},
    time::Duration,
};

use moka::future::Cache;

use super::{BarCount, BarLatency, BucketPlan};
use crate::infra::caching::QUERY_FRESH_WINDOW_MICROS;

/// Separate gaps counted at most; more are counted as one range.
const MAX_GAPS: usize = 4;

/// The bars a histogram asks for: `count` bars of `width`, the first beginning at `origin`, over
/// `[from, to)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in super::super) struct Layout {
    pub(in super::super) from: i64,
    pub(in super::super) to: i64,
    pub(in super::super) origin: i64,
    pub(in super::super) width: i64,
    pub(in super::super) count: i64,
}

impl Layout {
    pub(in super::super) fn of(plan: &BucketPlan) -> Self {
        Self {
            from: plan.from,
            to: plan.to,
            origin: plan.origin_micros(),
            width: plan.bucket_micros(),
            count: plan.bars() as i64,
        }
    }

    fn start(&self, index: i64) -> i64 {
        self.origin + index * self.width
    }

    /// The bar lies wholly inside `[from, to)`: its count does not depend on where they fall.
    fn whole(&self, index: i64) -> bool {
        let start = self.start(index);
        start >= self.from && start.saturating_add(self.width) <= self.to
    }

    /// The bars of `gap`, counted with the same first bar as this layout.
    pub(in super::super) fn plan(&self, gap: &Gap) -> crate::shared::Result<BucketPlan> {
        BucketPlan::new(
            Some(gap.from),
            Some(gap.to),
            Some(self.width),
            Some(self.origin),
        )
    }
}

/// One bar counted: the results in it, the errors among them, and how long they took at the
/// median and the 95th and 99th percentile when they have a duration.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in super::super) struct KeptBar {
    pub(in super::super) total: u64,
    pub(in super::super) errors: u64,
    pub(in super::super) took: Option<[i64; 3]>,
}

/// The bars of one query counted so far, by where each begins. A bar with no result is kept
/// too, with zero counts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in super::super) struct KnownBars {
    /// Errors were counted apart from the rest.
    pub(in super::super) split: bool,
    pub(in super::super) bars: BTreeMap<i64, KeptBar>,
}

/// A range of bars to count: `[from, to)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in super::super) struct Gap {
    pub(in super::super) from: i64,
    pub(in super::super) to: i64,
}

/// The ranges to count for `layout`: every bar not kept, the bars at either end that lie partly
/// outside the range, and always the newest bar.
pub(in super::super) fn gaps(layout: &Layout, known: Option<&KnownBars>) -> Vec<Gap> {
    let kept = |index: i64| {
        index + 1 < layout.count
            && layout.whole(index)
            && known.is_some_and(|known| known.bars.contains_key(&layout.start(index)))
    };

    let mut runs: Vec<(i64, i64)> = Vec::new();

    for index in (0..layout.count).filter(|index| !kept(*index)) {
        match runs.last_mut() {
            Some((_, last)) if *last + 1 == index => *last = index,
            _ => runs.push((index, index)),
        }
    }

    if runs.len() > MAX_GAPS {
        let (first, last) = (runs[0].0, runs[runs.len() - 1].1);
        runs = vec![(first, last)];
    }

    runs.into_iter()
        .map(|(first, last)| Gap {
            from: layout.from.max(layout.start(first)),
            to: layout
                .to
                .min(layout.start(last).saturating_add(layout.width)),
        })
        .collect()
}

/// What a count of a gap returned, by where each bar begins.
pub(in super::super) fn by_start(
    layout: &Layout,
    bars: &[BarCount],
    latency: &[BarLatency],
) -> BTreeMap<i64, KeptBar> {
    bars.iter()
        .map(|bar| {
            let took = latency
                .iter()
                .find(|took| took.index == bar.index)
                .map(|took| [took.p50_ns, took.p95_ns, took.p99_ns]);

            let kept = KeptBar {
                total: bar.total,
                errors: bar.errors,
                took,
            };

            (layout.start(bar.index), kept)
        })
        .collect()
}

/// A histogram's bars, counted now or kept, and the bars this count adds to the kept ones.
pub(in super::super) struct Assembled {
    pub(in super::super) bars: Vec<BarCount>,
    pub(in super::super) latency: Vec<BarLatency>,
    keep: BTreeMap<i64, KeptBar>,
}

/// The bars of `layout`: the ones of the gaps counted now, the rest from `known`.
pub(in super::super) fn assemble(
    layout: &Layout,
    counted: &[(Gap, BTreeMap<i64, KeptBar>)],
    known: Option<&KnownBars>,
    now: i64,
) -> Assembled {
    let mut by_start = BTreeMap::new();
    let mut keep = BTreeMap::new();

    for (gap, bars) in counted {
        keep.extend(closed(layout, gap, bars, now));
        by_start.extend(bars.iter().map(|(start, bar)| (*start, *bar)));
    }

    if let Some(known) = known {
        for index in 0..layout.count {
            let start = layout.start(index);

            let counted_now = counted
                .iter()
                .any(|(gap, _)| start.saturating_add(layout.width) > gap.from && start < gap.to);

            if !counted_now && let Some(bar) = known.bars.get(&start) {
                by_start.insert(start, *bar);
            }
        }
    }

    let index = |start: i64| (start - layout.origin) / layout.width;

    Assembled {
        bars: by_start
            .iter()
            .map(|(start, bar)| BarCount {
                index: index(*start),
                total: bar.total,
                errors: bar.errors,
            })
            .collect(),
        latency: by_start
            .iter()
            .filter_map(|(start, bar)| {
                let [p50_ns, p95_ns, p99_ns] = bar.took?;

                Some(BarLatency {
                    index: index(*start),
                    p50_ns,
                    p95_ns,
                    p99_ns,
                })
            })
            .collect(),
        keep,
    }
}

/// Keeps the closed bars of `assembled` under `key`. `split` says whether this count told errors
/// apart; kept bars of the other kind are dropped.
pub(in super::super) async fn store(
    key: String,
    known: Option<&KnownBars>,
    split: bool,
    assembled: Assembled,
) {
    let Some(cache) = cache() else {
        return;
    };

    if assembled.keep.is_empty() {
        return;
    }

    let mut kept = known
        .filter(|known| known.split == split)
        .cloned()
        .unwrap_or_default();

    kept.split = split;
    kept.bars.extend(assembled.keep);
    cache.insert(key, Arc::new(kept)).await;
}

/// The bars of `layout` that can be kept from a count of `counted`: whole bars inside it that
/// ended before the fresh window. Bars with no result are absent from `counted` and kept as zero.
fn closed(
    layout: &Layout,
    gap: &Gap,
    counted: &BTreeMap<i64, KeptBar>,
    now: i64,
) -> BTreeMap<i64, KeptBar> {
    (0..layout.count)
        .map(|index| layout.start(index))
        .filter(|start| {
            let end = start.saturating_add(layout.width);
            *start >= gap.from && end <= gap.to && end <= now - QUERY_FRESH_WINDOW_MICROS
        })
        .map(|start| (start, counted.get(&start).copied().unwrap_or_default()))
        .collect()
}

/// Bars kept across histograms, or `None` when `[cache.query_result]` has no capacity or the
/// settings are not loaded.
pub(in super::super) fn cache() -> Option<&'static Cache<String, Arc<KnownBars>>> {
    static CACHE: OnceLock<Option<Cache<String, Arc<KnownBars>>>> = OnceLock::new();

    CACHE
        .get_or_init(|| {
            let settings = &crate::config::try_get()?.cache.query_result;

            (settings.capacity > 0).then(|| {
                Cache::builder()
                    .max_capacity(settings.capacity)
                    .time_to_live(Duration::from_secs(settings.ttl_secs))
                    .build()
            })
        })
        .as_ref()
}

/// The key of a query: its organization, the role it reads as, and a description of it that
/// names every bar of the query wherever its range begins.
pub(in super::super) fn key(org: &str, role: &str, query: &str) -> String {
    let payload = format!("{org}\0{role}\0{query}");
    blake3::hash(payload.as_bytes()).to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: i64 = 60_000_000;

    /// Fifteen minutes from 10:02:30 in bars of a minute from 10:02.
    fn layout() -> Layout {
        Layout {
            from: 602 * MINUTE + 30_000_000,
            to: 617 * MINUTE + 30_000_000,
            origin: 602 * MINUTE,
            width: MINUTE,
            count: 16,
        }
    }

    fn bar(total: u64, errors: u64) -> KeptBar {
        KeptBar {
            total,
            errors,
            took: None,
        }
    }

    fn known(starts: impl IntoIterator<Item = i64>) -> KnownBars {
        KnownBars {
            split: true,
            bars: starts.into_iter().map(|start| (start, bar(1, 0))).collect(),
        }
    }

    #[test]
    fn without_kept_bars_the_whole_range_is_counted() {
        let layout = layout();

        assert_eq!(
            gaps(&layout, None),
            vec![Gap {
                from: layout.from,
                to: layout.to
            }]
        );
    }

    #[test]
    fn kept_bars_leave_the_edges_and_the_newest_bar_to_count() {
        let layout = layout();
        let all = known((0..16).map(|index| layout.start(index)));

        assert_eq!(
            gaps(&layout, Some(&all)),
            vec![
                Gap {
                    from: layout.from,
                    to: 603 * MINUTE
                },
                Gap {
                    from: 617 * MINUTE,
                    to: layout.to
                },
            ]
        );

        // Two kept bars short of the end: they are counted with the newest.
        let most = known((0..13).map(|index| layout.start(index)));

        assert_eq!(
            gaps(&layout, Some(&most)),
            vec![
                Gap {
                    from: layout.from,
                    to: 603 * MINUTE
                },
                Gap {
                    from: 615 * MINUTE,
                    to: layout.to
                },
            ]
        );
    }

    #[test]
    fn scattered_gaps_are_counted_as_one_range() {
        let layout = layout();
        let every_other = known((0..16).step_by(2).map(|index| layout.start(index)));

        assert_eq!(
            gaps(&layout, Some(&every_other)),
            vec![Gap {
                from: layout.from,
                to: layout.to
            }]
        );
    }

    #[test]
    fn only_whole_bars_that_ended_before_the_fresh_window_are_kept() {
        let layout = layout();

        let gap = Gap {
            from: layout.from,
            to: layout.to,
        };

        let counted = BTreeMap::from([(605 * MINUTE, bar(3, 1))]);
        let now = 612 * MINUTE + QUERY_FRESH_WINDOW_MICROS;
        let kept = closed(&layout, &gap, &counted, now);

        // Whole bars from 10:03 to the one ending at 10:12; the 10:02 bar is cut by the range.
        assert_eq!(kept.keys().next(), Some(&(603 * MINUTE)));
        assert_eq!(kept.keys().last(), Some(&(611 * MINUTE)));
        assert_eq!(kept.get(&(605 * MINUTE)), Some(&bar(3, 1)));
        assert_eq!(kept.get(&(606 * MINUTE)), Some(&bar(0, 0)));
    }

    /// Bars counted now and bars kept make one histogram; how long a bar's results took comes
    /// back with it, and the closed bars counted now are the ones to keep.
    #[test]
    fn counted_and_kept_bars_make_one_histogram() {
        let layout = layout();
        let mut kept = known((0..14).map(|index| layout.start(index)));
        kept.bars.get_mut(&(604 * MINUTE)).unwrap().took = Some([10, 20, 30]);

        let gaps = gaps(&layout, Some(&kept));
        let newest = *gaps.last().unwrap();

        let counted = vec![
            (gaps[0], BTreeMap::from([(602 * MINUTE, bar(5, 1))])),
            (newest, BTreeMap::from([(617 * MINUTE, bar(2, 0))])),
        ];

        let now = 617 * MINUTE;
        let assembled = assemble(&layout, &counted, Some(&kept), now);

        assert_eq!(
            assembled.bars.first().map(|bar| (bar.index, bar.total)),
            Some((0, 5))
        );
        assert_eq!(
            assembled.bars.last().map(|bar| (bar.index, bar.total)),
            Some((15, 2))
        );
        assert_eq!(
            assembled.bars.len(),
            15,
            "13 kept bars between the two counted ones"
        );
        assert_eq!(
            assembled
                .latency
                .iter()
                .map(|took| (took.index, took.p95_ns))
                .collect::<Vec<_>>(),
            vec![(2, 20)]
        );
        assert!(
            assembled.keep.is_empty(),
            "the edge bars and the newest one are never kept"
        );
    }
}
