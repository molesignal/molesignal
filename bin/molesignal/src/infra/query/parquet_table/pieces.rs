// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Pieces: runs of a file's rows that one partition reads on its own, so that a scan of fewer
//! files than partitions still keeps every partition busy, each about as busy as the others.
//!
//! Without them, DataFusion splits such a scan into equal byte ranges, and a row group belongs to
//! the range its first page falls in. A file of one row group was read by one partition however
//! large it was, and where row groups and ranges did not line up, one partition read two row groups
//! while another read none: the same hour of data could take almost twice as long, depending on
//! how it had been merged.

use std::{borrow::Cow, sync::Arc};

use datafusion::{
    catalog::Session,
    datasource::physical_plan::parquet::{ParquetRowSelection, metadata::DFParquetMetadata},
    execution::object_store::ObjectStoreUrl,
};
use futures::future::try_join_all;
use kernel::time::TimeRange;
use object_store::ObjectStore;
use parquet::{
    arrow::arrow_reader::{RowSelection, RowSelector},
    basic::{LogicalType, TimeUnit, Type as PhysicalType},
    file::{
        metadata::{ParquetMetaData, RowGroupMetaData},
        page_index::column_index::ColumnIndexMetaData,
        statistics::Statistics,
    },
};

use super::{FOOTER_READ_BYTES, ParquetMetadataCache, groups::ScanFile};
use crate::infra::storage::{arrow_schema::TS_COL, filters::BLOCK_ROWS};

/// The fewest rows worth a piece of their own: fewer cost about as much to open as to read.
const MIN_PIECE_ROWS: usize = 16 * BLOCK_ROWS;

/// Cuts the files of a query's scans into pieces. It reads their footers through the metadata
/// cache, where the scan's reader finds them again when it opens the files.
pub(super) struct Cutter {
    store: Arc<dyn ObjectStore>,
    cache: ParquetMetadataCache,
    /// A scan of fewer bytes reads its files whole, as DataFusion splits no scan smaller than its
    /// `repartition_file_min_size`.
    min_bytes: u64,
}

impl Cutter {
    /// `None` when the session splits no file scan, or without a metadata cache: the footers read
    /// here would then be read once more when the files open. Also without the store, whose
    /// absence the scan reports when it reads a file.
    pub(super) fn new(
        state: &dyn Session,
        url: &ObjectStoreUrl,
        cache: Option<&ParquetMetadataCache>,
    ) -> Option<Self> {
        let optimizer = &state.config().options().optimizer;

        if !optimizer.repartition_file_scans {
            return None;
        }

        let cache = cache?.clone();

        Some(Self {
            store: state.runtime_env().object_store(url).ok()?,
            cache,
            min_bytes: u64::try_from(optimizer.repartition_file_min_size).unwrap_or(u64::MAX),
        })
    }

