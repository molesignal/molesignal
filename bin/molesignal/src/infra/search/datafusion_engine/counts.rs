// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 语句里不读数据就能回答的部分：只数行数的 `count(*)`、explorer 直方图按时间分柱的计数，与
//! 字段分析里一列最常见的值和一列的汇总。
//!
//! `count(*)`：整体落在时间窗内的文件用 Catalog 记录的行数或索引的命中数回答。直方图：用
//! Tantivy 归档里每行的时间分柱，文件不必整体落在窗内。两者的条件是等值（整值索引）与子串
//! （`ILIKE '%词%'`，分词索引）的合取。最常见的值与汇总：数整值索引词典里每个值的行，
//! DataFusion 对其余文件执行数出每个值的语句，合起来再排序取前几个，或数出行数、有值的行数与
//! 不同值的个数。答出的文件不再交给 DataFusion 扫描，DataFusion 只数其余文件与缓冲，最后两份
//! 计数相加。

use std::collections::{BTreeMap, HashMap};

use arrow::{array::RecordBatch, datatypes::SchemaRef};

use super::DataFusionEngine;
use crate::{
    domain::{
        query::QueryRequest,
        storage::QueryFile,
        stream::{FieldType, StreamDefinition, StreamIndexType},
    },
    infra::{
        query::parser::{KEY_VALUE_SUFFIX, bar_counts, field_summary, plain_count, top_values},
        search::tantivy_index::{BarQuery, BarTally},
    },
    shared::{
        Result,
        time::{TimeRange, TimestampMicros},
    },
};

mod merge;

use merge::{add_bars, add_rows, add_summary, add_values};

/// 能原样成为一个整值索引 term 的字面量长度上限。远小于 Tantivy 丢弃 term 的长度，命中这样
/// 字面量的值一定被索引过。
const MAX_INDEXED_TERM_BYTES: usize = 1024;

/// 从索引数的柱子数上限。explorer 一次最多画 512 根；分组更多的语句交给 DataFusion。
const MAX_BARS: i64 = 4096;

