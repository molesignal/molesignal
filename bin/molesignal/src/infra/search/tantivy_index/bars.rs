// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 直方图按时间分柱的计数：命中的文档按 [`TIMESTAMP_FIELD`] 快字段落进柱子，错误另计。

use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
};

use anyhow::{Result, anyhow};
use tantivy::{
    DocId, Score, Searcher, SegmentReader,
    collector::{Collector, SegmentCollector},
    columnar::Column,
    query::BooleanQuery,
    schema::{Field, FieldType as TantivyFieldType},
};

use super::{IndexHandle, TIMESTAMP_FIELD, term_query};

/// 找错误值时最多扫描的词典大小。level 字段只有少数几个值；值多到这个数就不像 level，
/// 交给数据扫描。
const MAX_LEVEL_TERMS: usize = 4096;

impl IndexHandle {
    /// 把 `query.terms` 全部命中、`query.contains` 的子串都出现、行时间落在 `query.window` 内的
    /// 文档按 [`BarQuery`] 分进柱子，即该归档对应文件里这些行在直方图上的计数（每行一个文档）。
    ///
    /// 归档没有 [`TIMESTAMP_FIELD`] 快字段（加入它之前写的归档）、某个等值条件的字段在本归档里
    /// 不是整值索引、某个子串数不出来（见 [`Self::contains_query`]）、错误字段的值多得不像
    /// level，或柱号会溢出时返回 `None`。调用方须在 blocking 线程上调用，见
    /// [`Self::count_term`]。
    pub fn count_bars(&self, query: &BarQuery) -> Result<Option<BTreeMap<i64, BarTally>>> {
        let has_timestamps = self.schema.get_field(TIMESTAMP_FIELD).is_ok_and(|field| {
            let entry = self.schema.get_field_entry(field);
            entry.is_fast() && matches!(entry.field_type(), TantivyFieldType::I64(_))
        });

        let fits = query.width > 0
            && query.window.start.checked_sub(query.origin).is_some()
            && query.window.end.checked_sub(query.origin).is_some();

        let errors = match &query.errors {
            Some(name) => match self.whole_value_field(name) {
                Some(field) => Some(field),
                None => return Ok(None),
            },
            None => None,
        };

        if !has_timestamps || !fits {
            return Ok(None);
        }

        let searcher = self.searcher()?;

        let Some(matching) = self.matching_query(&searcher, &query.terms, &query.contains)? else {
            return Ok(None);
        };

        let collector = BarCollector {
            window: query.window.clone(),
            origin: query.origin,
            width: query.width,
        };

        let mut bars: BTreeMap<i64, BarTally> = searcher
            .search(matching.as_ref(), &collector)
            .map_err(|e| anyhow!("tantivy count bars: {e}"))?
            .into_iter()
            .map(|(bar, total)| (bar, BarTally { total, errors: 0 }))
            .collect();

        let Some(field) = errors else {
            return Ok(Some(bars));
        };

        let Some(values) = error_values(&searcher, field)? else {
            return Ok(None);
        };

        if values.is_empty() || bars.is_empty() {
            return Ok(Some(bars));
        }

        let errors = BooleanQuery::intersection(vec![
            matching,
            Box::new(BooleanQuery::union(
                values
                    .iter()
                    .map(|value| term_query(field, value))
                    .collect(),
            )),
        ]);

        let counted = searcher
            .search(&errors, &collector)
            .map_err(|e| anyhow!("tantivy count bars: {e}"))?;

        for (bar, errors) in counted {
            bars.entry(bar).or_default().errors += errors;
        }

        Ok(Some(bars))
    }
}

/// 直方图按时间分柱计数的问题，见 [`IndexHandle::count_bars`]。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BarQuery {
    /// 同时成立的 `field = 'value'`。
    pub terms: Vec<(String, String)>,
    /// 都要出现在值里的子串 `(field, term)`：小写 ASCII 字母与数字，见
    /// [`IndexHandle::contains_query`]。
    pub contains: Vec<(String, String)>,
    /// 计入的行时间：微秒，左闭右开。
    pub window: Range<i64>,
    /// 行时间为 `t` 的行落在第 `(t - origin) / width` 根柱子。
    pub origin: i64,
    pub width: i64,
    /// 值转成大写等于 `ERROR` 的行另计为错误时，所读的整值字段。
    pub errors: Option<String>,
}

/// 一根柱子里的行数，与其中的错误行数。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BarTally {
    pub total: u64,
    pub errors: u64,
}

/// 把命中的文档按行时间分进柱子。
struct BarCollector {
    window: Range<i64>,
    origin: i64,
    width: i64,
}

impl Collector for BarCollector {
    type Fruit = BTreeMap<i64, u64>;
    type Child = BarSegmentCollector;

    fn for_segment(&self, _: u32, segment: &SegmentReader) -> tantivy::Result<Self::Child> {
        Ok(BarSegmentCollector {
            timestamps: segment.fast_fields().column_opt(TIMESTAMP_FIELD)?,
            window: self.window.clone(),
            origin: self.origin,
            width: self.width,
            bars: BTreeMap::new(),
        })
    }

    fn requires_scoring(&self) -> bool {
        false
    }

    fn merge_fruits(&self, fruits: Vec<Self::Fruit>) -> tantivy::Result<Self::Fruit> {
        let mut merged = BTreeMap::new();

        for (bar, count) in fruits.into_iter().flatten() {
            *merged.entry(bar).or_default() += count;
        }

        Ok(merged)
    }
}

struct BarSegmentCollector {
    /// 段里没有一个文档带行时间时没有这一列；这些文档都不计入，正如它们在 SQL 里不满足
    /// 时间条件。
    timestamps: Option<Column<i64>>,
    window: Range<i64>,
    origin: i64,
    width: i64,
    bars: BTreeMap<i64, u64>,
}

impl SegmentCollector for BarSegmentCollector {
    type Fruit = BTreeMap<i64, u64>;

    fn collect(&mut self, doc: DocId, _: Score) {
        let Some(time) = self
            .timestamps
            .as_ref()
            .and_then(|column| column.first(doc))
        else {
            return;
        };

        // 窗内的行时间减去 origin 不溢出：`count_bars` 检查过窗的两端。
        if self.window.contains(&time) {
            *self
                .bars
                .entry((time - self.origin) / self.width)
                .or_default() += 1;
        }
    }

    fn harvest(self) -> Self::Fruit {
        self.bars
    }
}

/// `field` 各段词典里转成大写等于 `ERROR` 的值，即 `UPPER(field) = 'ERROR'` 成立的值。
/// 某段词典超过 [`MAX_LEVEL_TERMS`] 个值时为 `None`。
fn error_values(searcher: &Searcher, field: Field) -> Result<Option<BTreeSet<String>>> {
    let mut values = BTreeSet::new();

    for segment in searcher.segment_readers() {
        let index = segment
            .inverted_index(field)
            .map_err(|e| anyhow!("tantivy terms: {e}"))?;

        if index.terms().num_terms() > MAX_LEVEL_TERMS {
            return Ok(None);
        }

        let mut terms = index
            .terms()
            .stream()
            .map_err(|e| anyhow!("tantivy terms: {e}"))?;

        while terms.advance() {
            if let Ok(value) = std::str::from_utf8(terms.key())
                && value.to_uppercase() == "ERROR"
            {
                values.insert(value.to_owned());
            }
        }
    }

    Ok(Some(values))
}
