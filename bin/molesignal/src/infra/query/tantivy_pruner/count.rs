// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 用 Tantivy 归档回答计数与定位行：等值与子串过滤成立的行数（`count(*)`）、explorer 直方图
//! 每根柱子的行数、字段每个值的行数，以及等值过滤命中的行在文件里的行号。文件的每一行在归档里
//! 正好是一个文档，归档里数出的就是文件里的；没有可用归档、或归档答不出的文件，由调用方照常
//! 扫描。

use std::{
    collections::{BTreeMap, HashMap},
    future::Future,
    sync::Arc,
};

use domain::storage::QueryFile;
use futures::{StreamExt, stream};
use query_language::NumberRange;

use super::{TantivyPruner, index_source};
use crate::infra::{
    caching::TantivyResultKey,
    search::tantivy_index::{BarQuery, BarTally, IndexHandle},
};

/// 同时计数的归档数。
const COUNT_CONCURRENCY: usize = 32;

/// 按行号读一个文件时最多读的行数。
const MAX_SELECTED_ROWS: usize = 4096;

/// 按值计数时一个段最多有的值；更多的字段（比如每行不同的 ID）交给扫描。
const MAX_COUNTED_VALUES: usize = 10_000;

/// 按各字段互不相关估计的同时命中数比能按行号读的行多出这么多倍，才不再打开归档定位行。
const EXPECTED_MARGIN: u64 = 4;

impl TantivyPruner {
    /// 每个文件里 `terms`（`列 = '值'`）全部成立、`contains`（`列 ILIKE '%子串%'`）都出现的
    /// 行数，见 [`IndexHandle::count_exact`]。没有可用 Tantivy Artifact、归档打不开或答不出时为
    /// `None`，调用方照常扫描这些文件。
    pub async fn count_exact(
        &self,
        files: &[QueryFile],
        terms: &[(String, String)],
        contains: &[(String, String)],
        ranges: &[(String, NumberRange)],
        index_keys: &HashMap<String, String>,
    ) -> Vec<Option<u64>> {
        self.each_archive(files, index_keys, |key, _| async move {
            self.exact_count(&key, terms, contains, ranges).await
        })
        .await
    }

    /// 每个文件按 `query` 分柱的计数，见 [`IndexHandle::count_bars`]。没有可用 Tantivy
    /// Artifact、归档没有行时间快字段或答不出时为 `None`，调用方照常扫描这些文件。
    pub async fn count_bars(
        &self,
        files: &[QueryFile],
        query: &BarQuery,
        index_keys: &HashMap<String, String>,
    ) -> Vec<Option<BTreeMap<i64, BarTally>>> {
        self.each_archive(files, index_keys, |key, _| async move {
            let handle = self
                .resident_handle(&key, !query.contains.is_empty())
                .await?;

            let query = query.clone();

            blocking_count(&key, handle, move |handle| handle.count_bars(&query)).await
        })
        .await
    }

    /// 每个文件里每个字段都取 `sets` 里它的值之一的行号，从小到大。只读这些行要能跳过页才划算：
    /// 文件一页最多写 `page_rows` 行，命中不超过每列至少有的页数的一半（也不超过
    /// [`MAX_SELECTED_ROWS`]）时才给出，否则为 `None`。每列只有一页的文件不打开
    /// 索引；裁剪时已数出的各个值的行数说明命中超过上限的文件也不打开，见 [`exceeds`]。
    pub async fn matching_rows(
        &self,
        files: &[QueryFile],
        sets: &[(String, Vec<String>)],
        index_keys: &HashMap<String, String>,
        page_rows: usize,
    ) -> Vec<Option<Vec<u64>>> {
        self.each_archive(files, index_keys, |key, rows| async move {
            let pages = rows / page_rows.max(1) as u64;

            let max_rows = usize::try_from(pages / 2)
                .unwrap_or(usize::MAX)
                .min(MAX_SELECTED_ROWS);

            if max_rows == 0 || self.too_many_matches(&key, sets, rows, max_rows).await {
                return None;
            }

            let handle = self.handle_for_count(&key).await?;
            let sets = sets.to_vec();

            blocking_count(&key, handle, move |handle| {
                handle.matching_rows(&sets, max_rows)
            })
            .await
        })
        .await
    }

