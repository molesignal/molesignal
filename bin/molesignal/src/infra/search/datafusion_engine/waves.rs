// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 按行时间取前 n 行的查询：`ORDER BY _timestamp DESC LIMIT n`，升序同理。
//!
//! 文件按时间从新到旧（升序时从旧到新）分批加入，每批对已加入的全部文件执行原语句。凑满 n 行
//! 时，第 n 行的时间就是界线：还没加入的文件里最新的行也早于它（升序时最早的行也晚于它），
//! 这些文件就不可能排进前 n 行，结果与扫描全部文件相同；否则把可能排进来的文件一次补齐，再
//! 执行一遍。时间窗里有上千个文件时，取最新一页通常只读最新的几个。
//!
//! 文件到要加入时才经过索引剪枝，从最新的剪起，剪掉的不读；剪枝数出了每个文件最多有几行满足
//! 条件时，第一批只加到这些行够一页为止：条件只有整值等值时一批就够。数不出时，第一批至少有
//! [`FIRST_WAVE_ROWS`] 行。

use std::{cmp::Reverse, collections::HashMap};

use arrow::array::{Array, RecordBatch, TimestampMicrosecondArray};
use datafusion::{
    common::tree_node::TreeNode,
    logical_expr::{Expr, FetchType, LogicalPlan, SkipType},
};

use crate::domain::storage::QueryFile;

/// 行时间列。
const TIMESTAMP_COLUMN: &str = "_timestamp";
/// 第一批至少加入这么多行的文件。
const FIRST_WAVE_ROWS: u64 = 50_000;
/// 每批加入后的行数是上一批的几倍。
const WAVE_GROWTH: u64 = 4;
/// 第一次一起剪枝的文件数，之后每次加倍：与剪枝的并发数相同，一轮读完它们的索引。
const FIRST_CHECKED_FILES: usize = 16;

/// 语句按行时间排序后取前 `fetch` 行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TimeOrder {
    pub(super) descending: bool,
    pub(super) fetch: usize,
}

/// `plan` 是否只逐行地投影、过滤一张表，再按 `_timestamp` 排序取前 n 行，且结果里有那张表原样
/// 的 `_timestamp` 列。聚合、窗口函数、连接、子查询与 `OFFSET` 都不是：它们的结果取决于全部行。
pub(super) fn time_order(plan: &LogicalPlan) -> Option<TimeOrder> {
    let LogicalPlan::Limit(limit) = plan else {
        return None;
    };

    let (Ok(SkipType::Literal(0)), Ok(FetchType::Literal(Some(fetch)))) =
        (limit.get_skip_type(), limit.get_fetch_type())
    else {
        return None;
    };

    let LogicalPlan::Sort(sort) = limit.input.as_ref() else {
        return None;
    };

    let first = sort.expr.first()?;

    let sorts_by_time =
        matches!(&first.expr, Expr::Column(column) if column.name == TIMESTAMP_COLUMN);

    (sorts_by_time && sort.fetch.is_none() && keeps_timestamp(&sort.input)).then_some(TimeOrder {
        descending: !first.asc,
        fetch,
    })
}

/// `node` 只逐行地投影、过滤一张表，输出里的 `_timestamp` 就是表里的那一列。
fn keeps_timestamp(node: &LogicalPlan) -> bool {
    match node {
        LogicalPlan::Projection(projection) => {
            let fields = projection.schema.fields();

            let passes = fields
                .iter()
                .zip(&projection.expr)
                .any(|(field, expr)| field.name() == TIMESTAMP_COLUMN && is_timestamp(expr));

            fields.len() == projection.expr.len()
                && passes
                && projection.expr.iter().all(row_wise)
                && keeps_timestamp(&projection.input)
        }
        LogicalPlan::Filter(filter) => {
            row_wise(&filter.predicate) && keeps_timestamp(&filter.input)
        }
        LogicalPlan::SubqueryAlias(alias) => keeps_timestamp(&alias.input),
        LogicalPlan::TableScan(scan) => scan.fetch.is_none() && scan.filters.iter().all(row_wise),
        _ => false,
    }
}

fn is_timestamp(expr: &Expr) -> bool {
    let column = match expr {
        Expr::Alias(alias) => alias.expr.as_ref(),
        other => other,
    };

    matches!(column, Expr::Column(column) if column.name == TIMESTAMP_COLUMN)
}