    /// `files` cut into at most `width` pieces in all, at least one a file. `None` to read them
    /// whole: as many files as partitions, a small scan, a footer that could not be read, or too
    /// few rows to cut.
    async fn pieces(
        &self,
        files: &[ScanFile],
        width: usize,
        window: Option<TimeRange>,
    ) -> Option<Vec<ScanFile>> {
        let bytes: u64 = files
            .iter()
            .map(|scanned| scanned.file.effective_size())
            .sum();

        if files.is_empty() || files.len() >= width || bytes < self.min_bytes {
            return None;
        }

        let footers = try_join_all(files.iter().map(|scanned| async move {
            DFParquetMetadata::new(self.store.as_ref(), &scanned.file.object_meta)
                .with_file_metadata_cache(Some(self.cache.clone()))
                .with_metadata_size_hint(Some(FOOTER_READ_BYTES))
                .fetch_metadata()
                .await
        }))
        .await
        .inspect_err(|error| tracing::debug!(%error, "files read whole: a footer failed to read"))
        .ok()?;

        // The rows each file has to read: those of the pages the window may fall in, and of those
        // the ones an index chose.
        let selections: Vec<RowSelection> = files
            .iter()
            .zip(&footers)
            .map(|(scanned, footer)| {
                let candidates = candidates(footer, window);

                match scanned.file.extension::<ParquetRowSelection>() {
                    Some(chosen) => candidates.intersection(chosen.selection()),
                    None => candidates,
                }
            })
            .collect();

        let rows: Vec<usize> = selections.iter().map(RowSelection::row_count).collect();
        let counts = allocate(&rows, width);

        if counts.iter().all(|&count| count == 1) {
            return None;
        }

        let mut pieces = Vec::with_capacity(width);

        for ((scanned, footer), (selection, count)) in files
            .iter()
            .zip(&footers)
            .zip(selections.iter().zip(counts))
        {
            let group_rows: Vec<usize> = footer.row_groups().iter().map(rows_of).collect();

            pieces.extend(cut(selection, &group_rows, count).into_iter().map(|piece| {
                ScanFile {
                    file: scanned
                        .file
                        .clone()
                        .with_extension(ParquetRowSelection::new(piece)),
                    time: scanned.time,
                    clustered: scanned.clustered,
                    piece: true,
                }
            }));
        }

        Some(pieces)
    }
}

/// `files` as a scan of `width` partitions reads them: in pieces when `cutter` is given and they
/// are fewer than that, otherwise whole. With a `window`, the time window their rows are read
/// through, pieces hold only the pages whose times may fall in it.
pub(super) async fn read<'a>(
    cutter: Option<&Cutter>,
    files: &'a [ScanFile],
    width: usize,
    window: Option<TimeRange>,
) -> Cow<'a, [ScanFile]> {
    let pieces = match cutter {
        Some(cutter) => cutter.pieces(files, width, window).await,
        None => None,
    };

    pieces.map_or(Cow::Borrowed(files), Cow::Owned)
}

/// How many pieces each file of `rows` rows gets: `width` in all at most, at least one each. The
/// next piece goes to the file whose pieces are the largest, while its pieces keep
/// [`MIN_PIECE_ROWS`] each.
fn allocate(rows: &[usize], width: usize) -> Vec<usize> {
    let mut counts = vec![1; rows.len()];

    for _ in rows.len()..width {
        let largest = (0..rows.len())
            .filter(|&file| rows[file] / (counts[file] + 1) >= MIN_PIECE_ROWS)
            .max_by(|&left, &right| {
                (rows[left] * counts[right])
                    .cmp(&(rows[right] * counts[left]))
                    .then(right.cmp(&left))
            });

        let Some(file) = largest else {
            break;
        };

        counts[file] += 1;
    }

    counts
}

/// `selection`, over every row of a file whose row groups hold `group_rows` rows each, cut into
/// `count` pieces that select about as many rows each. A piece selects only in its own run of the
/// file's rows; the runs follow each other, and each starts on a block of the row group it begins
/// in, so that pieces of a file with block filters share no page. A piece that would select no row
/// is left out.
fn cut(selection: &RowSelection, group_rows: &[usize], count: usize) -> Vec<RowSelection> {
    let total: usize = group_rows.iter().sum();
    let selected = selection.row_count();

    if count <= 1 || selected == 0 {
        return vec![selection.clone()];
    }

    let mut bounds = vec![0];
    bounds.extend(
        (1..count)
            .map(|piece| block_start(group_rows, row_of(selection, selected * piece / count))),
    );
    bounds.push(total);

    bounds
        .windows(2)
        .filter(|run| run[0] < run[1])
        .map(|run| {
            selection.intersection(&RowSelection::from(vec![
                RowSelector::skip(run[0]),
                RowSelector::select(run[1] - run[0]),
                RowSelector::skip(total - run[1]),
            ]))
        })
        .filter(RowSelection::selects_any)
        .collect()
}