    /// 每个文件里 `column` 的每个值出现在几行，见 [`IndexHandle::count_values`]：只读词典，
    /// 调用方只交来整个落在时间窗里的文件。答不出的文件为 `None`。
    pub async fn count_values(
        &self,
        files: &[QueryFile],
        column: &str,
        index_keys: &HashMap<String, String>,
    ) -> Vec<Option<HashMap<String, u64>>> {
        self.each_archive(files, index_keys, |key, _| async move {
            let handle = self.handle_for_count(&key).await?;
            let column = column.to_owned();

            blocking_count(&key, handle, move |handle| {
                handle.count_values(&column, MAX_COUNTED_VALUES)
            })
            .await
        })
        .await
    }

    /// 计数要读的归档，各段快字段已读进内存；`dictionaries` 时词典也是，见
    /// [`IndexHandle::with_resident`]。
    async fn resident_handle(
        &self,
        index_object_key: &str,
        dictionaries: bool,
    ) -> Option<Arc<IndexHandle>> {
        let handle = self.handle_for_count(index_object_key).await?;

        match handle.with_resident(dictionaries).await {
            Ok(handle) => Some(Arc::new(handle)),
            Err(error) => {
                tracing::warn!(
                    archive = index_object_key,
                    %error,
                    "tantivy fast field read failed; scanning the file instead"
                );
                None
            }
        }
    }

    async fn exact_count(
        &self,
        index_object_key: &str,
        terms: &[(String, String)],
        contains: &[(String, String)],
        ranges: &[(String, NumberRange)],
    ) -> Option<u64> {
        // 按子串数行要走遍词典：先把词典整个读进来。
        let handle = match contains {
            [] => self.handle_for_count(index_object_key).await?,
            _ => self.resident_handle(index_object_key, true).await?,
        };

        // 整值字段上单个 term 的命中数就是 `count_term` 的结果，与裁剪共用缓存。
        let cache_key = match (terms, contains, ranges) {
            ([(field, term)], [], []) if handle.indexes_whole_values(field) => Some(
                TantivyResultKey::new(index_object_key, field.as_str(), term.as_str()),
            ),
            _ => None,
        };

        if let (Some(key), Some(cache)) = (cache_key.as_ref(), self.result_cache.as_ref())
            && let Some(count) = cache.get(key).await
        {
            return Some(count);
        }

        let (terms, contains, ranges) = (terms.to_vec(), contains.to_vec(), ranges.to_vec());

        let count = blocking_count(index_object_key, handle, move |handle| {
            Ok(handle
                .count_matching(&terms, &contains, &ranges)?
                .map(|count| count as u64))
        })
        .await?;

        if let (Some(key), Some(cache)) = (cache_key, self.result_cache.as_ref()) {
            cache.insert(key, count).await;
        }

        Some(count)
    }

    /// 结果缓存里有 `sets` 的每个值在归档 `index_object_key` 里的行数（裁剪时数出的）时，按
    /// [`exceeds`] 判断有 `rows` 行的文件里同时命中各字段的行是否多过 `max_rows`。缺哪个值的
    /// 行数都不判断：打开归档数一次。
    async fn too_many_matches(
        &self,
        index_object_key: &str,
        sets: &[(String, Vec<String>)],
        rows: u64,
        max_rows: usize,
    ) -> bool {
        let Some(cache) = &self.result_cache else {
            return false;
        };

        let mut counts = Vec::with_capacity(sets.len());

        for (field, values) in sets {
            let mut count = 0_u64;

            for value in values {
                let key = TantivyResultKey::new(index_object_key, field.as_str(), value.as_str());

                let Some(found) = cache.get(&key).await else {
                    return false;
                };

                count = count.saturating_add(found);
            }

            counts.push(count);
        }

        exceeds(&counts, rows, max_rows as u64)
    }

    /// 计数要读的归档。打不开时记一条警告，调用方扫描该文件。
    async fn handle_for_count(&self, index_object_key: &str) -> Option<Arc<IndexHandle>> {
        match self.load_handle(index_object_key).await {
            Ok(handle) => handle,
            Err(error) => {
                tracing::warn!(
                    archive = index_object_key,
                    %error,
                    "tantivy archive load failed; scanning the file to count it"
                );
                None
            }
        }
    }

