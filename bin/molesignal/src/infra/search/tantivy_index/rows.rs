// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 命中的文档在对应 Parquet 文件里的行号：查询只读这些行所在的页，不扫描整个文件。

use anyhow::{Result, anyhow};
use tantivy::{TantivyDocument, collector::DocSetCollector, schema::Value};

use super::{IndexHandle, ROW_FIELD, ROWS_VERSION};

impl IndexHandle {
    /// 每个字段都取 `sets` 里它的值之一（`field = 'value'`、`field IN (...)`）的文档在对应文件
    /// 里的行号，从小到大。命中超过 `max_rows` 个时为 `None`：读这么多行不如扫描整个文件。没有
    /// 条件、归档没有行号（格式 1），或某个字段在本归档里不是整值索引时也为 `None`。行号存在文档
    /// 库里，每个命中的文档读它所在的一个块。调用方须在 blocking 线程上调用，见
    /// [`Self::count_term`]。
    pub fn matching_rows(
        &self,
        sets: &[(String, Vec<String>)],
        max_rows: usize,
    ) -> Result<Option<Vec<u64>>> {
        if sets.is_empty() || self.format_version() < ROWS_VERSION {
            return Ok(None);
        }

        let (Some(query), Ok(row)) = (
            self.whole_value_query(sets),
            self.schema.get_field(ROW_FIELD),
        ) else {
            return Ok(None);
        };

        let searcher = self.searcher()?;

        let matches = query
            .count(&searcher)
            .map_err(|e| anyhow!("tantivy count: {e}"))?;

        if matches > max_rows {
            return Ok(None);
        }

        let addresses = searcher
            .search(query.as_ref(), &DocSetCollector)
            .map_err(|e| anyhow!("tantivy rows: {e}"))?;

        let mut rows = Vec::with_capacity(addresses.len());

        for address in addresses {
            let doc: TantivyDocument = searcher
                .doc(address)
                .map_err(|e| anyhow!("tantivy doc: {e}"))?;

            // 一个文档没有行号就不知道它在哪一行：不给出选择，免得漏掉它。
            let Some(value) = doc.get_first(row).and_then(|value| value.as_u64()) else {
                return Ok(None);
            };

            rows.push(value);
        }

        rows.sort_unstable();
        Ok(Some(rows))
    }
}