/// The row of the file that holds the selected row numbered `target`, counting from 0.
fn row_of(selection: &RowSelection, target: usize) -> usize {
    let mut row = 0;
    let mut seen = 0;

    for selector in selection.iter() {
        if !selector.skip {
            if seen + selector.row_count > target {
                return row + (target - seen);
            }

            seen += selector.row_count;
        }

        row += selector.row_count;
    }

    row
}

/// `row` moved to the nearest start of a block of the row group holding it, or to that group's end.
fn block_start(group_rows: &[usize], row: usize) -> usize {
    let mut start = 0;

    for &rows in group_rows {
        if row < start + rows {
            let offset = (row - start + BLOCK_ROWS / 2) / BLOCK_ROWS * BLOCK_ROWS;
            return start + offset.min(rows);
        }

        start += rows;
    }

    start
}

/// The rows of the file of `footer` that may hold a time in `window`: all of them without a
/// window, or when the file records no microsecond `_timestamp`. Row groups whose statistics leave
/// the window out are skipped, and so are the pages of the others whose page index does.
fn candidates(footer: &ParquetMetaData, window: Option<TimeRange>) -> RowSelection {
    let groups = footer.row_groups();

    let (Some(window), Some(column)) = (window, timestamp_column(footer)) else {
        return groups
            .iter()
            .map(|group| RowSelector::select(rows_of(group)))
            .collect();
    };

    let mut selectors = Vec::new();

    for (index, group) in groups.iter().enumerate() {
        let rows = rows_of(group);

        if !may_hold(window, group_span(group, column)) {
            selectors.push(RowSelector::skip(rows));
            continue;
        }

        let Some(pages) = page_spans(footer, index, column, rows) else {
            selectors.push(RowSelector::select(rows));
            continue;
        };

        selectors.extend(pages.into_iter().map(|page| {
            if may_hold(window, page.span) {
                RowSelector::select(page.rows)
            } else {
                RowSelector::skip(page.rows)
            }
        }));
    }

    selectors.into_iter().collect()
}

fn rows_of(group: &RowGroupMetaData) -> usize {
    usize::try_from(group.num_rows()).unwrap_or(0)
}

/// The index of the file's `_timestamp` column, when it holds microseconds: its statistics then
/// compare with a [`TimeRange`] as they are.
fn timestamp_column(footer: &ParquetMetaData) -> Option<usize> {
    footer
        .file_metadata()
        .schema_descr()
        .columns()
        .iter()
        .position(|column| {
            column.path().parts() == [TS_COL]
                && column.physical_type() == PhysicalType::INT64
                && matches!(
                    column.logical_type_ref(),
                    Some(LogicalType::Timestamp(timestamp)) if matches!(timestamp.unit, TimeUnit::MICROS)
                )
        })
}

/// The earliest and the latest time of some rows, as statistics bound them.
#[derive(Debug, Clone, Copy)]
struct Span {
    min: i64,
    max: i64,
}

/// A page of a row group: how many rows it holds, and the span of their times when the page
/// index has it.
struct Page {
    rows: usize,
    span: Option<Span>,
}

/// The span of the times in `group`'s `column`, as its statistics bound them.
fn group_span(group: &RowGroupMetaData, column: usize) -> Option<Span> {
    match group.column(column).statistics()? {
        Statistics::Int64(values) => Some(Span {
            min: *values.min_opt()?,
            max: *values.max_opt()?,
        }),
        _ => None,
    }
}

