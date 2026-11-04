// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 只读等值过滤命中的行：`trace_id = '…'`、`trace_id IN (…)` 这类整值字段上的条件，从 Tantivy
//! 归档拿到命中的行在文件里的行号，扫描时只读这些行所在的页。条件照常在读出的行上执行，所以选中
//! 的行只要包含所有可能的结果即可；等值条件的命中正是这样的超集。

use std::collections::HashMap;

use datafusion::datasource::physical_plan::parquet::ParquetRowSelection;
use parquet::arrow::arrow_reader::{RowSelection, RowSelector};

use super::{DataFusionEngine, counts::exact};
use crate::{
    domain::{storage::QueryFile, stream::StreamDefinition},
    infra::query::parser::EqualitySet,
};

/// 顶层合取链上落在整值索引字段上的等值条件（`=`、`IN`）：字段与它可取的值，每个字面量都能原样
/// 成为一个 term。每一行都满足它们，命中的行就是要读的行的超集。
pub(super) fn exact_terms(
    equalities: &[EqualitySet],
    definition: Option<&StreamDefinition>,
) -> Vec<(String, Vec<String>)> {
    let Some(definition) = definition else {
        return Vec::new();
    };

    equalities
        .iter()
        .filter(|set| {
            set.values
                .iter()
                .all(|value| exact(&[(set.column.clone(), value.clone())], definition))
        })
        .map(|set| (set.column.clone(), set.values.clone()))
        .collect()
}

/// 每个能从索引定位命中行的文件，只读哪些行，按 object key。
pub(super) async fn row_selections(
    engine: &DataFusionEngine,
    files: &[QueryFile],
    terms: &[(String, Vec<String>)],
    index_keys: &HashMap<String, String>,
) -> HashMap<String, ParquetRowSelection> {
    let Some(pruner) = &engine.tantivy_pruner else {
        return HashMap::new();
    };

    if terms.is_empty() {
        return HashMap::new();
    }

    let rows = pruner.matching_rows(files, terms, index_keys).await;

    files
        .iter()
        .zip(rows)
        .filter_map(|(file, rows)| {
            let selection = selection(&rows?, file.rows)?;
            Some((file.object_key.clone(), ParquetRowSelection::new(selection)))
        })
        .collect()
}

/// `files` 要读的行数：有选择的文件只算选中的行。
pub(super) fn rows_to_read(
    files: &[QueryFile],
    selections: &HashMap<String, ParquetRowSelection>,
) -> u64 {
    files
        .iter()
        .map(|file| match selections.get(&file.object_key) {
            Some(selection) => selection.selection().row_count() as u64,
            None => file.rows,
        })
        .sum()
}

/// 有 `file_rows` 行的文件里只选 `rows` 这些行（从小到大）。行号超出文件时为 `None`。
fn selection(rows: &[u64], file_rows: u64) -> Option<RowSelection> {
    if rows.last().is_some_and(|last| *last >= file_rows) {
        return None;
    }

    let mut selectors = Vec::new();
    let mut next = 0_u64;

    for row in rows {
        if *row < next {
            continue;
        }

        if *row > next {
            selectors.push(RowSelector::skip(usize::try_from(row - next).ok()?));
        }

        selectors.push(RowSelector::select(1));
        next = row + 1;
    }

    if file_rows > next {
        selectors.push(RowSelector::skip(usize::try_from(file_rows - next).ok()?));
    }

    Some(RowSelection::from(selectors))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows_of(selection: &RowSelection) -> Vec<(bool, usize)> {
        selection
            .iter()
            .map(|selector| (selector.skip, selector.row_count))
            .collect()
    }

    #[test]
    fn a_selection_covers_every_row_and_picks_the_matches() {
        let selection = selection(&[2, 3, 7], 10).unwrap();

        assert_eq!(
            rows_of(&selection),
            vec![(true, 2), (false, 2), (true, 3), (false, 1), (true, 2)]
        );
        assert_eq!(selection.row_count(), 3);
    }

    #[test]
    fn rows_past_the_end_give_no_selection() {
        assert!(selection(&[10], 10).is_none());
        assert_eq!(rows_of(&selection(&[], 4).unwrap()), vec![(true, 4)]);
    }
}
