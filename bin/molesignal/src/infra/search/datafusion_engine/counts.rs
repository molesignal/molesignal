// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 语句里不读数据就能回答的部分：只数行数的 `count(*)`、explorer 直方图按时间分柱的计数、
//! 字段分析里一列最常见的值和一列的汇总，以及按一列的值数行的 `GROUP BY`。
//!
//! `count(*)`：时间窗是请求的时间窗再按语句里行时间的上下界收窄；整体落在窗内的文件用 Catalog
//! 记录的行数或索引的命中数回答，与窗不相交的文件不再扫描。直方图：用
//! Tantivy 归档里每行的时间分柱，文件不必整体落在窗内。两者的条件是等值（整值索引）与子串
//! （`ILIKE '%词%'`，分词索引）的合取。最常见的值、汇总与 `GROUP BY`：数整值索引词典里每个值
//! 的行，DataFusion 对其余文件执行数出每个值的语句，合起来再排序取前几个，或数出行数、有值的
//! 行数与不同值的个数。答出的文件不再交给 DataFusion 扫描，DataFusion 只数其余文件与缓冲，最后
//! 两份计数相加。
//!
//! A file with minute counts (see [`minutes`]) answers first a count, a count by one column and
//! bars of whole minutes, with equality conditions on the fields its counts are kept by, also
//! where the window cuts it between minutes.

use std::collections::{BTreeMap, HashMap};

use arrow::{array::RecordBatch, datatypes::SchemaRef};
use domain::{query::QueryRequest, storage::QueryFile, stream::StreamDefinition};
use kernel::{Result, time::TimeRange};
use query_language::{
    BarKey, GroupedCounts, NumberRange, bar_counts, field_summary, grouped_counts, plain_count,
    top_values,
};

use super::DataFusionEngine;
use crate::infra::{
    search::tantivy_index::{BarQuery, BarTally},
    storage::minute_counts::MinuteCountsKeys,
};

mod dictionary;
mod fields;
mod groups;
mod merge;
mod minutes;
mod window;

use dictionary::count_values;
pub(super) use fields::exact;
use fields::{contained, ranged, text_exact};
use groups::{add_groups, missing_rows};
use merge::{add_bars, add_rows, add_summary, add_values};
use window::{fits, narrowed, overlaps, within};

pub(super) enum Counted {
    /// `SELECT count(*)`：等值与子串过滤、数值范围、请求按语句的时间条件收窄后的时间窗，与已数出的
    /// 行数。
    Rows {
        filters: Vec<(String, String)>,
        contains: Vec<(String, String)>,
        ranges: Vec<(String, NumberRange)>,
        window: TimeRange,
        answers: Answers,
        rows: u64,
    },
    /// explorer 直方图：要数的柱子，与已数出的计数。
    Bars {
        query: BarQuery,
        /// What the statement returns for a bar.
        key: BarKey,
        /// The request's row limit, kept to the first bars once the counts are added up.
        limit: Option<usize>,
        answers: Answers,
        bars: BTreeMap<i64, BarTally>,
        /// Some file was counted. Otherwise DataFusion's answer stands as it is.
        counted: bool,
    },
    /// 一列最常见的值：要数的列与时间窗、已数出的每个值的行数，与交给 DataFusion 的不排序语句。
    Values {
        column: String,
        window: TimeRange,
        limit: usize,
        grouped: String,
        counts: HashMap<String, u64>,
    },
    /// 一列的汇总：要数的列与时间窗、结果的列名、已数出的行数与每个值的行数，与交给 DataFusion
    /// 的数出每个值的语句。没有文件由索引数出时 DataFusion 照原样执行语句（`counted` 为假）。
    Summary {
        column: String,
        window: TimeRange,
        names: [String; 3],
        grouped: String,
        rows: u64,
        counts: HashMap<String, u64>,
        counted: bool,
    },
    /// 按一列的值数行的 `GROUP BY`：语句、时间窗与请求的行数上限，已数出的每个值的行数与没有值
    /// 的行数。有文件数出时（`counted`），DataFusion 对其余文件执行不排序、不截断的语句，合起来
    /// 再照语句排序、截断；没有时照原样执行语句。
    Groups {
        groups: GroupedCounts,
        window: TimeRange,
        limit: Option<usize>,
        answers: Answers,
        counts: HashMap<String, u64>,
        nulls: u64,
        counted: bool,
    },
}