/// 表达式只看一行：没有子查询、窗口函数或聚合。
fn row_wise(expr: &Expr) -> bool {
    let spans_rows = expr.exists(|expr| {
        Ok(matches!(
            expr,
            Expr::ScalarSubquery(_)
                | Expr::Exists(_)
                | Expr::InSubquery(_)
                | Expr::SetComparison(_)
                | Expr::OuterReferenceColumn(..)
                | Expr::WindowFunction(_)
                | Expr::AggregateFunction(_)
        ))
    });

    !spans_rows.unwrap_or(true)
}

/// 下一批至少要有哪些文件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Want {
    /// 比上一批至少多一个文件，直到加入的文件最多能给出这么多行。
    Reach(u64),
    /// 还可能有行排在这个时间或更前面的每个文件。
    Boundary(i64),
    /// 全部文件。
    All,
}

/// 分批读文件的计划：文件按时间排好，记着哪些已经过索引剪枝、已加入多少个。
pub(super) struct Waves {
    order: TimeOrder,
    files: Vec<QueryFile>,
    /// 每个文件最多能给出的行数：剪枝数出的命中数，数不出时是文件的行数。
    reach: Vec<u64>,
    /// `files[..checked]` 已经过剪枝，剪掉的已不在其中。
    checked: usize,
    /// [`Self::unchecked`] 交出、还没交回的文件到这里为止。
    pending: usize,
    /// 下一次至少一起剪枝这么多文件。
    chunk: usize,
    taken: usize,
    /// 已经执行过一批。
    ran: bool,
    want: Want,
}

impl Waves {
    /// `limit` 是请求另给的行数上限，与语句的 `LIMIT` 取小。`check` 时文件还没剪枝：每批先用
    /// [`Self::unchecked`] 取出要剪的文件，剪完用 [`Self::checked`] 交回。`counted` 表示剪枝会
    /// 数出每个文件的命中数，第一批只加到这些命中够一页。
    pub(super) fn new(
        order: TimeOrder,
        mut files: Vec<QueryFile>,
        limit: Option<usize>,
        check: bool,
        counted: bool,
    ) -> Self {
        if order.descending {
            files.sort_by_key(|file| Reverse((file.time_range.end.0, file.time_range.start.0)));
        } else {
            files.sort_by_key(|file| (file.time_range.start.0, file.time_range.end.0));
        }

        let order = TimeOrder {
            fetch: limit.map_or(order.fetch, |limit| limit.min(order.fetch)),
            ..order
        };

        let first = if counted {
            order.fetch as u64
        } else {
            FIRST_WAVE_ROWS.max(order.fetch as u64)
        };

        let checked = if check { 0 } else { files.len() };

        Self {
            order,
            reach: files.iter().map(|file| file.rows).collect(),
            files,
            checked,
            pending: checked,
            chunk: FIRST_CHECKED_FILES,
            taken: 0,
            ran: false,
            want: Want::Reach(first),
        }
    }

    /// 下一批要的文件里还没剪枝的，至少 `chunk` 个一起交出，好一起剪。没有时为 `None`。
    pub(super) fn unchecked(&mut self) -> Option<&[QueryFile]> {
        let needed = self.needed();

        if needed <= self.checked {
            return None;
        }

        self.pending = needed
            .max(self.checked.saturating_add(self.chunk))
            .min(self.files.len());

        Some(&self.files[self.checked..self.pending])
    }

    /// 交回 [`Self::unchecked`] 交出的文件：剪枝留下的 `kept`（顺序不变），与按 object key 数出的
    /// 命中数。
    pub(super) fn checked(&mut self, kept: Vec<QueryFile>, matches: &HashMap<String, u64>) {
        let reach = kept
            .iter()
            .map(|file| {
                matches
                    .get(&file.object_key)
                    .map_or(file.rows, |rows| (*rows).min(file.rows))
            })
            .collect::<Vec<_>>();

        let count = kept.len();

        self.files.splice(self.checked..self.pending, kept);
        self.reach.splice(self.checked..self.pending, reach);
        self.checked += count;
        self.pending = self.checked;
        self.chunk = self.chunk.saturating_mul(2);
    }