/// The pages of `column` in the row group `group`, which holds `rows` rows, from the page index;
/// `None` without one.
fn page_spans(
    footer: &ParquetMetaData,
    group: usize,
    column: usize,
    rows: usize,
) -> Option<Vec<Page>> {
    let ColumnIndexMetaData::INT64(index) = footer.column_index()?.get(group)?.get(column)? else {
        return None;
    };

    let starts = footer
        .offset_index()?
        .get(group)?
        .get(column)?
        .page_locations()
        .iter()
        .map(|page| usize::try_from(page.first_row_index).ok())
        .collect::<Option<Vec<usize>>>()?;

    let consistent = starts.first() == Some(&0)
        && starts.windows(2).all(|pair| pair[0] < pair[1])
        && starts.last().is_some_and(|&last| last < rows)
        && u64::try_from(starts.len()).ok() == Some(index.num_pages());

    if !consistent {
        return None;
    }

    let pages = starts.iter().enumerate().map(|(page, &start)| {
        let end = starts.get(page + 1).copied().unwrap_or(rows);
        let span = index
            .min_value(page)
            .zip(index.max_value(page))
            .map(|(min, max)| Span {
                min: *min,
                max: *max,
            });

        Page {
            rows: end - start,
            span,
        }
    });

    Some(pages.collect())
}

/// Whether rows whose times span `span` may hold a time in `window`; without a span they may.
fn may_hold(window: TimeRange, span: Option<Span>) -> bool {
    span.is_none_or(|span| span.max >= window.start.0 && span.min < window.end.0)
}

#[cfg(test)]
mod tests {
    use arrow::{
        array::{Int64Array, RecordBatch, TimestampMicrosecondArray},
        datatypes::{DataType, Field, Schema, TimeUnit as ArrowTimeUnit},
    };
    use bytes::Bytes;
    use kernel::time::TimestampMicros;
    use parquet::{
        arrow::ArrowWriter,
        file::{
            metadata::{PageIndexPolicy, ParquetMetaDataReader},
            properties::WriterProperties,
        },
    };

    use super::*;

    /// The runs of rows `selection` selects, as `[start, end)` row numbers of the file.
    fn runs(selection: &RowSelection) -> Vec<(usize, usize)> {
        let mut runs = Vec::new();
        let mut row = 0;

        for selector in selection.iter() {
            if !selector.skip {
                runs.push((row, row + selector.row_count));
            }

            row += selector.row_count;
        }

        runs
    }

    fn rows_covered(selection: &RowSelection) -> usize {
        selection.row_count() + selection.skipped_row_count()
    }

    /// Each piece covers every row of the file, no row is selected by two pieces, and together
    /// they select exactly the rows of `selection`.
    fn assert_pieces_of(selection: &RowSelection, pieces: &[RowSelection]) {
        let total = rows_covered(selection);
        let mut union = RowSelection::from(vec![RowSelector::skip(total)]);

        for piece in pieces {
            assert_eq!(rows_covered(piece), total, "{piece:?}");
            assert!(!union.intersection(piece).selects_any(), "{pieces:?}");

            union = union.union(piece);
        }

        assert_eq!(runs(&union), runs(selection));
    }

    #[test]
    fn the_next_piece_goes_to_the_file_whose_pieces_are_largest() {
        assert_eq!(allocate(&[3_225_806, 72_645], 3), [2, 1]);
        assert_eq!(allocate(&[806_452], 3), [3]);
        assert_eq!(allocate(&[100_000, 100_000], 3), [2, 1]);
        assert_eq!(allocate(&[100_000, 300_000], 4), [1, 3]);
    }

    #[test]
    fn a_piece_keeps_the_fewest_rows_worth_opening_a_file_for() {
        assert_eq!(allocate(&[2 * MIN_PIECE_ROWS - 1], 4), [1]);
        assert_eq!(allocate(&[2 * MIN_PIECE_ROWS], 4), [2]);
        assert_eq!(allocate(&[0, 0], 3), [1, 1]);
    }

    #[test]
    fn pieces_select_as_many_rows_each_and_start_on_blocks() {
        let groups = [4 * BLOCK_ROWS, 4 * BLOCK_ROWS, 1_000];
        let all = RowSelection::from(vec![RowSelector::select(groups.iter().sum())]);

        let pieces = cut(&all, &groups, 3);

        assert_pieces_of(&all, &pieces);
        assert_eq!(
            pieces.iter().map(runs).collect::<Vec<_>>(),
            [
                vec![(0, 3 * BLOCK_ROWS)],
                vec![(3 * BLOCK_ROWS, 6 * BLOCK_ROWS)],
                vec![(6 * BLOCK_ROWS, 8 * BLOCK_ROWS + 1_000)],
            ]
        );
    }