/// What can answer a statement's conditions, file by file: the index archives, and the minute
/// counts of files whose counts are kept by the columns the statement names.
#[derive(Clone, Copy)]
pub(super) struct Answers {
    index: bool,
    minutes: bool,
}

impl Answers {
    fn any(self) -> bool {
        self.index || self.minutes
    }
}

impl Counted {
    /// `sql` 能这样回答时返回要数的内容。`definition` 是 `table` 的 stream 定义。
    pub(super) fn plan(
        engine: &DataFusionEngine,
        req: &QueryRequest,
        sql: &str,
        table: &str,
        definition: Option<&StreamDefinition>,
    ) -> Option<Self> {
        let has_pruner = engine.tantivy_pruner.is_some();

        if let Some(count) = plain_count(sql, table) {
            let answers = Answers {
                index: has_pruner
                    && definition.is_some_and(|definition| {
                        exact(&count.filters, definition)
                            && contained(&count.contains, definition)
                            && ranged(&count.ranges, definition)
                    }),
                // Without conditions too: the Catalog's row counts, and the minute counts of
                // files the window cuts.
                minutes: count.contains.is_empty()
                    && count.ranges.is_empty()
                    && minutes::answerable(&count.filters, definition),
            };

            return answers.any().then(|| Self::Rows {
                window: narrowed(req, count.from, count.to),
                filters: count.filters,
                contains: count.contains,
                ranges: count.ranges,
                answers,
                rows: 0,
            });
        }

        // 只读词典能回答的才数：没有其它条件，见 [`IndexHandle::count_values`]。
        if let Some(values) = top_values(sql, table) {
            let definition = definition?;
            let window = narrowed(req, values.from, values.to);

            let countable = has_pruner
                && req.limit.is_none()
                && values.filters.is_empty()
                && values.contains.is_empty()
                && text_exact(&values.column, definition);

            return countable.then(|| Self::Values {
                column: values.column,
                window,
                limit: values.limit,
                grouped: values.grouped,
                counts: HashMap::new(),
            });
        }

        if let Some(summary) = field_summary(sql, table) {
            let definition = definition?;
            let window = narrowed(req, summary.from, summary.to);

            let countable = has_pruner
                && req.limit.is_none()
                && summary.filters.is_empty()
                && summary.contains.is_empty()
                && text_exact(&summary.column, definition);

            return countable.then(|| Self::Summary {
                column: summary.column,
                window,
                names: summary.names,
                grouped: summary.grouped,
                rows: 0,
                counts: HashMap::new(),
                counted: false,
            });
        }

        if let Some(groups) = grouped_counts(sql, table) {
            let definition = definition?;
            let window = narrowed(req, groups.from, groups.to);

            // The dictionary counts a column's values only without conditions.
            let answers = Answers {
                index: has_pruner
                    && groups.filters.is_empty()
                    && text_exact(&groups.column, definition),
                minutes: minutes::text_field(&groups.column, definition)
                    && minutes::answerable(&groups.filters, Some(definition)),
            };

            return answers.any().then(|| Self::Groups {
                window,
                limit: req.limit,
                groups,
                answers,
                counts: HashMap::new(),
                nulls: 0,
                counted: false,
            });
        }

        let bars = bar_counts(sql, table)?;
        let definition = definition?;

        // 引擎在扫描层把行限定在请求的时间窗内，语句的时间条件再收窄一次。
        let window = req.time_range.start.0.max(bars.from)..req.time_range.end.0.min(bars.to);

        let answers = Answers {
            index: has_pruner
                && exact(&bars.filters, definition)
                && contained(&bars.contains, definition)
                && bars
                    .errors
                    .as_deref()
                    .is_none_or(|level| text_exact(level, definition)),
            minutes: bars.contains.is_empty()
                && minutes::answerable(&bars.filters, Some(definition))
                && bars
                    .errors
                    .as_deref()
                    .is_none_or(|level| minutes::text_field(level, definition))
                && minutes::whole_minute_bars(bars.origin, bars.width, window.start),
        };

        let countable = answers.any() && fits(&window, bars.origin, bars.width);

        countable.then(|| Self::Bars {
            query: BarQuery {
                terms: bars.filters,
                contains: bars.contains,
                window,
                origin: bars.origin,
                width: bars.width,
                errors: bars.errors,
            },
            key: bars.key,
            limit: req.limit,
            answers,
            bars: BTreeMap::new(),
            counted: false,
        })
    }

