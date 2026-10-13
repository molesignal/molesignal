// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The traces a scan keeps: the best of what it has seen, one row per trace.

use std::{cmp::Ordering, collections::HashMap};

use super::{TraceListContext, TraceListRow, effective_cmp};

/// The best `limit` traces a scan has seen so far.
///
/// A page can ask for thousands of traces and a scan can look at millions of
/// rows, so a row is taken or refused without walking the rows already kept.
/// The kept rows grow to twice the limit before they are cut back to it; the
/// worst row kept at the last cut is the floor that nothing worse can pass;
/// and an index by trace id finds the row a trace already has.
pub(super) struct TopTraces<'a> {
    context: &'a TraceListContext,
    limit: usize,
    rows: Vec<TraceListRow>,
    /// Where the row of each kept trace is in `rows`.
    by_trace: HashMap<String, usize>,
    /// The worst row among the best `limit` when they were last counted. The
    /// best only get better afterwards, so it stays a bound until the next cut.
    floor: Option<TraceListRow>,
}

impl<'a> TopTraces<'a> {
    pub(super) fn new(context: &'a TraceListContext, limit: usize) -> Self {
        Self {
            context,
            limit,
            rows: Vec::with_capacity(limit.saturating_mul(2)),
            by_trace: HashMap::with_capacity(limit.saturating_mul(2)),
            floor: None,
        }
    }

    /// Whether as many traces are kept as were asked for.
    pub(super) fn is_full(&self) -> bool {
        self.rows.len() >= self.limit
    }

    /// Whether a row is no better than the worst of the best, and so has no place among them.
    pub(super) fn cannot_enter(&self, candidate: &TraceListRow) -> bool {
        self.floor
            .as_ref()
            .is_some_and(|floor| effective_cmp(candidate, floor, self.context) != Ordering::Less)
    }

    /// Takes a row if it is among the best seen, as the row of its trace.
    pub(super) fn offer(&mut self, candidate: TraceListRow) {
        if self.limit == 0 || self.cannot_enter(&candidate) {
            return;
        }

        if let Some(&at) = self.by_trace.get(&candidate.item.trace_id) {
            // A trace summarised more than once is one trace: its best row stands for it.
            if effective_cmp(&candidate, &self.rows[at], self.context) == Ordering::Less {
                self.rows[at] = candidate;
            }

            return;
        }

        self.by_trace
            .insert(candidate.item.trace_id.clone(), self.rows.len());
        self.rows.push(candidate);

        if self.rows.len() == self.limit || self.rows.len() >= self.limit.saturating_mul(2) {
            self.settle();
        }
    }

    /// Cuts the kept rows back to the limit and marks the worst of them as the floor.
    pub(super) fn settle(&mut self) {
        if self.limit == 0 || self.rows.len() < self.limit {
            return;
        }

        let context = self.context;

        if self.rows.len() > self.limit {
            self.rows
                .select_nth_unstable_by(self.limit - 1, |left, right| {
                    effective_cmp(left, right, context)
                });
            self.rows.truncate(self.limit);

            self.by_trace.clear();

            for (at, row) in self.rows.iter().enumerate() {
                self.by_trace.insert(row.item.trace_id.clone(), at);
            }
        }

        self.floor = self
            .rows
            .iter()
            .max_by(|left, right| effective_cmp(left, right, context))
            .cloned();
    }

    /// The rows kept. After `settle` they are the best `limit` and no more.
    pub(super) fn rows(&self) -> &[TraceListRow] {
        &self.rows
    }