    /// 对 `files` 各自的 Tantivy Artifact 并发调用 `count(归档 key, 文件行数)`，结果与 `files`
    /// 一一对应；没有 Artifact 的文件为 `None`。
    async fn each_archive<T, Fut>(
        &self,
        files: &[QueryFile],
        index_keys: &HashMap<String, String>,
        count: impl Fn(String, u64) -> Fut,
    ) -> Vec<Option<T>>
    where
        Fut: Future<Output = Option<T>>,
    {
        // 流里放拥有所有权的 key：异步闭包借用 `files` 的元素会让 future 的 `Send` 推断失败。
        let keys: Vec<Option<(String, u64)>> = files
            .iter()
            .map(|file| {
                index_source::key_for(index_keys, file).map(|key| (key.to_owned(), file.rows))
            })
            .collect();

        // 计数只读每个归档的一小部分，同时读的归档比裁剪多。
        let mut counts = stream::iter(keys.into_iter().enumerate())
            .map(|(index, key)| {
                let counted = key.map(|(key, rows)| count(key, rows));

                async move {
                    let count = match counted {
                        Some(counted) => counted.await,
                        None => None,
                    };

                    (index, count)
                }
            })
            .buffer_unordered(COUNT_CONCURRENCY)
            .collect::<Vec<_>>()
            .await;

        counts.sort_unstable_by_key(|(index, _)| *index);
        counts.into_iter().map(|(_, count)| count).collect()
    }
}

/// 有 `rows` 行的文件里，各字段命中的行数分别是 `counts` 时（一个字段的命中是它各个值的行数
/// 之和，每行只有一个值），同时命中所有字段的行是否多过 `max_rows`：
///
/// - 至多是最少的那个字段的命中数，不多过时不算；
/// - 至少是各字段没命中的行都去掉之后剩下的，多过时一定算，一个字段时就是它的命中数；
/// - 两者之间按各字段互不相关估计，估出的比 `max_rows` 多 [`EXPECTED_MARGIN`] 倍以上才算。
///   估错只是照常扫描这个文件，或白开一次归档。
fn exceeds(counts: &[u64], rows: u64, max_rows: u64) -> bool {
    let Some(&fewest) = counts.iter().min() else {
        return false;
    };

    if fewest <= max_rows || rows == 0 {
        return false;
    }

    let missed = counts
        .iter()
        .map(|count| rows.saturating_sub(*count))
        .fold(0_u64, u64::saturating_add);

    if rows.saturating_sub(missed) > max_rows {
        return true;
    }

    let expected = counts.iter().fold(rows as f64, |expected, count| {
        expected * (*count as f64 / rows as f64)
    });

    expected > max_rows.saturating_mul(EXPECTED_MARGIN) as f64
}

/// 在 blocking 线程上跑 `count`：hot cache 未覆盖的读会同步等对象存储返回。出错时记一条
/// 警告并返回 `None`。
async fn blocking_count<T: Send + 'static>(
    index_object_key: &str,
    handle: Arc<IndexHandle>,
    count: impl FnOnce(&IndexHandle) -> anyhow::Result<Option<T>> + Send + 'static,
) -> Option<T> {
    match tokio::task::spawn_blocking(move || count(&handle)).await {
        Ok(Ok(counted)) => counted,
        Ok(Err(error)) => {
            tracing::warn!(
                archive = index_object_key,
                %error,
                "tantivy count failed; scanning the file to count it"
            );
            None
        }
        Err(error) => {
            tracing::warn!(archive = index_object_key, %error, "tantivy count join failed");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_field_matches_the_sum_of_its_values() {
        assert!(exceeds(&[1_500], 50_000, 1));
        assert!(!exceeds(&[1], 50_000, 1));
        assert!(!exceeds(&[], 50_000, 1));
    }

    #[test]
    fn fields_that_cover_most_rows_must_match_together() {
        // 至少 45,000 + 45,000 − 50,000 行两个字段都命中。
        assert!(exceeds(&[45_000, 45_000], 50_000, 7));
    }

    #[test]
    fn unrelated_fields_are_estimated_and_a_rare_value_is_looked_up() {
        // 3% 与 13% 的行：估计约 195 行两个都命中。
        assert!(exceeds(&[1_500, 6_500], 50_000, 1));

        // 至多 3 行两个都命中：只读这几行划算。
        assert!(!exceeds(&[3, 6_500], 50_000, 7));

        // 估计约 2 行：没有多过能按行号读的行的 4 倍。
        assert!(!exceeds(&[100, 1_000], 50_000, 1));
    }
}