    /// 数出 `files` 里能这样回答的文件，返回其余仍要扫描的文件。`index_keys` 是文件到其
    /// Tantivy Artifact 的对应，`minute_keys` 是段到其分钟计数的对应。
    pub(super) async fn take(
        &mut self,
        engine: &DataFusionEngine,
        files: Vec<QueryFile>,
        index_keys: &HashMap<String, String>,
        minute_keys: &HashMap<String, MinuteCountsKeys>,
    ) -> Vec<QueryFile> {
        match self {
            Self::Rows {
                filters,
                contains,
                ranges,
                window,
                answers,
                rows,
            } => {
                let window = *window;
                let unconditioned = filters.is_empty() && contains.is_empty() && ranges.is_empty();

                // 与时间窗不相交的文件没有要数的行，不必再交给 DataFusion；没有条件时，整体落在窗内
                // 的文件用 Catalog 记录的行数。
                let (covered, mut files): (Vec<QueryFile>, Vec<QueryFile>) = files
                    .into_iter()
                    .filter(|file| overlaps(window, file.time_range))
                    .partition(|file| unconditioned && within(window, file.time_range));

                *rows += covered.iter().map(|file| file.rows).sum::<u64>();

                if answers.minutes {
                    files = minutes::count_rows(engine, files, minute_keys, window, filters, rows)
                        .await;
                }

                let Some(pruner) = engine.tantivy_pruner.as_ref().filter(|_| answers.index) else {
                    return files;
                };

                let (covered, mut rest): (Vec<QueryFile>, Vec<QueryFile>) = files
                    .into_iter()
                    .partition(|file| within(window, file.time_range));

                let counts = pruner
                    .count_exact(&covered, filters, contains, ranges, index_keys)
                    .await;

                for (file, count) in covered.into_iter().zip(counts) {
                    match count {
                        Some(count) => *rows += count,
                        None => rest.push(file),
                    }
                }

                rest
            }
            Self::Bars {
                query,
                key,
                answers,
                bars,
                counted,
                ..
            } => {
                let mut files = files;
                let candidates = files.len();

                if answers.minutes {
                    files =
                        minutes::count_bars(engine, files, minute_keys, query, *key, bars).await;
                }

                let Some(pruner) = engine.tantivy_pruner.as_ref().filter(|_| answers.index) else {
                    *counted |= files.len() < candidates;
                    return files;
                };

                let counts = pruner.count_bars(&files, query, index_keys).await;
                let mut rest = Vec::new();

                for (file, counted) in files.into_iter().zip(counts) {
                    let Some(counted) = counted else {
                        rest.push(file);
                        continue;
                    };

                    for (index, tally) in counted {
                        let bar = match key {
                            BarKey::Index => index,
                            BarKey::Start => index * query.width,
                        };

                        let into = bars.entry(bar).or_default();
                        into.total += tally.total;
                        into.errors += tally.errors;
                    }
                }

                *counted |= rest.len() < candidates;
                rest
            }
            Self::Values {
                column,
                window,
                counts,
                ..
            } => {
                let (_, rest) = count_values(
                    engine,
                    files,
                    *window,
                    column,
                    index_keys,
                    counts,
                    |_, _| true,
                )
                .await;

                rest
            }
            Self::Summary {
                column,
                window,
                rows,
                counts,
                counted,
                ..
            } => {
                let (taken, rest) = count_values(
                    engine,
                    files,
                    *window,
                    column,
                    index_keys,
                    counts,
                    |_, _| true,
                )
                .await;

                *rows += taken.iter().map(|file| file.rows).sum::<u64>();
                *counted |= !taken.is_empty();
                rest
            }
            Self::Groups {
                groups,
                window,
                answers,
                counts,
                nulls,
                counted,
                ..
            } => {
                let window = *window;

                // 与时间窗不相交的文件没有要数的行；没有值的行也是一组，每一行都要有着落。
                let mut files: Vec<QueryFile> = files
                    .into_iter()
                    .filter(|file| overlaps(window, file.time_range))
                    .collect();

                let candidates = files.len();

                if answers.minutes {
                    files = minutes::count_groups(
                        engine,
                        files,
                        minute_keys,
                        window,
                        &groups.column,
                        &groups.filters,
                        counts,
                        nulls,
                    )
                    .await;
                }

                if !answers.index {
                    *counted |= files.len() < candidates;
                    return files;
                }

                let (_, rest) = count_values(
                    engine,
                    files,
                    window,
                    &groups.column,
                    index_keys,
                    counts,
                    |file, values| {
                        missing_rows(file, &groups.column, values)
                            .map(|missing| *nulls += missing)
                            .is_some()
                    },
                )
                .await;

                *counted |= rest.len() < candidates;
                rest
            }
        }
    }