    /// The best `limit` rows, the best first.
    pub(super) fn into_sorted(mut self) -> Vec<TraceListRow> {
        self.settle();

        let context = self.context;
        self.rows
            .sort_by(|left, right| effective_cmp(left, right, context));

        self.rows
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{
        super::{TraceListSort, effective_cmp},
        TopTraces,
    };
    use crate::api::http::{
        pagination::cursor::CursorDirection,
        routes::web::trace::list::{
            TraceListContext, TraceListItem, TraceListRow,
            cursor::{TraceCursorBoundary, TraceCursorPosition},
        },
    };

    fn context(sort: TraceListSort) -> TraceListContext {
        TraceListContext {
            from: 0,
            to: 1_000_000,
            sort,
            page_size: 2,
            q: None,
            filters: Vec::new(),
            boundary: None,
        }
    }

    fn row(trace_id: &str, start_ns: i64, duration_ns: i64, span_count: i64) -> TraceListRow {
        TraceListRow {
            item: TraceListItem {
                trace_id: trace_id.to_string(),
                service: "api".to_string(),
                operation: "GET /".to_string(),
                start_ns,
                duration_ms: duration_ns as f64 / 1_000_000.0,
                span_count,
                error_count: span_count % 3,
            },
            duration_ns,
        }
    }

    fn ids(rows: &[TraceListRow]) -> Vec<&str> {
        rows.iter().map(|row| row.item.trace_id.as_str()).collect()
    }

    /// Rows in no order at all: many traces, some of them summarised more than once,
    /// with values that repeat so that ties are broken by the later keys.
    fn scattered(count: usize) -> Vec<TraceListRow> {
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;

        (0..count)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);

                let draw = state >> 33;
                let trace = draw % (count as u64 * 3 / 4).max(1);

                row(
                    &format!("trace-{trace:05}"),
                    ((draw >> 3) % 5_000) as i64,
                    ((draw >> 7) % 400) as i64,
                    ((draw >> 11) % 40) as i64,
                )
            })
            .collect()
    }

    /// What the page is by definition: the best row of every trace, the best `limit` of those.
    fn by_definition(
        rows: &[TraceListRow],
        limit: usize,
        context: &TraceListContext,
    ) -> Vec<TraceListRow> {
        let mut best: HashMap<&str, &TraceListRow> = HashMap::new();

        for row in rows {
            best.entry(row.item.trace_id.as_str())
                .and_modify(|kept| {
                    if effective_cmp(row, kept, context).is_lt() {
                        *kept = row;
                    }
                })
                .or_insert(row);
        }

        let mut all: Vec<TraceListRow> = best.into_values().cloned().collect();
        all.sort_by(|left, right| effective_cmp(left, right, context));
        all.truncate(limit);

        all
    }

    #[test]
    fn keeps_the_best_of_many_rows_whatever_order_they_come_in() {
        let rows = scattered(6_000);

        for sort in [
            TraceListSort::Latest,
            TraceListSort::Earliest,
            TraceListSort::DurationDesc,
            TraceListSort::DurationAsc,
            TraceListSort::SpanCountDesc,
            TraceListSort::ErrorsDesc,
        ] {
            let context = context(sort);

            for limit in [1, 2, 101, 1_001, 5_001] {
                let mut top = TopTraces::new(&context, limit);

                for candidate in rows.iter().cloned() {
                    top.offer(candidate);
                }

                let kept = top.into_sorted();
                let expected = by_definition(&rows, limit, &context);

                assert_eq!(ids(&kept), ids(&expected), "{sort:?} limit {limit}");
                assert_eq!(
                    kept.iter().map(|row| row.item.start_ns).collect::<Vec<_>>(),
                    expected
                        .iter()
                        .map(|row| row.item.start_ns)
                        .collect::<Vec<_>>(),
                    "{sort:?} limit {limit}: the row a trace is kept with",
                );
            }
        }
    }

    #[test]
    fn keeps_the_best_when_paging_backwards() {
        let rows = scattered(2_000);
        let mut context = context(TraceListSort::DurationDesc);

        // Backwards the order is the reverse one: the rows nearest the boundary come first.
        context.boundary = Some(TraceCursorBoundary {
            direction: CursorDirection::Before,
            position: TraceCursorPosition {
                start_ns: i64::MAX,
                trace_id: String::new(),
                primary: i64::MAX,
            },
        });

        let mut top = TopTraces::new(&context, 301);

        for candidate in rows.iter().cloned() {
            top.offer(candidate);
        }

        assert_eq!(
            ids(&top.into_sorted()),
            ids(&by_definition(&rows, 301, &context))
        );
    }

    #[test]
    fn a_trace_summarised_twice_is_kept_once_with_its_best_row() {
        let context = context(TraceListSort::Latest);
        let mut top = TopTraces::new(&context, 2);

        top.offer(row("same", 10, 1, 1));
        top.offer(row("same", 20, 2, 2));
        top.offer(row("same", 15, 3, 3));

        let kept = top.into_sorted();

        assert_eq!(ids(&kept), vec!["same"]);
        assert_eq!(kept[0].item.start_ns, 20);
    }

    #[test]
    fn nothing_enters_under_the_worst_of_the_best() {
        let context = context(TraceListSort::Latest);
        let mut top = TopTraces::new(&context, 2);

        // Until as many are kept as were asked for, any row may be among them.
        top.offer(row("b", 20, 1, 1));
        assert!(!top.is_full());
        assert!(!top.cannot_enter(&row("a", 1, 1, 1)));

        top.offer(row("c", 30, 1, 1));
        assert!(top.is_full());

        // The worst kept cannot enter again, and neither can anything behind it.
        assert!(top.cannot_enter(&row("b", 20, 1, 1)));
        assert!(top.cannot_enter(&row("a", 10, 1, 1)));
        assert!(!top.cannot_enter(&row("d", 25, 1, 1)));

        top.offer(row("a", 10, 1, 1));
        assert_eq!(top.rows().len(), 2);

        top.offer(row("d", 25, 1, 1));
        assert_eq!(ids(&top.into_sorted()), vec!["c", "d"]);
    }

    #[test]
    fn the_kept_rows_never_grow_past_twice_what_was_asked_for() {
        let context = context(TraceListSort::Earliest);
        let mut top = TopTraces::new(&context, 50);

        // Every row is better than the last: each one enters.
        for index in (0..10_000).rev() {
            top.offer(row(&format!("trace-{index:05}"), index, 1, 1));
            assert!(top.rows().len() < 100);
        }

        top.settle();
        assert_eq!(top.rows().len(), 50);
        assert_eq!(top.into_sorted()[0].item.start_ns, 0);
    }
}