pub(super) enum Counted {
    /// `SELECT count(*)`：等值与子串过滤，与已数出的行数。
    Rows {
        filters: Vec<(String, String)>,
        contains: Vec<(String, String)>,
        rows: u64,
    },
    /// explorer 直方图：要数的柱子，与已数出的计数。
    Bars {
        query: BarQuery,
        bars: BTreeMap<i64, BarTally>,
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
            let whole_table = count.filters.is_empty() && count.contains.is_empty();

            let countable = whole_table
                || (has_pruner
                    && definition.is_some_and(|definition| {
                        exact(&count.filters, definition) && contained(&count.contains, definition)
                    }));

            return countable.then_some(Self::Rows {
                filters: count.filters,
                contains: count.contains,
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

        let bars = bar_counts(sql, table)?;
        let definition = definition?;

        // 引擎在扫描层把行限定在请求的时间窗内，语句的时间条件再收窄一次。
        let window = req.time_range.start.0.max(bars.from)..req.time_range.end.0.min(bars.to);

        let countable = has_pruner
            && req.limit.is_none()
            && exact(&bars.filters, definition)
            && contained(&bars.contains, definition)
            && bars
                .errors
                .as_deref()
                .is_none_or(|level| text_exact(level, definition))
            && fits(&window, bars.origin, bars.width);

        countable.then(|| Self::Bars {
            query: BarQuery {
                terms: bars.filters,
                contains: bars.contains,
                window,
                origin: bars.origin,
                width: bars.width,
                errors: bars.errors,
            },
            bars: BTreeMap::new(),
        })
    }

    /// 数出 `files` 里能这样回答的文件，返回其余仍要扫描的文件。`window` 是请求的时间窗；
    /// `index_keys` 是文件到其 Tantivy Artifact 的对应。
    pub(super) async fn take(
        &mut self,
        engine: &DataFusionEngine,
        files: Vec<QueryFile>,
        window: TimeRange,
        index_keys: &HashMap<String, String>,
    ) -> Vec<QueryFile> {
        match self {
            Self::Rows {
                filters,
                contains,
                rows,
            } => {
                let (covered, mut rest): (Vec<QueryFile>, Vec<QueryFile>) = files
                    .into_iter()
                    .partition(|file| within(window, file.time_range));

                if filters.is_empty() && contains.is_empty() {
                    *rows += covered.iter().map(|file| file.rows).sum::<u64>();
                    return rest;
                }

                let Some(pruner) = &engine.tantivy_pruner else {
                    rest.extend(covered);
                    return rest;
                };

                let counts = pruner
                    .count_exact(&covered, filters, contains, index_keys)
                    .await;

                for (file, count) in covered.into_iter().zip(counts) {
                    match count {
                        Some(count) => *rows += count,
                        None => rest.push(file),
                    }
                }

                rest
            }
            Self::Bars { query, bars } => {
                let Some(pruner) = &engine.tantivy_pruner else {
                    return files;
                };

                let counts = pruner.count_bars(&files, query, index_keys).await;
                let mut rest = Vec::new();

                for (file, counted) in files.into_iter().zip(counts) {
                    let Some(counted) = counted else {
                        rest.push(file);
                        continue;
                    };

                    for (bar, tally) in counted {
                        let into = bars.entry(bar).or_default();
                        into.total += tally.total;
                        into.errors += tally.errors;
                    }
                }

                rest
            }
            Self::Values {
                column,
                window,
                counts,
                ..
            } => {
                let (_, rest) =
                    count_values(engine, files, *window, column, index_keys, counts).await;

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
                let (taken, rest) =
                    count_values(engine, files, *window, column, index_keys, counts).await;

                *rows += taken.iter().map(|file| file.rows).sum::<u64>();
                *counted |= !taken.is_empty();
                rest
            }
        }
    }

    /// DataFusion 对其余文件执行的语句：数最常见的值、汇总一列时是数出每个值的语句，其余照旧。
    pub(super) fn statement<'a>(&'a self, sql: &'a str) -> &'a str {
        match self {
            Self::Values { grouped, .. }
            | Self::Summary {
                grouped,
                counted: true,
                ..
            } => grouped,
            _ => sql,
        }
    }

    /// 把已数出的计数加进 DataFusion 对其余文件与缓冲的结果。`schema` 是结果的 schema：
    /// 文件全部由索引数出时 DataFusion 可能一行也不返回。
    pub(super) fn add_to(&self, batches: &mut Vec<RecordBatch>, schema: SchemaRef) -> Result<()> {
        match self {
            Self::Rows { rows, .. } if *rows > 0 => add_rows(batches, *rows),
            Self::Bars { query, bars } if !bars.is_empty() => {
                add_bars(batches, schema, bars, query.errors.is_some())
            }
            Self::Values { counts, limit, .. } => add_values(batches, schema, counts, *limit),
            Self::Summary {
                names,
                rows,
                counts,
                counted: true,
                ..
            } => add_summary(batches, names, *rows, counts),
            _ => Ok(()),
        }
    }
}

/// 整个落在 `window` 里的文件用整值索引的词典数出 `column` 每个值的行数，加进 `counts`。返回
/// 数出的文件，与其余仍要扫描的文件。
async fn count_values(
    engine: &DataFusionEngine,
    files: Vec<QueryFile>,
    window: TimeRange,
    column: &str,
    index_keys: &HashMap<String, String>,
    counts: &mut HashMap<String, u64>,
) -> (Vec<QueryFile>, Vec<QueryFile>) {
    let Some(pruner) = &engine.tantivy_pruner else {
        return (Vec::new(), files);
    };

    let (covered, mut rest): (Vec<QueryFile>, Vec<QueryFile>) = files
        .into_iter()
        .partition(|file| within(window, file.time_range));

    let counted = pruner.count_values(&covered, column, index_keys).await;
    let mut taken = Vec::new();

    for (file, counted) in covered.into_iter().zip(counted) {
        let Some(counted) = counted else {
            rest.push(file);
            continue;
        };

        for (value, count) in counted {
            *counts.entry(value).or_default() += count;
        }

        taken.push(file);
    }

    (taken, rest)
}

/// 请求的时间窗，再按语句的时间条件收窄。
fn narrowed(req: &QueryRequest, from: Option<i64>, to: Option<i64>) -> TimeRange {
    TimeRange::new(
        TimestampMicros(req.time_range.start.0.max(from.unwrap_or(i64::MIN))),
        TimestampMicros(req.time_range.end.0.min(to.unwrap_or(i64::MAX))),
    )
}

/// 等值条件都落在整值索引字段上，字面量能原样成为一个 term；`json_text(列, 'key')` 的条件落在
/// 键值索引的 JSON 字段上。
pub(super) fn exact(filters: &[(String, String)], definition: &StreamDefinition) -> bool {
    filters.iter().all(|(column, value)| {
        !value.is_empty()
            && value.len() <= MAX_INDEXED_TERM_BYTES
            && definition.schema.fields.iter().any(|field| {
                !field.encrypted
                    && match column.strip_suffix(KEY_VALUE_SUFFIX) {
                        // `json_text(列, 'key')` 的条件：键值索引里的一个词。
                        Some(json) => {
                            field.name == json
                                && field.data_type == FieldType::Json
                                && field.effective_index_type() == StreamIndexType::KeyValue
                        }
                        None => {
                            field.name == *column
                                && matches!(field.data_type, FieldType::Utf8 | FieldType::Json)
                                && field.effective_index_type() == StreamIndexType::Exact
                        }
                    }
            })
    })
}

/// 子串条件都落在全文索引的文本字段上，子串只由小写 ASCII 字母与数字组成：这样的子串整个落在
/// 一个词里，有它的行能从词典数出来，见 [`IndexHandle::contains_query`]。
///
/// [`IndexHandle::contains_query`]: crate::infra::search::tantivy_index::IndexHandle::contains_query
fn contained(contains: &[(String, String)], definition: &StreamDefinition) -> bool {
    contains.iter().all(|(column, term)| {
        !term.is_empty()
            && term.len() <= MAX_INDEXED_TERM_BYTES
            && term
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            && definition.schema.fields.iter().any(|field| {
                field.name == *column
                    && !field.encrypted
                    && field.data_type == FieldType::Utf8
                    && field.effective_index_type() == StreamIndexType::FullText
            })
    })
}

/// 按整值建索引的文本字段：`CAST(列 AS VARCHAR)`、`NULLIF(列, '')` 读到的就是索引里的值。
fn text_exact(column: &str, definition: &StreamDefinition) -> bool {
    definition.schema.fields.iter().any(|field| {
        field.name == column
            && !field.encrypted
            && field.data_type == FieldType::Utf8
            && field.effective_index_type() == StreamIndexType::Exact
    })
}

/// 窗内的柱子不超过 [`MAX_BARS`] 根。柱号随时间单调，首尾两根之间的就是全部。
fn fits(window: &std::ops::Range<i64>, origin: i64, width: i64) -> bool {
    if window.is_empty() {
        return true;
    }

    let first = (window.start - origin) / width;
    let last = (window.end - 1 - origin) / width;

    last.checked_sub(first)
        .and_then(|span| span.checked_add(1))
        .is_some_and(|bars| bars <= MAX_BARS)
}

/// 文件的全部行都落在查询时间窗 `[start, end)` 内。
fn within(window: TimeRange, file: TimeRange) -> bool {
    window.start.0 <= file.start.0 && file.end.0 < window.end.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_fits_when_its_bars_can_be_counted() {
        assert!(fits(&(0..0), 0, 1));
        assert!(fits(&(0..4096), 0, 1));
        assert!(!fits(&(0..4097), 0, 1));
        assert!(fits(&(-5..5), 10, 3));
        assert!(!fits(&(i64::MIN / 2..i64::MAX / 2), 0, 1));
    }
}