    #[test]
    fn pieces_share_out_the_selected_rows_not_the_file() {
        let groups = [8 * BLOCK_ROWS, 8 * BLOCK_ROWS];

        // 1,000 rows selected near the start, 6,000 in the second row group.
        let chosen = RowSelection::from(vec![
            RowSelector::skip(100),
            RowSelector::select(1_000),
            RowSelector::skip(9_000 - 1_100),
            RowSelector::select(6_000),
            RowSelector::skip(16 * BLOCK_ROWS - 15_000),
        ]);

        let pieces = cut(&chosen, &groups, 2);

        assert_pieces_of(&chosen, &pieces);
        assert_eq!(
            pieces.iter().map(runs).collect::<Vec<_>>(),
            [vec![(100, 1_100), (9_000, 11_264)], vec![(11_264, 15_000)]]
        );
    }

    #[test]
    fn a_selection_of_no_rows_stays_whole() {
        let groups = [4 * BLOCK_ROWS];
        let none = RowSelection::from(vec![RowSelector::skip(4 * BLOCK_ROWS)]);

        assert_eq!(cut(&none, &groups, 3), std::slice::from_ref(&none));
    }

    /// One file of rows at the times `0..rows`, newest first, in row groups of `group_rows` rows
    /// and pages of a block, with its footer and page index read back.
    fn footer(rows: i64, group_rows: usize) -> ParquetMetaData {
        let schema = Arc::new(Schema::new(vec![
            Field::new(
                TS_COL,
                DataType::Timestamp(ArrowTimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
            Field::new("value", DataType::Int64, false),
        ]));

        let times: Vec<i64> = (0..rows).rev().collect();
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(TimestampMicrosecondArray::from(times.clone()).with_timezone("UTC")),
                Arc::new(Int64Array::from(times)),
            ],
        )
        .unwrap();

        let properties = WriterProperties::builder()
            .set_max_row_group_row_count(Some(group_rows))
            .set_data_page_row_count_limit(BLOCK_ROWS)
            .set_write_batch_size(BLOCK_ROWS)
            .build();

        let mut bytes = Vec::new();
        let mut writer = ArrowWriter::try_new(&mut bytes, schema, Some(properties)).unwrap();
        writer.write(&batch).unwrap();
        writer.close().unwrap();

        ParquetMetaDataReader::new()
            .with_page_index_policy(PageIndexPolicy::Required)
            .parse_and_finish(&Bytes::from(bytes))
            .unwrap()
    }

    fn window(start: i64, end: i64) -> Option<TimeRange> {
        Some(TimeRange::new(TimestampMicros(start), TimestampMicros(end)))
    }

    #[test]
    fn without_a_window_every_row_is_a_candidate() {
        let footer = footer(10_000, 4_096);

        assert_eq!(runs(&candidates(&footer, None)), [(0, 10_000)]);
    }

    #[test]
    fn candidates_are_the_pages_whose_times_the_window_may_hold() {
        // Row r holds the time 9,999 - r: rows 6,000..8,000 hold the times 2,000..4,000.
        let footer = footer(10_000, 4_096);
        let candidates = candidates(&footer, window(2_000, 4_000));

        assert_eq!(rows_covered(&candidates), 10_000);
        assert_eq!(runs(&candidates), [(5_120, 8_192)]);
    }

    #[test]
    fn a_window_past_every_row_leaves_no_candidate() {
        let footer = footer(10_000, 4_096);

        assert!(!candidates(&footer, window(20_000, 30_000)).selects_any());
    }
}
