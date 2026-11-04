// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Tantivy 候选裁剪：
//!
//! Catalog 查询为每个候选 `QueryFile` 提供显式的 Tantivy Artifact object key。条件树
//! （[`WhereTree`]）里整值字段的 `field = 'value'` 与 `field IN (...)` 用
//! [`IndexHandle::count_term`] 查命中数；`MATCH`、`MATCH_TEXT` 与 `LIKE` / `ILIKE` 的字面量按子串
//! 执行，用 [`IndexHandle::may_contain`] 判断文件里可能不可能有。这样证明不成立的原子条件与前面
//! 几步证明的合起来，条件树在文件里已不可能成立，就剔除该 file（见 [`Refutations`]）。缺失
//! Artifact 时保守回退扫描 Primary Parquet；任何调用方都不得从主对象路径反推索引对象路径。
//!
//! `IndexHandle` 通过 [`IndexHandleCache`]（容量/TTL 同 `index_handle` 层）按 Index
//! Artifact object key 缓存，避免重复下载 + 解归档。

use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
};

use futures::{StreamExt, stream};
use object_store::{ObjectStore, path::Path};
use prometheus::IntCounter;

use crate::{
    domain::storage::QueryFile,
    infra::{
        caching::{IndexHandleCache, TantivyFooterCache, TantivyResultCache, TantivyResultKey},
        query::{
            parser::{Refuted, WhereTree, atom_bit},
            refutations::Refutations,
        },
        search::tantivy_index::{IndexHandle, TantivyArchiveOpener},
    },
    shared::metrics::register_int_counter,
};

mod atoms;
mod count;
mod index_source;
mod match_sql;
mod match_text;

pub use atoms::{IndexedAtom, counts_rows, indexed_atoms, key_value_conjuncts, mask};
pub use match_sql::{match_text_fields, rewrite_match_calls};

/// 单个裁剪谓词：字段满足 `terms` 之一。子串谓词来自 `MATCH`、`MATCH_TEXT` 与 `LIKE` /
/// `ILIKE` 模式里的一段字面量；整值谓词来自整值字段上的 `=` 与 `IN`（`IN` 的几个值是几个词）。
/// 字段名是 DataFusion 解析出的列名。
#[derive(Debug, Clone)]
pub struct MatchPredicate {
    pub field: String,
    /// 任一成立即可，至少一个。
    pub terms: Vec<String>,
    pub matching: TermMatch,
}

impl MatchPredicate {
    pub fn new(field: impl Into<String>, term: impl Into<String>, matching: TermMatch) -> Self {
        Self {
            field: field.into(),
            terms: vec![term.into()],
            matching,
        }
    }
}

/// 谓词怎样判断一个文件里有没有满足它的行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermMatch {
    /// 整值字段等于某个词：词典里有这个值才有这样的行，各个词的命中数之和就是行数。
    Value,
    /// 分词字段含有某个子串（小写化，按 `ILIKE '%term%'` 执行），见
    /// [`IndexHandle::may_contain`]。
    Substring,
    /// 整值字段的值含有某段文字：`LIKE`，`case_insensitive` 时是 `ILIKE`，见
    /// [`IndexHandle::may_contain_value`]。
    InValue { case_insensitive: bool },
}

/// 裁剪留下的文件，与整值原子的命中数给出的每个文件最多有几行满足条件树（按 object key）。
pub struct Pruned {
    pub files: Vec<QueryFile>,
    pub rows: HashMap<String, u64>,
}

/// 一个文件的归档对原子条件的回答。
#[derive(Default)]
struct Verdict {
    /// 证明不成立的原子。
    refuted: Refuted,
    /// 整值原子与它的命中数。
    rows: HashMap<usize, u64>,
}

static PRUNED_TOTAL: OnceLock<IntCounter> = OnceLock::new();
const PRUNE_CONCURRENCY: usize = 16;

fn pruned_counter() -> &'static IntCounter {
    PRUNED_TOTAL.get_or_init(|| {
        register_int_counter(
            "tantivy_pruned_files_total",
            "files skipped by tantivy index pruner",
        )
    })
}

