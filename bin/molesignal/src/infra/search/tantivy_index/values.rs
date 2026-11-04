// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 整值字段的词典：每个值出现在多少行（字段分析里“最常见的值”），与有没有值含某段文字。

use std::collections::HashMap;

use anyhow::{Result, anyhow};
use tantivy::{Searcher, query::Query};

use super::{IndexHandle, OVERLONG_FIELD, ROWS_VERSION, term_query};

/// 按子串查整值字段时最多逐个看这么多个值：值更多的字段（ID 一类）不这样查，文件照常扫描。
const MAX_SCANNED_VALUES: usize = 65_536;

impl IndexHandle {
    /// 整值字段里可能有含 `literal` 的值：`LIKE '%literal%'`，`case_insensitive` 时是 `ILIKE`。
    /// `Some(false)` 时一定没有：词典里没有哪个值含它。字段在本归档里不是整值索引、有值长到没能
    /// 建索引、归档是格式 1，或某段的值超过 [`MAX_SCANNED_VALUES`] 个时为 `None`。
    ///
    /// 不计大小写时只把 ASCII 字母当作大小写：字面量里有非 ASCII 字符时为 `None`；有 `k`、`s` 时
    /// 也为 `None`，开尔文符号与长 s 不计大小写时等于它们。调用方须在 blocking 线程上调用。
    pub fn may_contain_value(
        &self,
        field: &str,
        literal: &str,
        case_insensitive: bool,
    ) -> Result<Option<bool>> {
        let folded = match case_insensitive {
            false => None,
            true if literal.is_ascii()
                && !literal
                    .bytes()
                    .any(|byte| matches!(byte.to_ascii_lowercase(), b'k' | b's')) =>
            {
                Some(literal.to_ascii_lowercase())
            }
            true => return Ok(None),
        };

        if literal.is_empty() || self.format_version() < ROWS_VERSION {
            return Ok(None);
        }

        let Some(target) = self.whole_value_field(field) else {
            return Ok(None);
        };

        let searcher = self.searcher()?;

        if self.has_overlong(&searcher, field)? {
            return Ok(None);
        }

        for segment in searcher.segment_readers() {
            let index = segment
                .inverted_index(target)
                .map_err(|e| anyhow!("tantivy terms: {e}"))?;

            if index.terms().num_terms() > MAX_SCANNED_VALUES {
                return Ok(None);
            }

            let mut stream = index
                .terms()
                .stream()
                .map_err(|e| anyhow!("tantivy terms: {e}"))?;

            while stream.advance() {
                let Ok(value) = std::str::from_utf8(stream.key()) else {
                    return Ok(None);
                };

                let found = match &folded {
                    None => value.contains(literal),
                    Some(folded) => value.to_ascii_lowercase().contains(folded.as_str()),
                };

                if found {
                    return Ok(Some(true));
                }
            }
        }

        Ok(Some(false))
    }

    /// 每个值出现在几行，即这个文件对 `SELECT NULLIF(field, '') ..., COUNT(*) ... GROUP BY 1`
    /// 的回答：空串与没有值的行不计。只读词典：每个值的文档频次就是它的行数，所以只回答没有
    /// 其它条件、整个文件都在时间窗里的情形。按条件或行时间挑行要逐个值读倒排表，在对象存储上
    /// 是成百上千次小读取，不如扫描文件。
    ///
    /// 字段在本归档里不是整值索引、某段的值超过 `max_values` 个、有值长到没能建索引，或归档
    /// 是格式 1（不知道有没有这样的值）时为 `None`。调用方须在 blocking 线程上调用。
    pub fn count_values(
        &self,
        field: &str,
        max_values: usize,
    ) -> Result<Option<HashMap<String, u64>>> {
        if self.format_version() < ROWS_VERSION {
            return Ok(None);
        }

        let Some(target) = self.whole_value_field(field) else {
            return Ok(None);
        };

        let searcher = self.searcher()?;

        if self.has_overlong(&searcher, field)? {
            return Ok(None);
        }

        let mut counts: HashMap<String, u64> = HashMap::new();

        for segment in searcher.segment_readers() {
            let index = segment
                .inverted_index(target)
                .map_err(|e| anyhow!("tantivy terms: {e}"))?;

            if index.terms().num_terms() > max_values {
                return Ok(None);
            }

            let mut stream = index
                .terms()
                .stream()
                .map_err(|e| anyhow!("tantivy terms: {e}"))?;

            while stream.advance() {
                let Ok(value) = std::str::from_utf8(stream.key()) else {
                    return Ok(None);
                };

                let rows = u64::from(stream.value().doc_freq);

                if !value.is_empty() && rows > 0 {
                    *counts.entry(value.to_owned()).or_default() += rows;
                }
            }
        }

        Ok(Some(counts))
    }

    /// 整值字段在各段词典里的值共有多少个（同一个值在几个段里各算一次）。字段在本归档里不是
    /// 整值索引时为 `None`。调用方须在 blocking 线程上调用。
    pub fn term_count(&self, field: &str) -> Result<Option<usize>> {
        let Some(target) = self.whole_value_field(field) else {
            return Ok(None);
        };

        let searcher = self.searcher()?;
        let mut count = 0;

        for segment in searcher.segment_readers() {
            let index = segment
                .inverted_index(target)
                .map_err(|e| anyhow!("tantivy terms: {e}"))?;

            count += index.terms().num_terms();
        }

        Ok(Some(count))
    }

    /// 依次交出整值字段在各段词典里的每个值。字段不是整值索引时什么也不交出。调用方须在
    /// blocking 线程上调用。
    pub fn visit_terms(&self, field: &str, visit: &mut dyn FnMut(&[u8])) -> Result<()> {
        let Some(target) = self.whole_value_field(field) else {
            return Ok(());
        };

        let searcher = self.searcher()?;

        for segment in searcher.segment_readers() {
            let index = segment
                .inverted_index(target)
                .map_err(|e| anyhow!("tantivy terms: {e}"))?;

            let mut stream = index
                .terms()
                .stream()
                .map_err(|e| anyhow!("tantivy terms: {e}"))?;

            while stream.advance() {
                visit(stream.key());
            }
        }

        Ok(())
    }

    /// 文件里有 `field` 的值长到没能建索引。
    fn has_overlong(&self, searcher: &Searcher, field: &str) -> Result<bool> {
        let Ok(overlong) = self.schema.get_field(OVERLONG_FIELD) else {
            return Ok(false);
        };

        let count = term_query(overlong, field)
            .count(searcher)
            .map_err(|e| anyhow!("tantivy count: {e}"))?;

        Ok(count > 0)
    }
}
