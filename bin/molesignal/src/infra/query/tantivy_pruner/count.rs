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

use futures::{StreamExt, stream};
use parquet::file::properties::DEFAULT_DATA_PAGE_ROW_COUNT_LIMIT;

use super::{TantivyPruner, index_source};
use crate::{
    domain::storage::QueryFile,
    infra::{
        caching::TantivyResultKey,
        search::tantivy_index::{BarQuery, BarTally, IndexHandle},
    },
};

/// 同时计数的归档数。
const COUNT_CONCURRENCY: usize = 32;

/// 按行号读一个文件时最多读的行数。
const MAX_SELECTED_ROWS: usize = 4096;

/// 按值计数时一个段最多有的值；更多的字段（比如每行不同的 ID）交给扫描。
const MAX_COUNTED_VALUES: usize = 10_000;

impl TantivyPruner {
    /// 每个文件里 `terms`（`列 = '值'`）全部成立、`contains`（`列 ILIKE '%子串%'`）都出现的
    /// 行数，见 [`IndexHandle::count_exact`]。没有可用 Tantivy Artifact、归档打不开或答不出时为
    /// `None`，调用方照常扫描这些文件。
    pub async fn count_exact(
        &self,
        files: &[QueryFile],
        terms: &[(String, String)],
        contains: &[(String, String)],
        index_keys: &HashMap<String, String>,
    ) -> Vec<Option<u64>> {
        self.each_archive(files, index_keys, |key, _| async move {
            self.exact_count(&key, terms, contains).await
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
    /// Parquet 一页最多写 [`DEFAULT_DATA_PAGE_ROW_COUNT_LIMIT`] 行，命中不超过每列至少有的页数的
    /// 一半（也不超过 [`MAX_SELECTED_ROWS`]）时才给出，否则为 `None`。每列只有一页的文件不打开
    /// 索引。
    pub async fn matching_rows(
        &self,
        files: &[QueryFile],
        sets: &[(String, Vec<String>)],
        index_keys: &HashMap<String, String>,
    ) -> Vec<Option<Vec<u64>>> {
        self.each_archive(files, index_keys, |key, rows| async move {
            let pages = rows / DEFAULT_DATA_PAGE_ROW_COUNT_LIMIT as u64;

            let max_rows = usize::try_from(pages / 2)
                .unwrap_or(usize::MAX)
                .min(MAX_SELECTED_ROWS);

            if max_rows == 0 {
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
    ) -> Option<u64> {
        // 按子串数行要走遍词典：先把词典整个读进来。
        let handle = match contains {
            [] => self.handle_for_count(index_object_key).await?,
            _ => self.resident_handle(index_object_key, true).await?,
        };

        // 整值字段上单个 term 的命中数就是 `count_term` 的结果，与裁剪共用缓存。
        let cache_key = match (terms, contains) {
            ([(field, term)], []) if handle.indexes_whole_values(field) => Some(
                TantivyResultKey::new(index_object_key, field.as_str(), term.as_str()),
            ),
            _ => None,
        };

        if let (Some(key), Some(cache)) = (cache_key.as_ref(), self.result_cache.as_ref())
            && let Some(count) = cache.get(key).await
        {
            return Some(count);
        }

        let (terms, contains) = (terms.to_vec(), contains.to_vec());

        let count = blocking_count(index_object_key, handle, move |handle| {
            Ok(handle
                .count_exact(&terms, &contains)?
                .map(|count| count as u64))
        })
        .await?;

        if let (Some(key), Some(cache)) = (cache_key, self.result_cache.as_ref()) {
            cache.insert(key, count).await;
        }

        Some(count)
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