pub struct TantivyPruner {
    cache: Arc<IndexHandleCache<Arc<IndexHandle>>>,
    object_store: Arc<dyn ObjectStore>,
    /// `(index_object_key, field, term) → count`：命中时跳过 `IndexHandle::count_term`。
    /// 默认 `None`（向后兼容老 wire），bootstrap 通过 [`Self::with_result_cache`] 注入。
    result_cache: Option<Arc<TantivyResultCache>>,
    /// `index_object_key → Arc<TantivyFooter>`：IndexHandle TTL 过期重新打开时短路掉
    /// 对象存储 GET。默认 `None`，bootstrap 通过 [`Self::with_footer_cache`] 注入。
    footer_cache: Option<Arc<TantivyFooterCache>>,
}

impl TantivyPruner {
    pub fn new(
        cache: Arc<IndexHandleCache<Arc<IndexHandle>>>,
        object_store: Arc<dyn ObjectStore>,
    ) -> Self {
        Self {
            cache,
            object_store,
            result_cache: None,
            footer_cache: None,
        }
    }

    pub fn with_result_cache(mut self, result_cache: Arc<TantivyResultCache>) -> Self {
        self.result_cache = Some(result_cache);
        self
    }

    pub fn with_footer_cache(mut self, footer_cache: Arc<TantivyFooterCache>) -> Self {
        self.footer_cache = Some(footer_cache);
        self
    }

    /// 用 `atoms` 再证明一些原子条件不成立，留下条件树仍可能成立的文件，并给出整值原子数出的每个
    /// 留下的文件最多有几行满足条件树：按时间取一页的查询据此决定先读哪些文件。
    ///
    /// Index object keys come exclusively from explicit Artifact relationships. Absence or an
    /// unusable Artifact conservatively keeps the Primary file.
    pub async fn prune_counting(
        &self,
        candidates: Vec<QueryFile>,
        atoms: &[IndexedAtom],
        refutations: &mut Refutations,
        index_keys: &HashMap<String, String>,
    ) -> anyhow::Result<Pruned> {
        if atoms.is_empty() {
            return Ok(Pruned {
                files: candidates,
                rows: HashMap::new(),
            });
        }

        let known = candidates
            .iter()
            .map(|file| refutations.of(file))
            .collect::<Vec<_>>();

        let tree = refutations.tree();

        // object_store HEAD / footer range-read 是每文件独立的 I/O。串行扫描会把
        // 候选文件数直接叠加到请求延迟，因此做有界并发；最后按原下标恢复
        // 顺序，避免破坏“最新文件优先”的后续扫描约定。
        let mut decisions = stream::iter(candidates.into_iter().zip(known).enumerate())
            .map(|(index, (file, known))| async move {
                let verdict = match index_source::key_for(index_keys, &file) {
                    Some(index_key) => self.verdict(tree, atoms, known, index_key).await?,
                    None => Verdict::default(),
                };

                Ok::<_, anyhow::Error>((index, file, verdict))
            })
            .buffer_unordered(PRUNE_CONCURRENCY)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<anyhow::Result<Vec<_>>>()?;

        decisions.sort_unstable_by_key(|(index, _, _)| *index);

        let mut pruned = Pruned {
            files: Vec::with_capacity(decisions.len()),
            rows: HashMap::new(),
        };

        for (_, file, verdict) in decisions {
            if !refutations.record(&file, verdict.refuted) {
                pruned_counter().inc();
                continue;
            }

            // 不成立的原子没有一行满足：OR 里被证明不成立的一支上界是 0。
            let refuted = refutations.of(&file);

            if let Some(rows) = refutations
                .tree()
                .bound(&|atom| match refuted & atom_bit(atom) {
                    0 => verdict.rows.get(&atom).copied(),
                    _ => Some(0),
                })
            {
                pruned.rows.insert(file.object_key.clone(), rows);
            }

            pruned.files.push(file);
        }

        Ok(pruned)
    }