    /// DataFusion 对其余文件执行的语句：数最常见的值、汇总一列与 `GROUP BY` 时是数出每个值的
    /// 语句，其余照旧。
    pub(super) fn statement<'a>(&'a self, sql: &'a str) -> &'a str {
        match self {
            Self::Values { grouped, .. }
            | Self::Summary {
                grouped,
                counted: true,
                ..
            } => grouped,
            Self::Groups {
                groups,
                counted: true,
                ..
            } => &groups.grouped,
            _ => sql,
        }
    }

    /// DataFusion 对其余文件与缓冲的结果最多取几行：`GROUP BY` 要全部的组、直方图要全部的柱子，
    /// 与数出的合起来排序之后才截断；其余照请求的上限。
    pub(super) fn result_limit(&self, limit: Option<usize>) -> Option<usize> {
        match self {
            Self::Groups { counted: true, .. } | Self::Bars { counted: true, .. } => None,
            _ => limit,
        }
    }

    /// 把已数出的计数加进 DataFusion 对其余文件与缓冲的结果。`schema` 是结果的 schema：
    /// 文件全部由索引数出时 DataFusion 可能一行也不返回。
    pub(super) fn add_to(&self, batches: &mut Vec<RecordBatch>, schema: SchemaRef) -> Result<()> {
        match self {
            Self::Rows { rows, .. } if *rows > 0 => add_rows(batches, *rows),
            Self::Bars {
                query,
                bars,
                limit,
                counted: true,
                ..
            } => add_bars(batches, schema, bars, query.errors.is_some(), *limit),
            Self::Values { counts, limit, .. } => add_values(batches, schema, counts, *limit),
            Self::Summary {
                names,
                rows,
                counts,
                counted: true,
                ..
            } => add_summary(batches, names, *rows, counts),
            Self::Groups {
                groups,
                limit,
                counts,
                nulls,
                counted: true,
                ..
            } => add_groups(batches, schema, groups, *limit, counts, *nulls),
            _ => Ok(()),
        }
    }
}
