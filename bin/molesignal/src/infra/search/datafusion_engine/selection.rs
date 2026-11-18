// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 扫描时每个文件只读哪些行：
//!
//! - 等值过滤命中的行：`trace_id = '…'`、`trace_id IN (…)` 这类整值字段上的条件，从 Tantivy
//!   归档拿到命中的行在文件里的行号，扫描时只读这些行所在的页。
//! - `LIKE` 可能成立的块：ngram 字段上的 `LIKE`、`ILIKE`，字面量的五字节片段（较早写入的段是
//!   三元组）不全在一块的块过滤器里时它在这块里不成立，条件树因此不可能成立的块不读，见
//!   [`GramBlocks`]。
//!
//! 条件照常在读出的行上执行，所以选中的行只要包含所有可能的结果即可；两种选择都是这样的超集，
//! 一个文件两种都有时只读两者都选中的行。
//!
//! [`GramBlocks`]: crate::infra::storage::filters::GramBlocks

use std::collections::HashMap;

use datafusion::datasource::physical_plan::parquet::ParquetRowSelection;
use domain::{storage::QueryFile, stream::StreamDefinition};
use futures::{StreamExt, stream};
use parquet::arrow::arrow_reader::{RowSelection, RowSelector};
use query_language::{EqualitySet, Refuted, atom_bit};

use super::{DataFusionEngine, blooms::FilterTerms, counts::exact, tables::TableInput};
use crate::infra::{
    query::{catalog_source::CatalogQuerySource, refutations::Refutations},
    storage::filters::Filters,
};

/// 同时读取的过滤器 Artifact 数。
const CONCURRENT_READS: usize = 16;

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

/// `files`（`table` 的一部分文件）里每个文件要读的行，按 object key：`exact_terms` 在索引里
/// 命中的行，与 `filter_terms` 的 `LIKE` 可能成立的块；`refutations` 是前面各步已证明不成立的
/// 原子。两样都没有的文件不在结果里，整个读。
pub(super) async fn selections(
    engine: &DataFusionEngine,
    files: &[QueryFile],
    table: &TableInput,
    exact_terms: &[(String, Vec<String>)],
    filter_terms: &FilterTerms,
    refutations: &Refutations,
) -> HashMap<String, ParquetRowSelection> {
    let mut selections = row_selections(engine, files, exact_terms, table).await;

    let blocks =
        block_selections(engine, files, filter_terms, refutations, &table.filter_keys).await;

    for (key, blocks) in blocks {
        let selection = match selections.remove(&key) {
            Some(rows) => rows.selection().intersection(blocks.selection()).into(),
            None => blocks,
        };

        selections.insert(key, selection);
    }

    selections
}

/// 每个能从索引定位命中行的文件，只读哪些行，按 object key。
async fn row_selections(
    engine: &DataFusionEngine,
    files: &[QueryFile],
    terms: &[(String, Vec<String>)],
    table: &TableInput,
) -> HashMap<String, ParquetRowSelection> {
    let Some(pruner) = &engine.tantivy_pruner else {
        return HashMap::new();
    };

    if terms.is_empty() {
        return HashMap::new();
    }

    let rows = pruner
        .matching_rows(files, terms, &table.index_keys, table.page_rows)
        .await;

    files
        .iter()
        .zip(rows)
        .filter_map(|(file, rows)| {
            let selection = selection(&rows?, file.rows)?;
            Some((file.object_key.clone(), ParquetRowSelection::new(selection)))
        })
        .collect()
}

/// 每个文件里条件树仍可能成立的块的行，按 object key。块过滤器在文件自己的过滤器 Artifact 里
/// （`filter_keys` 按段 id）；没有、读不出，或每块都可能成立的文件不在结果里。
async fn block_selections(
    engine: &DataFusionEngine,
    files: &[QueryFile],
    terms: &FilterTerms,
    refutations: &Refutations,
    filter_keys: &HashMap<String, String>,
) -> HashMap<String, ParquetRowSelection> {
    let Some(source) = &engine.catalog_source else {
        return HashMap::new();
    };

    if filter_keys.is_empty() || !refutations.could_drop(terms.gram_mask()) {
        return HashMap::new();
    }

    let tree = refutations.tree();

    // 流里放拥有所有权的值：异步闭包借用 `files` 的元素会让 future 的 `Send` 推断失败。
    let keyed = files
        .iter()
        .filter_map(|file| {
            let key = filter_keys.get(file.id.as_str())?.clone();
            let known = refutations.of(file);

            Some((
                key,
                file.id.to_string(),
                file.object_key.clone(),
                file.rows,
                known,
            ))
        })
        .collect::<Vec<_>>();

    stream::iter(keyed)
        .map(|(key, segment, object_key, rows, known)| async move {
            let filters = match source.partition_blooms(&key).await {
                Ok(filters) => filters?,
                Err(error) => {
                    tracing::warn!(object_key = key, %error, "block filters unreadable");
                    return None;
                }
            };

            let (block_rows, refuted) =
                refuted_blocks(source, &key, &filters, &segment, terms).await?;

            let selection = block_selection(rows, block_rows, &refuted, |refuted| {
                tree.may_hold(known | refuted)
            })?;

            Some((object_key, ParquetRowSelection::new(selection)))
        })
        .buffer_unordered(CONCURRENT_READS)
        .filter_map(std::future::ready)
        .collect()
        .await
}