    /// 一个文件的归档对 `atoms` 的回答。`known` 是别的步骤已证明不成立的原子，不再查；条件树
    /// 已不可能成立时不再查别的原子。
    async fn verdict(
        &self,
        tree: &WhereTree,
        atoms: &[IndexedAtom],
        known: Refuted,
        index_object_key: &str,
    ) -> anyhow::Result<Verdict> {
        let mut verdict = Verdict::default();

        let handle = match self.load_handle(index_object_key).await {
            Ok(Some(handle)) => handle,
            Ok(None) => return Ok(verdict),
            Err(error) => {
                tracing::warn!(
                    archive = index_object_key,
                    %error,
                    "tantivy archive load failed; keeping file as candidate"
                );
                return Ok(verdict);
            }
        };

        for atom in atoms {
            let bit = atom_bit(atom.atom);

            if (known | verdict.refuted) & bit != 0 {
                continue;
            }

            match self.atom_rows(&handle, index_object_key, atom).await? {
                Some(0) => {
                    verdict.refuted |= bit;

                    if !tree.may_hold(known | verdict.refuted) {
                        break;
                    }
                }
                Some(rows) => {
                    verdict.rows.insert(atom.atom, rows);
                }
                None => {}
            }
        }

        Ok(verdict)
    }

    /// 一个原子在一个归档里的回答：`Some(0)` 时它在文件里不成立；整值原子另给出命中数。答不出、
    /// 或子串可能出现时为 `None`。答不出的谓词跳过：别的谓词仍可能证明它不成立。
    async fn atom_rows(
        &self,
        handle: &Arc<IndexHandle>,
        index_object_key: &str,
        atom: &IndexedAtom,
    ) -> anyhow::Result<Option<u64>> {
        let mut rows = None;

        for predicate in &atom.predicates {
            match self.found_any(handle, index_object_key, predicate).await? {
                Some(0) => return Ok(Some(0)),
                Some(found) if predicate.matching == TermMatch::Value => {
                    rows = Some(rows.map_or(found, |rows: u64| rows.min(found)));
                }
                _ => {}
            }
        }

        Ok(rows)
    }

    /// 谓词各个词的回答合起来：整值谓词是各词命中数之和（一行只有一个值，各词的行互不相交），
    /// 子串谓词有一个词可能出现就是 1。有一个词答不出、又没有别的词证明有时为 `None`。
    async fn found_any(
        &self,
        handle: &Arc<IndexHandle>,
        index_object_key: &str,
        predicate: &MatchPredicate,
    ) -> anyhow::Result<Option<u64>> {
        let mut total = Some(0_u64);

        for term in &predicate.terms {
            match self
                .found(
                    handle,
                    index_object_key,
                    &predicate.field,
                    term,
                    predicate.matching,
                )
                .await?
            {
                Some(found) if predicate.matching != TermMatch::Value && found > 0 => {
                    return Ok(Some(1));
                }
                Some(found) => total = total.map(|total| total.saturating_add(found)),
                None => total = None,
            }
        }

        Ok(total)
    }

    /// 一个词在一个归档里的回答，与结果缓存共用：整值谓词是命中数，子串谓词可能有时为 1、一定
    /// 没有时为 0。答不出（字段不在这个归档的 schema 里、在这个归档里的索引方式与谓词不符、
    /// 归档太旧）时为 `None`。
    ///
    /// 查询是同步 API：hot cache 未覆盖的读会在 puffin reader 里阻塞调用方线程等对象存储返回，
    /// 所以放到 blocking 池上，不占 tokio worker。
    async fn found(
        &self,
        handle: &Arc<IndexHandle>,
        index_object_key: &str,
        field: &str,
        term: &str,
        matching: TermMatch,
    ) -> anyhow::Result<Option<u64>> {
        // 子串的回答存在与命中数不同的键下，用控制字符隔开：它们不会出现在 SQL 字面量里。
        let cached_term = match matching {
            TermMatch::Value => term.to_owned(),
            TermMatch::Substring => format!("\0{term}"),
            TermMatch::InValue {
                case_insensitive: false,
            } => format!("\u{1}{term}"),
            TermMatch::InValue {
                case_insensitive: true,
            } => format!("\u{2}{term}"),
        };

        let key = TantivyResultKey::new(index_object_key, field, cached_term.as_str());

        if let Some(cache) = self.result_cache.as_ref()
            && let Some(found) = cache.get(&key).await
        {
            return Ok(Some(found));
        }

        let handle = handle.clone();
        let (field, term) = (field.to_owned(), term.to_owned());

        let answer = tokio::task::spawn_blocking(move || match matching {
            TermMatch::Value => handle
                .count_term(&field, &term)
                .map(|count| count.map(|count| count as u64)),
            TermMatch::Substring => handle
                .may_contain(&field, &term)
                .map(|found| found.map(u64::from)),
            TermMatch::InValue { case_insensitive } => handle
                .may_contain_value(&field, &term, case_insensitive)
                .map(|found| found.map(u64::from)),
        })
        .await
        .map_err(|e| anyhow::anyhow!("tantivy lookup join: {e}"))?;

        let Ok(Some(found)) = answer else {
            return Ok(None);
        };

        if let Some(cache) = self.result_cache.as_ref() {
            cache.insert(key, found).await;
        }

        Ok(Some(found))
    }

