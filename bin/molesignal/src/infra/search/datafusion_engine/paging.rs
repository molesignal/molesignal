// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 按时间取一页的语句：从最新（升序时最早）的文件读起，凑满一页即停，见 [`super::waves`]。文件
//! 到要读时才经过索引剪枝，剪掉的不读；等值条件命中很少行的文件只读命中的行所在的页，`LIKE` 只读
//! 它可能成立的块。Where the files' minute counts tell the minutes the page lies in, it is read
//! from those minutes alone, see [`super::newest`].

use std::collections::HashMap;

use arrow::array::RecordBatch;
use datafusion::{execution::object_store::ObjectStoreUrl, prelude::SessionContext};
use kernel::{Error, Result, time::TimeRange};

use super::{
    DataFusionEngine,
    blooms::FilterTerms,
    newest::page_window,
    selection::{rows_to_read, selections},
    tables::{TableInput, collect},
    waves::{TimeOrder, Waves},
};
use crate::infra::query::{
    refutations::Refutations,
    scan_limit::ensure_within_scan_limit,
    tantivy_pruner::{IndexedAtom, counts_rows},
};

/// 读一页要用到的：引擎与会话、语句、请求的时间窗与行数上限，以及还要剪枝时的原子条件。
pub(super) struct Page<'a> {
    pub(super) engine: &'a DataFusionEngine,
    pub(super) ctx: &'a SessionContext,
    pub(super) object_store_url: &'a ObjectStoreUrl,
    pub(super) time_range: TimeRange,
    pub(super) limit: Option<usize>,
    pub(super) sql: &'a str,
    /// 文件还没经过 Tantivy 剪枝时它能判断的原子条件；已经剪过或没有时为空。
    pub(super) atoms: &'a [IndexedAtom],
    /// 前面几步已证明不成立的原子，Tantivy 的证明也记在这里。
    pub(super) refutations: &'a mut Refutations,
    pub(super) exact_terms: &'a [(String, Vec<String>)],
    pub(super) filter_terms: &'a FilterTerms,
}

/// 读出 `table` 上按 `order` 取的一页，返回结果与读过的行数。
pub(super) async fn read(
    page: Page<'_>,
    order: TimeOrder,
    mut table: TableInput,
) -> Result<(Vec<RecordBatch>, u64)> {
    let engine = page.engine;

    let buffered_rows = table
        .buffered
        .iter()
        .map(|batch| batch.num_rows() as u64)
        .sum::<u64>();

    let pruner = engine
        .tantivy_pruner
        .as_ref()
        .filter(|_| !page.atoms.is_empty());

    let counted = pruner.is_some() && counts_rows(page.refutations.tree(), page.atoms);

    let page_size = TimeOrder {
        fetch: page
            .limit
            .map_or(order.fetch, |limit| limit.min(order.fetch)),
        ..order
    };

    // The minutes the page lies in: files wholly outside them are not read, and the page index
    // skips the pages of the rest outside them.
    let time_range = match page_window(
        engine,
        (page.sql, &table.name),
        &table.files,
        &table.minute_keys,
        page.time_range,
        page_size,
    )
    .await
    {
        Some(window) => {
            table.files.retain(|file| {
                file.time_range.start.0 < window.end.0 && window.start.0 <= file.time_range.end.0
            });

            window
        }
        None => page.time_range,
    };

    let mut waves = Waves::new(
        order,
        std::mem::take(&mut table.files),
        page.limit,
        pruner.is_some(),
        counted,
    );

    let mut looked = 0;
    let mut last = None;

    loop {
        while let Some(unchecked) = waves.unchecked() {
            let unchecked = unchecked.to_vec();

            match pruner {
                Some(pruner) => {
                    let pruned = pruner
                        .prune_counting(
                            unchecked,
                            page.atoms,
                            &mut *page.refutations,
                            &table.index_keys,
                        )
                        .await
                        .map_err(|error| Error::internal(format!("tantivy prune: {error}")))?;

                    waves.checked(pruned.files, &pruned.rows);
                }
                None => waves.checked(unchecked, &HashMap::new()),
            }
        }

        // 剪枝后没有新文件可读：上一批的结果就是答案。
        let Some(files) = waves.take() else {
            return Ok(last.unwrap_or_default());
        };

        // 只给这一批新加入的文件找要读的行。
        let found = selections(
            engine,
            &files[looked..],
            &table,
            page.exact_terms,
            page.filter_terms,
            page.refutations,
        )
        .await;

        table.selections.extend(found);
        looked = files.len();

        let scanned_rows = rows_to_read(files, &table.selections).saturating_add(buffered_rows);

        ensure_within_scan_limit(scanned_rows, engine.max_scan_rows)?;

        table.register(page.ctx, engine, files, page.object_store_url, time_range)?;

        let (_, batches) = collect(page.ctx, page.sql, page.limit).await?;

        if !waves.next(&batches) {
            return Ok((batches, scanned_rows));
        }

        last = Some((batches, scanned_rows));
    }
}
