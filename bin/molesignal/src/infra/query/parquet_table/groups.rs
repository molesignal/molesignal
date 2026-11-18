// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! How a scan's files become DataFusion execution partitions. DataFusion runs a scan's partitions
//! at once and reads each partition's files one after another, so a scan reads as many files at
//! once as it has groups. Each query scan gets at most its session's parallelism of them, however
//! many files it reads.

use std::{cmp::Reverse, sync::Arc};

use arrow::datatypes::{DataType, Schema, TimeUnit};
use datafusion::{
    common::{ColumnStatistics, ScalarValue, Statistics, stats::Precision},
    datasource::{listing::PartitionedFile, physical_plan::FileGroup},
};
use kernel::time::TimeRange;

/// A file to scan and the span of its rows' `_timestamp`, both ends included, as the writer
/// recorded it.
#[derive(Debug, Clone)]
pub(super) struct ScanFile {
    pub(super) file: PartitionedFile,
    pub(super) time: TimeRange,
    /// The file's rows are clustered (see [`Clustering`](domain::stream::Clustering)): in
    /// time slices, not in the storage order.
    pub(super) clustered: bool,
    /// A piece of the file, a run of its rows that one partition reads (see
    /// [`pieces`](super::pieces)), rather than the whole file.
    pub(super) piece: bool,
}

/// The order a scan's groups keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Order {
    /// One file per group: each group is in its file's storage order.
    PerFile,
    /// Each group lists its files newest first, none of them overlapping the next in time: each
    /// group is in `_timestamp` descending order.
    NewestFirst,
    /// The groups keep no order.
    Unordered,
}

/// `files` in at most `width` groups. With `by_time`, files that follow each other in time share
/// a group in that order, so the groups keep `_timestamp` descending order across files; when
/// more than `width` files overlap, or without `by_time`, the groups keep no order. Neither do
/// they when a file is clustered: its rows are not in the storage order, and DataFusion sorts
/// where a query needs an order.
pub(super) fn group(files: &[ScanFile], width: usize, by_time: bool) -> (Vec<FileGroup>, Order) {
    let width = width.max(1);
    let ordered = files.iter().all(|file| !file.clustered);

    if files.len() <= width {
        let groups = files
            .iter()
            .map(|file| FileGroup::new(vec![file.file.clone()]))
            .collect();

        let order = if ordered {
            Order::PerFile
        } else {
            Order::Unordered
        };

        return (groups, order);
    }

    // Of files that end together, the one that starts latest goes first: the others, which end
    // where it starts, can follow it.
    let mut newest_first: Vec<&ScanFile> = files.iter().collect();
    newest_first.sort_by_key(|file| Reverse((file.time.end.0, file.time.start.0)));

    if by_time
        && ordered
        && let Some(groups) = chains(&newest_first, width)
    {
        return (groups, Order::NewestFirst);
    }

    (spread(&newest_first, width), Order::Unordered)
}

/// The files, newest first, in `width` groups that each list theirs newest first with none
/// overlapping the next: a file follows a group's last file only if it ends no later than that
/// one starts. `None` when more than `width` files overlap.
fn chains(newest_first: &[&ScanFile], width: usize) -> Option<Vec<FileGroup>> {
    let mut chains: Vec<Chain> = Vec::with_capacity(width);

    for file in newest_first {
        // A new group while there is room: more groups read more files at once. Starting one
        // never leaves a later file without a group it could have joined.
        if chains.len() < width {
            chains.push(Chain::new(file));
            continue;
        }

        // Every group open to this file stays open to the later files, which end no later, so
        // the choice among them only balances the groups.
        let chain = chains
            .iter_mut()
            .filter(|chain| file.time.end.0 <= chain.earliest)
            .min_by_key(|chain| chain.bytes)?;

        chain.push(file);
    }

    Some(
        chains
            .into_iter()
            .map(|chain| FileGroup::new(chain.files))
            .collect(),
    )
}

/// A group of files that follow each other in time, newest first.
struct Chain {
    files: Vec<PartitionedFile>,
    /// The earliest row of the last file.
    earliest: i64,
    bytes: u64,
}

impl Chain {
    fn new(file: &ScanFile) -> Self {
        Self {
            files: vec![file.file.clone()],
            earliest: file.time.start.0,
            bytes: file.file.effective_size(),
        }
    }

    fn push(&mut self, file: &ScanFile) {
        self.files.push(file.file.clone());
        self.earliest = file.time.start.0;
        self.bytes += file.file.effective_size();
    }
}

/// The files, newest first, dealt round the `width` groups. Partitions that keep no order take
/// their next file from a queue they share, so how the files are dealt only sets where each
/// partition starts.
fn spread(newest_first: &[&ScanFile], width: usize) -> Vec<FileGroup> {
    let mut groups = vec![Vec::new(); width];

    for (index, file) in newest_first.iter().enumerate() {
        groups[index % width].push(file.file.clone());
    }

    groups.into_iter().map(FileGroup::new).collect()
}

/// The statistics DataFusion reads to check that a group's files follow each other in time and to
/// skip files a query's `_timestamp` bounds rule out: the span of the column at `index`, which
/// must be a microsecond timestamp no row leaves NULL. Nothing for any other column, nor a row
/// count: DataFusion would answer some aggregates from it without applying the scan's filters.
pub(super) fn time_statistics(
    schema: &Schema,
    index: usize,
    time: TimeRange,
) -> Option<Arc<Statistics>> {
    let DataType::Timestamp(TimeUnit::Microsecond, zone) = schema.fields().get(index)?.data_type()
    else {
        return None;
    };

    let bound = |micros: i64| {
        Precision::Exact(ScalarValue::TimestampMicrosecond(
            Some(micros),
            zone.clone(),
        ))
    };

    let mut statistics = Statistics::new_unknown(schema);

    statistics.column_statistics[index] = ColumnStatistics::new_unknown()
        .with_min_value(bound(time.start.0))
        .with_max_value(bound(time.end.0))
        .with_null_count(Precision::Exact(0));

    Some(Arc::new(statistics))
}