/// For each block of `segment`, the atoms of `terms` that its block filters prove false, and the
/// rows of a block; bit-sliced filters are read from the filter object at `key`. `None` when no
/// `LIKE` field has block filters; a field whose blocks differ from the first one found proves
/// nothing.
async fn refuted_blocks(
    source: &CatalogQuerySource,
    key: &str,
    filters: &Filters,
    segment: &str,
    terms: &FilterTerms,
) -> Option<(usize, Vec<Refuted>)> {
    let mut found: Option<(usize, Vec<Refuted>)> = None;

    for (atom, field, grams) in terms.gram_atoms() {
        let Some(blocks) = filters.gram_blocks(field, segment) else {
            continue;
        };

        let (rows, refuted) = found.get_or_insert_with(|| (blocks.rows(), vec![0; blocks.len()]));

        if *rows != blocks.rows() || refuted.len() != blocks.len() {
            continue;
        }

        let ranges = blocks.ranges(grams);

        let read = match ranges.is_empty() {
            true => Vec::new(),
            false => match source.read_ranges(key, &ranges).await {
                Ok(read) => read,
                Err(error) => {
                    tracing::warn!(object_key = key, %error, "block filters unreadable");
                    continue;
                }
            },
        };

        let Some(holds) = blocks.may_hold(grams, &read) else {
            continue;
        };

        for (refuted, holds) in refuted.iter_mut().zip(holds) {
            if !holds {
                *refuted |= atom_bit(atom);
            }
        }
    }

    found
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

/// 有 `file_rows` 行、每块 `block_rows` 行的文件里，`holds` 对一块证明不成立的原子说条件树仍
/// 可能成立的块的行。块数与文件的行数对不上、或每块都可能成立时为 `None`。
fn block_selection(
    file_rows: u64,
    block_rows: usize,
    refuted: &[Refuted],
    holds: impl Fn(Refuted) -> bool,
) -> Option<RowSelection> {
    let file_rows = usize::try_from(file_rows).ok()?;

    if block_rows == 0 || refuted.len() != file_rows.div_ceil(block_rows) {
        return None;
    }

    let mut selectors: Vec<RowSelector> = Vec::new();
    let mut skipped = false;

    for (block, refuted) in refuted.iter().enumerate() {
        let rows = block_rows.min(file_rows - block * block_rows);
        let skip = !holds(*refuted);

        skipped |= skip;

        match selectors.last_mut() {
            Some(last) if last.skip == skip => last.row_count += rows,
            _ if skip => selectors.push(RowSelector::skip(rows)),
            _ => selectors.push(RowSelector::select(rows)),
        }
    }

    skipped.then(|| RowSelection::from(selectors))
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

    /// 五块里第 0、2、3 块证明了原子 0 不成立：只读第 1 块与最后不满的一块。
    #[test]
    fn only_the_blocks_where_the_condition_may_hold_are_read() {
        let refuted = [1, 0, 1, 1, 0];
        let holds = |refuted: Refuted| refuted == 0;

        let selection = block_selection(4 * 10 + 3, 10, &refuted, holds).unwrap();

        assert_eq!(
            rows_of(&selection),
            vec![(true, 10), (false, 10), (true, 20), (false, 3)]
        );
        assert_eq!(selection.row_count(), 13);
    }

    #[test]
    fn blocks_that_all_may_hold_or_do_not_fit_the_file_give_no_selection() {
        let holds = |refuted: Refuted| refuted == 0;

        assert!(block_selection(30, 10, &[0, 0, 0], holds).is_none());
        assert!(block_selection(31, 10, &[1, 0, 0], holds).is_none());
        assert!(block_selection(30, 0, &[], holds).is_none());

        let none = block_selection(25, 10, &[1, 1, 1], holds).unwrap();
        assert_eq!(rows_of(&none), vec![(true, 25)]);
    }
}