    /// 拉 footer → open → 缓存。`None` 表示对应 object 不存在（404）。
    ///
    /// IndexHandle cache miss 时的源数据查找顺序：
    /// 1. `footer_cache` 命中 → `open_with_cached_footer(store, key, footer.object_size,
    ///    footer)`，零 IO 重开；hot cache 未覆盖的读仍由 tantivy 按需 sub-range 读 blob。
    /// 2. miss → `open_tail(store, key)`：一次后缀范围读取拿到 footer、元数据 blob 与对象大小，
    ///    不发 HEAD →（成功）把打开时已读到的 footer（含 object_size 与 hot cache）写回 cache。
    ///
    /// Archive content is never downloaded as a whole; all blob access uses range reads.
    async fn load_handle(
        &self,
        index_object_key: &str,
    ) -> anyhow::Result<Option<Arc<IndexHandle>>> {
        let store = self.object_store.clone();
        let footer_cache = self.footer_cache.clone();
        let key = index_object_key.to_string();
        let key_for_load = key.clone();

        let result = self
            .cache
            .get_or_load(key, || async move {
                let path = Path::from(key_for_load.as_str());

                // 1. footer cache 短路：命中则用缓存里的 object_size 零 IO 重开，
                //    完全跳过 object_store head/GET（IndexHandle TTL 过期后的快路径，
                //    即使对象暂时不可达也能复用已缓存的 footer）。
                if let Some(fc) = footer_cache.as_ref()
                    && let Some(footer) = fc.get(&key_for_load).await {
                        match TantivyArchiveOpener::open_with_cached_footer(
                            store.clone(),
                            path.clone(),
                            footer.object_size,
                            &footer,
                        ) {
                            Ok(handle) => return Ok(Arc::new(handle)),
                            Err(e) => {
                                tracing::warn!(
                                    archive = %key_for_load,
                                    error = %e,
                                    "tantivy footer cache hit but open_with_cached_footer failed; falling back to footer parse"
                                );
                                fc.record_error();
                            }
                        }
                    }

                // 2. footer cache miss（或命中重开失败）：一次后缀范围读取拿到末尾与对象大小
                //    （不发 HEAD），完成 footer parse + open，并把打开时已读到的 footer（含 size）
                //    写回 cache 供下次短路。
                match TantivyArchiveOpener::open_tail(store.clone(), path.clone()).await {
                    Ok(handle) => {
                        if let Some(fc) = footer_cache.as_ref() {
                            fc.insert(key_for_load.clone(), Arc::new(handle.footer())).await;
                        }

                        Ok(Arc::new(handle))
                    }
                    Err(e) if is_not_found(&e) => Err(anyhow::anyhow!("__missing__")),
                    Err(e) => Err(anyhow::anyhow!("tantivy open: {e}")),
                }
            })
            .await;

        match result {
            Ok(h) => Ok(Some(h)),
            Err(e) if e.to_string().contains("__missing__") => Ok(None),
            Err(e) => Err(e),
        }
    }
}

fn is_not_found(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        matches!(
            cause.downcast_ref::<object_store::Error>(),
            Some(object_store::Error::NotFound { .. })
        )
    })
}

#[cfg(test)]
mod tests;