#[cfg(test)]
mod tests {
    use kernel::time::TimestampMicros;

    use super::*;

    fn file(name: &str, start: i64, end: i64) -> ScanFile {
        ScanFile {
            file: PartitionedFile::new(name, 100),
            time: TimeRange::new(TimestampMicros(start), TimestampMicros(end)),
            clustered: false,
            piece: false,
        }
    }

    #[test]
    fn a_clustered_file_leaves_the_scan_without_an_order() {
        let clustered = ScanFile {
            clustered: true,
            ..file("c", 0, 9)
        };

        let (groups, order) = group(&[file("a", 20, 29), clustered.clone()], 4, true);

        assert_eq!(groups.len(), 2);
        assert_eq!(order, Order::Unordered);

        let files = [
            file("a", 30, 39),
            file("b", 20, 29),
            file("d", 10, 19),
            clustered,
        ];
        let (groups, order) = group(&files, 2, true);

        assert_eq!(groups.iter().map(|group| group.len()).sum::<usize>(), 4);
        assert_eq!(order, Order::Unordered);
    }

    fn names(groups: &[FileGroup]) -> Vec<Vec<String>> {
        groups
            .iter()
            .map(|group| {
                group
                    .iter()
                    .map(|file| file.object_meta.location.to_string())
                    .collect()
            })
            .collect()
    }

    /// Hourly files, one after another, the oldest first as a catalog lists them.
    fn hours(count: i64) -> Vec<ScanFile> {
        (0..count)
            .map(|hour| file(&format!("h{hour}"), hour * 100, hour * 100 + 99))
            .collect()
    }

    #[test]
    fn as_many_files_as_the_width_each_get_a_group() {
        let (groups, order) = group(&hours(3), 3, true);

        assert_eq!(order, Order::PerFile);
        assert_eq!(names(&groups), [["h0"], ["h1"], ["h2"]]);
    }

    #[test]
    fn files_that_follow_each_other_share_groups_newest_first() {
        let (groups, order) = group(&hours(7), 3, true);

        assert_eq!(order, Order::NewestFirst);
        assert_eq!(
            names(&groups),
            [vec!["h6", "h3", "h0"], vec!["h5", "h2"], vec!["h4", "h1"]]
        );
    }

    #[test]
    fn files_touching_at_one_moment_follow_each_other() {
        let files = [file("a", 10, 20), file("b", 0, 10), file("c", 20, 30)];
        let (groups, order) = group(&files, 1, true);

        assert_eq!(order, Order::NewestFirst);
        assert_eq!(names(&groups), [["c", "a", "b"]]);
    }

    #[test]
    fn overlapping_files_fit_when_no_more_than_the_width_overlap() {
        // Two files overlap at any moment: two groups keep the order.
        let files = [
            file("a", 0, 15),
            file("b", 10, 25),
            file("c", 20, 35),
            file("d", 30, 45),
        ];

        let (groups, order) = group(&files, 2, true);

        assert_eq!(order, Order::NewestFirst);
        assert_eq!(names(&groups), [vec!["d", "b"], vec!["c", "a"]]);
    }

    #[test]
    fn more_overlapping_files_than_the_width_keep_no_order() {
        // Three files share the moment 12.
        let files = [
            file("a", 0, 20),
            file("b", 5, 15),
            file("c", 10, 30),
            file("d", 40, 50),
        ];

        let (groups, order) = group(&files, 2, true);

        assert_eq!(order, Order::Unordered);
        assert_eq!(names(&groups), [vec!["d", "a"], vec!["c", "b"]]);
    }

    #[test]
    fn without_a_time_order_files_are_dealt_round_the_groups() {
        let (groups, order) = group(&hours(5), 2, false);

        assert_eq!(order, Order::Unordered);
        assert_eq!(names(&groups), [vec!["h4", "h2", "h0"], vec!["h3", "h1"]]);
    }

    #[test]
    fn a_file_joins_the_group_holding_the_fewest_bytes() {
        let mut files = hours(4);
        files[3].file = PartitionedFile::new("h3", 1_000);

        // h3 and h2 start the groups; h3 alone outweighs the other three.
        let (groups, _) = group(&files, 2, true);

        assert_eq!(names(&groups), [vec!["h3"], vec!["h2", "h1", "h0"]]);
    }

    #[test]
    fn statistics_carry_the_span_in_the_column_type() {
        let schema = Schema::new(vec![
            arrow::datatypes::Field::new("value", DataType::Int64, true),
            arrow::datatypes::Field::new(
                "_timestamp",
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
        ]);

        let time = TimeRange::new(TimestampMicros(5), TimestampMicros(9));
        let statistics = time_statistics(&schema, 1, time).unwrap();
        let column = &statistics.column_statistics[1];

        assert_eq!(
            column.min_value,
            Precision::Exact(ScalarValue::TimestampMicrosecond(
                Some(5),
                Some("UTC".into())
            ))
        );
        assert_eq!(
            column.max_value,
            Precision::Exact(ScalarValue::TimestampMicrosecond(
                Some(9),
                Some("UTC".into())
            ))
        );
        assert_eq!(column.null_count, Precision::Exact(0));
        assert_eq!(
            statistics.column_statistics[0],
            ColumnStatistics::new_unknown()
        );
        assert_eq!(statistics.num_rows, Precision::Absent);
        assert!(time_statistics(&schema, 0, time).is_none());
    }
}
