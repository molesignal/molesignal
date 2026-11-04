// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Which row groups and rows of a file to read. Row-group statistics rule out whole row groups;
//! when the page index is loaded, the pages whose statistics rule a value out are skipped too.

use std::ops::Range;

use parquet::{
    arrow::arrow_reader::{RowSelection, RowSelector},
    file::{
        metadata::ParquetMetaData, page_index::column_index::ColumnIndexMetaData,
        statistics::Statistics,
    },
};

/// What the rows read must be able to hold in one leaf column.
#[derive(Clone, Copy)]
pub(super) enum RowTest<'a> {
    /// Text equal to this value.
    Equals(&'a [u8]),
    /// A time in `[start, end)`, in microseconds.
    Within(i64, i64),
}

/// The row groups that may hold rows passing every test, and the rows to read within them when
/// the page index rules some out. Missing statistics keep what they would have decided on.
pub(super) fn select(
    metadata: &ParquetMetaData,
    tests: &[(usize, RowTest<'_>)],
) -> (Vec<usize>, Option<RowSelection>) {
    let mut row_groups = Vec::with_capacity(metadata.num_row_groups());
    let mut selectors = Vec::new();
    let mut partial = false;

    for row_group in 0..metadata.num_row_groups() {
        if !tests
            .iter()
            .all(|&(column, test)| row_group_may_match(metadata, row_group, column, test))
        {
            continue;
        }

        let rows = usize::try_from(metadata.row_group(row_group).num_rows()).unwrap_or_default();
        let mut ranges = std::iter::once(0..rows).collect::<Vec<_>>();

        for &(column, test) in tests {
            if let Some(kept) = kept_rows(metadata, row_group, column, test) {
                ranges = intersect(&ranges, &kept);
            }
        }

        if ranges.is_empty() {
            continue;
        }

        partial |= ranges.len() != 1 || ranges[0] != (0..rows);
        row_groups.push(row_group);

        let mut at = 0;

        for range in ranges {
            if range.start > at {
                selectors.push(RowSelector::skip(range.start - at));
            }

            selectors.push(RowSelector::select(range.end - range.start));
            at = range.end;
        }

        if rows > at {
            selectors.push(RowSelector::skip(rows - at));
        }
    }

    (row_groups, partial.then(|| RowSelection::from(selectors)))
}

fn row_group_may_match(
    metadata: &ParquetMetaData,
    row_group: usize,
    column: usize,
    test: RowTest<'_>,
) -> bool {
    let statistics = metadata.row_group(row_group).column(column).statistics();

    match (test, statistics) {
        (RowTest::Equals(value), Some(Statistics::ByteArray(statistics))) => {
            match (statistics.min_bytes_opt(), statistics.max_bytes_opt()) {
                (Some(min), Some(max)) => min <= value && value <= max,
                _ => true,
            }
        }
        (RowTest::Within(start, end), Some(Statistics::Int64(statistics))) => {
            match (statistics.min_opt(), statistics.max_opt()) {
                (Some(&min), Some(&max)) => max >= start && min < end,
                _ => true,
            }
        }
        _ => true,
    }
}

/// The rows of `row_group` in pages that may pass `test`, as ranges. `None` when the page index
/// cannot tell, which keeps every row.
fn kept_rows(
    metadata: &ParquetMetaData,
    row_group: usize,
    column: usize,
    test: RowTest<'_>,
) -> Option<Vec<Range<usize>>> {
    let index = metadata.column_index()?.get(row_group)?.get(column)?;

    let locations = metadata
        .offset_index()?
        .get(row_group)?
        .get(column)?
        .page_locations();

    if matches!(index, ColumnIndexMetaData::NONE)
        || usize::try_from(index.num_pages()).ok()? != locations.len()
    {
        return None;
    }

    let rows = usize::try_from(metadata.row_group(row_group).num_rows()).ok()?;
    let mut kept: Vec<Range<usize>> = Vec::new();

    for (page, location) in locations.iter().enumerate() {
        if !page_may_match(index, page, test)? {
            continue;
        }

        let start = usize::try_from(location.first_row_index).ok()?;

        let end = match locations.get(page + 1) {
            Some(next) => usize::try_from(next.first_row_index).ok()?,
            None => rows,
        };

        match kept.last_mut() {
            Some(last) if last.end == start => last.end = end,
            _ => kept.push(start..end),
        }
    }

    Some(kept)
}

/// `None` when the column index holds another type than `test` reads.
fn page_may_match(index: &ColumnIndexMetaData, page: usize, test: RowTest<'_>) -> Option<bool> {
    if index.is_null_page(page) {
        return Some(false);
    }

    match (test, index) {
        (RowTest::Equals(value), ColumnIndexMetaData::BYTE_ARRAY(index)) => {
            Some(match (index.min_value(page), index.max_value(page)) {
                (Some(min), Some(max)) => min <= value && value <= max,
                _ => true,
            })
        }
        (RowTest::Within(start, end), ColumnIndexMetaData::INT64(index)) => {
            Some(match (index.min_value(page), index.max_value(page)) {
                (Some(&min), Some(&max)) => max >= start && min < end,
                _ => true,
            })
        }
        _ => None,
    }
}

fn intersect(left: &[Range<usize>], right: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut both = Vec::new();
    let (mut i, mut j) = (0, 0);

    while i < left.len() && j < right.len() {
        let start = left[i].start.max(right[j].start);
        let end = left[i].end.min(right[j].end);

        if start < end {
            both.push(start..end);
        }

        if left[i].end < right[j].end {
            i += 1;
        } else {
            j += 1;
        }
    }

    both
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(ranges: &[(usize, usize)]) -> Vec<Range<usize>> {
        ranges.iter().map(|&(start, end)| start..end).collect()
    }

    #[test]
    fn intersect_keeps_the_rows_in_both() {
        assert_eq!(
            intersect(&rows(&[(0, 10), (20, 30)]), &rows(&[(5, 25)])),
            rows(&[(5, 10), (20, 25)])
        );
        assert_eq!(intersect(&rows(&[(0, 10)]), &rows(&[(10, 20)])), rows(&[]));
        assert_eq!(
            intersect(&rows(&[(0, 10)]), &rows(&[(0, 10)])),
            rows(&[(0, 10)])
        );
    }
}
