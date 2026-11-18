// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The newest (or oldest) rows of files that keep minute counts. A clustered file holds its rows
//! by time slice, not in time order, so a scan for its newest rows reads its whole time column.
//! Where a statement's conditions are equalities on the fields the counts are kept by, the counts
//! tell a minute its `n`-th newest row is no older than: the page is read from that minute on, the
//! files wholly older are left out, and the page index skips the older slices of the rest.
//!
//! The rows counted are only some of the rows the page may come from, never rows it may not: a
//! file without counts, or whose counts are not kept by the statement's fields, is not counted, and
//! a minute the window cuts is not counted either. The minute found is therefore never newer than
//! the `n`-th newest row itself.

use std::{
    cmp::Reverse,
    collections::{BTreeMap, HashMap},
};

use domain::storage::QueryFile;
use kernel::time::{TimeRange, TimestampMicros};
use query_language::row_filters;

use super::{DataFusionEngine, waves::TimeOrder};
use crate::infra::storage::minute_counts::{MINUTE, MinuteCountsKeys};

/// Files whose counts are read at once.
const FILES_AT_ONCE: usize = 16;

/// Most files whose counts are read for one page.
const MAX_FILES: usize = 64;

/// The part of `window` the page of `sql` over `table` comes from, as far as the minute counts of
/// `files` (`keys`: by segment id) tell; `None` when they do not narrow it.
/// `order.fetch` is the page's size.
pub(super) async fn page_window(
    engine: &DataFusionEngine,
    (sql, table): (&str, &str),
    files: &[QueryFile],
    keys: &HashMap<String, MinuteCountsKeys>,
    window: TimeRange,
    order: TimeOrder,
) -> Option<TimeRange> {
    if keys.is_empty() || order.fetch == 0 {
        return None;
    }

    let conditions = row_filters(sql, table)?;

    // Only rows inside both the request's window and the statement's own bounds count, and only
    // in the whole minutes inside them.
    let start = window.start.0.max(conditions.from.unwrap_or(i64::MIN));
    let end = window.end.0.min(conditions.to.unwrap_or(i64::MAX));
    let first = start.div_euclid(MINUTE) + i64::from(start.rem_euclid(MINUTE) != 0);
    let last = end.div_euclid(MINUTE) - 1;

    if first > last {
        return None;
    }

    // Files nearest the page's end first.
    let mut candidates = files
        .iter()
        .filter(|file| {
            keys.contains_key(file.id.as_str())
                && file.time_range.start.0 < end
                && start <= file.time_range.end.0
        })
        .collect::<Vec<_>>();

    match order.descending {
        true => candidates.sort_by_key(|file| Reverse(file.time_range.end.0)),
        false => candidates.sort_by_key(|file| file.time_range.start.0),
    }

    candidates.truncate(MAX_FILES);

    let mut rows: BTreeMap<i64, u64> = BTreeMap::new();
    let mut minute = None;

    for (read, chunk) in candidates.chunks(FILES_AT_ONCE).enumerate() {
        let wanted = chunk
            .iter()
            .map(|file| {
                keys.get(file.id.as_str())
                    .map(|keys| (file.id.as_str().to_owned(), keys.clone()))
            })
            .collect();

        let loaded = engine
            .minute_counts
            .get_many(engine.object_store.as_ref(), wanted)
            .await;

        for (file, counts) in chunk.iter().zip(loaded) {
            let Some(counts) = counts.filter(|counts| counts.total() == file.rows) else {
                continue;
            };

            let Some(equal) = counts.places(&conditions.filters) else {
                continue;
            };

            for entry in counts.entries(first..=last, &equal) {
                *rows.entry(counts.minute(entry)).or_default() += counts.rows(entry);
            }
        }

        minute = reached(&rows, order);

        // A file not read yet can only bring the minute nearer the page's end, and one wholly
        // beyond the minute cannot.
        let next = candidates.get((read + 1) * FILES_AT_ONCE);

        let settled = minute.is_some_and(|minute| {
            next.is_none_or(|file| match order.descending {
                true => file.time_range.end.0 < minute.saturating_mul(MINUTE),
                false => file.time_range.start.0 >= minute.saturating_add(1).saturating_mul(MINUTE),
            })
        });

        if settled {
            break;
        }
    }

    let minute = minute?;

    let narrowed = match order.descending {
        true => TimeRange::new(
            TimestampMicros(window.start.0.max(minute.saturating_mul(MINUTE))),
            window.end,
        ),
        false => TimeRange::new(
            window.start,
            TimestampMicros(
                window
                    .end
                    .0
                    .min(minute.saturating_add(1).saturating_mul(MINUTE)),
            ),
        ),
    };

    (narrowed.start.0 > window.start.0 || narrowed.end.0 < window.end.0).then_some(narrowed)
}

/// The minute where the counted rows, taken from the page's end, first reach the page's size.
fn reached(rows: &BTreeMap<i64, u64>, order: TimeOrder) -> Option<i64> {
    let mut seen = 0_u64;

    let mut reach = |(minute, count): (&i64, &u64)| {
        seen = seen.saturating_add(*count);
        (seen >= order.fetch as u64).then_some(*minute)
    };

    match order.descending {
        true => rows.iter().rev().find_map(&mut reach),
        false => rows.iter().find_map(&mut reach),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(descending: bool, fetch: usize) -> TimeOrder {
        TimeOrder { descending, fetch }
    }

    #[test]
    fn the_minute_is_where_the_rows_from_the_end_reach_the_page() {
        let rows = BTreeMap::from([(10, 3), (11, 0), (12, 4), (13, 2)]);

        assert_eq!(reached(&rows, order(true, 2)), Some(13));
        assert_eq!(reached(&rows, order(true, 3)), Some(12));
        assert_eq!(reached(&rows, order(true, 9)), Some(10));
        assert_eq!(reached(&rows, order(true, 10)), None);
        assert_eq!(reached(&rows, order(false, 3)), Some(10));
        assert_eq!(reached(&rows, order(false, 4)), Some(12));
    }
}