    /// 这一批要执行的文件：已加入的全部，都已剪枝。剪枝后没有新文件可读时为 `None`：上一批的
    /// 结果就是答案。在 [`Self::unchecked`] 交不出文件之后调用。
    pub(super) fn take(&mut self) -> Option<&[QueryFile]> {
        let taken = self.needed().min(self.checked);

        if self.ran && taken == self.taken {
            return None;
        }

        self.taken = taken;
        self.ran = true;
        Some(&self.files[..taken])
    }

    /// 读完这一批的结果后决定下一批要什么。`false` 表示结果已与扫描全部文件相同。
    pub(super) fn next(&mut self, batches: &[RecordBatch]) -> bool {
        if self.taken == self.files.len() {
            return false;
        }

        let rows = batches.iter().map(RecordBatch::num_rows).sum::<usize>();

        if rows < self.order.fetch {
            let read = self.reached(self.taken);
            self.want = Want::Reach(read.saturating_mul(WAVE_GROWTH));
            return true;
        }

        // 凑满了却读不出第 n 行的时间：保守地补齐全部文件。
        self.want = match boundary(batches) {
            Some(boundary) => Want::Boundary(boundary),
            None => Want::All,
        };

        self.needed() > self.taken
    }

    /// 下一批要的文件数：`files` 的前这么多个。
    fn needed(&self) -> usize {
        match self.want {
            Want::Reach(rows) => {
                let mut needed = (self.taken + 1).min(self.files.len());

                while needed < self.files.len() && self.reached(needed) < rows {
                    needed += 1;
                }

                needed
            }
            Want::Boundary(boundary) => {
                let descending = self.order.descending;

                let reaching = self.files[self.taken..]
                    .iter()
                    .take_while(|file| {
                        if descending {
                            file.time_range.end.0 >= boundary
                        } else {
                            file.time_range.start.0 <= boundary
                        }
                    })
                    .count();

                self.taken + reaching
            }
            Want::All => self.files.len(),
        }
    }

    /// 前 `count` 个文件最多能给出的行数。
    fn reached(&self, count: usize) -> u64 {
        self.reach[..count].iter().sum()
    }
}

/// 结果最后一行的行时间：排在第 n 位的那一行。
fn boundary(batches: &[RecordBatch]) -> Option<i64> {
    let batch = batches.iter().rev().find(|batch| batch.num_rows() > 0)?;
    let column = batch.column(batch.schema().index_of(TIMESTAMP_COLUMN).ok()?);
    let times = column
        .as_any()
        .downcast_ref::<TimestampMicrosecondArray>()?;
    let last = times.len() - 1;

    (!times.is_null(last)).then(|| times.value(last))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
    use datafusion::{datasource::MemTable, prelude::SessionContext};

    use super::*;
    use crate::{
        domain::stream::StreamType,
        shared::{
            ids::Id,
            time::{TimeRange, TimestampMicros},
        },
    };

    async fn plan(sql: &str) -> LogicalPlan {
        let schema = Arc::new(Schema::new(vec![
            Field::new(
                TIMESTAMP_COLUMN,
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
            Field::new("_event_id", DataType::Utf8, true),
            Field::new("level", DataType::Utf8, true),
        ]));

        let ctx = SessionContext::new();
        let table = MemTable::try_new(schema, vec![vec![]]).unwrap();
        ctx.register_table("logs", Arc::new(table)).unwrap();
        ctx.sql(sql).await.unwrap().logical_plan().clone()
    }

    fn file(start: i64, end: i64, rows: u64) -> QueryFile {
        QueryFile {
            id: Id::from_string(format!("{start}-{end}")),
            org_id: Id::from_string("org"),
            stream: "logs".into(),
            stream_type: StreamType::LOGS,
            dataset_type: crate::domain::storage::primary_dataset_type(StreamType::LOGS).unwrap(),
            object_key: format!("objects/{start}"),
            checksum: None,
            etag: None,
            time_range: TimeRange::new(TimestampMicros(start), TimestampMicros(end)),
            rows,
            size_bytes: rows,
            min_values: Default::default(),
            max_values: Default::default(),
            value_sets: Default::default(),
            address_ranges: Default::default(),
            null_counts: Default::default(),
            indexed_at_micros: None,
            shard: None,
        }
    }

    fn page(times: &[i64]) -> Vec<RecordBatch> {
        let schema = Arc::new(Schema::new(vec![Field::new(
            TIMESTAMP_COLUMN,
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        )]));

        vec![
            RecordBatch::try_new(
                schema,
                vec![Arc::new(
                    TimestampMicrosecondArray::from(times.to_vec()).with_timezone("UTC"),
                )],
            )
            .unwrap(),
        ]
    }

    #[tokio::test]
    async fn recognizes_rows_ordered_by_time_and_cut_to_a_page() {
        assert_eq!(
            time_order(
                &plan(
                    "SELECT *, CAST(_timestamp AS BIGINT) AS t FROM logs WHERE level = 'error' \
                     ORDER BY _timestamp DESC NULLS LAST, _event_id DESC LIMIT 101"
                )
                .await
            ),
            Some(TimeOrder {
                descending: true,
                fetch: 101
            })
        );

        assert_eq!(
            time_order(&plan("SELECT * FROM logs ORDER BY _timestamp LIMIT 5").await),
            Some(TimeOrder {
                descending: false,
                fetch: 5
            })
        );
    }

    #[tokio::test]
    async fn rejects_queries_whose_page_depends_on_every_row() {
        for sql in [
            "SELECT * FROM logs ORDER BY _timestamp DESC",
            "SELECT * FROM logs ORDER BY _timestamp DESC LIMIT 5 OFFSET 5",
            "SELECT * FROM logs ORDER BY _event_id DESC LIMIT 5",
            "SELECT level FROM logs ORDER BY _timestamp DESC LIMIT 5",
            "SELECT level AS _timestamp FROM logs ORDER BY _timestamp DESC LIMIT 5",
            "SELECT *, count(*) OVER () AS n FROM logs ORDER BY _timestamp DESC LIMIT 5",
            "SELECT max(_timestamp) AS _timestamp FROM logs ORDER BY _timestamp DESC LIMIT 5",
            "SELECT DISTINCT * FROM logs ORDER BY _timestamp DESC LIMIT 5",
            "SELECT * FROM logs WHERE level IN (SELECT level FROM logs) \
             ORDER BY _timestamp DESC LIMIT 5",
            "SELECT * FROM (SELECT * FROM logs LIMIT 10) t ORDER BY _timestamp DESC LIMIT 5",
        ] {
            assert_eq!(time_order(&plan(sql).await), None, "{sql}");
        }
    }

    #[test]
    fn waves_grow_until_the_page_fills_and_then_take_only_what_can_rank() {
        let order = TimeOrder {
            descending: true,
            fetch: 2,
        };

        // Ten hourly files of 20 000 rows, given out of order; the newest overlaps the one before.
        let mut files = (0..10)
            .map(|hour| file(hour * 100, hour * 100 + 99, 20_000))
            .collect::<Vec<_>>();
        files.push(file(950, 1_050, 20_000));
        files.reverse();

        let mut waves = Waves::new(order, files.clone(), None, false, false);

        // The first wave holds the newest files up to 50 000 rows.
        let first = waves.take().unwrap();
        assert_eq!(first.len(), 3);
        assert_eq!(first[0].time_range.end.0, 1_050);

        // A short page asks for four times as many rows: here, every file.
        assert!(waves.next(&page(&[1_040])));
        assert_eq!(waves.take().map(<[QueryFile]>::len), Some(11));

        // A full page whose last row is at 750 still needs the file that reaches back to 799,
        // and then no other.
        let mut waves = Waves::new(order, files, None, false, false);
        assert_eq!(waves.take().map(<[QueryFile]>::len), Some(3));
        assert!(waves.next(&page(&[1_040, 750])));
        assert_eq!(waves.take().map(<[QueryFile]>::len), Some(4));
        assert!(!waves.next(&page(&[1_040, 760])));
    }

    #[test]
    fn a_page_from_the_oldest_rows_grows_forward_in_time() {
        let order = TimeOrder {
            descending: false,
            fetch: 1,
        };

        let files = (0..5)
            .map(|hour| file(hour * 100, hour * 100 + 99, 60_000))
            .collect::<Vec<_>>();

        let mut waves = Waves::new(order, files, Some(10), false, false);

        let first = waves.take().unwrap();
        assert_eq!(first[0].time_range.start.0, 0);
        assert_eq!(first.len(), 1);
        assert!(waves.next(&page(&[150])));
        assert_eq!(waves.take().map(<[QueryFile]>::len), Some(2));
        assert!(!waves.next(&page(&[150])));
    }

    /// Counted matches size the first wave to one page: with 30 matching rows in each hourly file
    /// of 20 000, a page of 100 reads the newest four files. Without counts the first wave holds
    /// the newest 50 000 rows, three files.
    #[test]
    fn counted_matches_size_the_first_wave_to_one_page() {
        let order = TimeOrder {
            descending: true,
            fetch: 100,
        };

        let files = (0..10)
            .map(|hour| file(hour * 100, hour * 100 + 99, 20_000))
            .collect::<Vec<_>>();

        let mut waves = Waves::new(order, files.clone(), None, true, true);
        let unchecked = waves.unchecked().unwrap().to_vec();
        assert_eq!(
            unchecked.len(),
            10,
            "pruned together, up to the first chunk"
        );

        let matches = counts(&unchecked, 30);
        waves.checked(unchecked, &matches);

        assert!(waves.unchecked().is_none());
        let first = waves.take().unwrap();
        assert_eq!(first.len(), 4);
        assert_eq!(first[0].time_range.end.0, 999);

        let mut waves = Waves::new(order, files, None, false, false);
        assert_eq!(waves.take().map(<[QueryFile]>::len), Some(3));
    }

    /// Files are pruned as the waves need them, newest first and a chunk at a time: the index
    /// reads stop once the counted rows fill a page, and a pruned file is never read.
    #[test]
    fn files_are_pruned_as_the_waves_reach_them() {
        let order = TimeOrder {
            descending: true,
            fetch: 100,
        };

        let files = (0..40)
            .map(|hour| file(hour * 100, hour * 100 + 99, 10_000))
            .collect::<Vec<_>>();

        let mut waves = Waves::new(order, files, None, true, true);

        // The first sixteen files: every other one holds 5 matching rows, too few for a page.
        let unchecked = waves.unchecked().unwrap().to_vec();
        assert_eq!(unchecked.len(), 16);
        assert_eq!(unchecked[0].time_range.end.0, 3_999);

        let kept = unchecked.into_iter().step_by(2).collect::<Vec<_>>();
        let matches = counts(&kept, 5);
        waves.checked(kept, &matches);

        // Eight kept files give 40 rows: the next thirty-two files are pruned together.
        let unchecked = waves.unchecked().unwrap().to_vec();
        assert_eq!(
            unchecked.len(),
            24,
            "the files left after the first sixteen"
        );

        let matches = counts(&unchecked, 5);
        waves.checked(unchecked, &matches);
        assert!(waves.unchecked().is_none());

        // 8 kept files of the first chunk and 12 more reach 100 rows.
        let first = waves.take().unwrap();
        assert_eq!(first.len(), 20);
        assert!(
            first[..8]
                .iter()
                .all(|file| file.time_range.end.0 % 200 == 199)
        );
    }

    /// When the files a full page could still need are all pruned, the page read stands.
    #[test]
    fn a_page_stands_when_pruning_leaves_nothing_to_add() {
        let order = TimeOrder {
            descending: true,
            fetch: 2,
        };

        let files = (0..20)
            .map(|hour| file(hour * 100, hour * 100 + 99, 20_000))
            .collect::<Vec<_>>();

        let mut waves = Waves::new(order, files, None, true, false);
        let unchecked = waves.unchecked().unwrap().to_vec();
        waves.checked(unchecked, &HashMap::new());
        assert_eq!(waves.take().map(<[QueryFile]>::len), Some(3));

        // The page reaches back into the next file, which the index prunes.
        assert!(waves.next(&page(&[1_950, 1_690])));
        assert!(
            waves.unchecked().is_none(),
            "the first chunk already checked it"
        );

        let mut waves = Waves::new(
            order,
            (0..20)
                .map(|hour| file(hour * 100, hour * 100 + 99, 20_000))
                .collect(),
            None,
            true,
            false,
        );

        let unchecked = waves.unchecked().unwrap().to_vec();
        let kept = unchecked
            .into_iter()
            .filter(|file| file.time_range.end.0 != 1_699)
            .collect::<Vec<_>>();
        waves.checked(kept, &HashMap::new());
        assert_eq!(waves.take().map(<[QueryFile]>::len), Some(3));

        // The next file (1 600–1 699) was pruned; the one after it ends before the boundary.
        assert!(!waves.next(&page(&[1_950, 1_690])));
        assert!(waves.take().is_none());
    }

    fn counts(files: &[QueryFile], rows: u64) -> HashMap<String, u64> {
        files
            .iter()
            .map(|file| (file.object_key.clone(), rows))
            .collect()
    }
}
