// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Tantivy 候选裁剪：
//!
//! 对每个候选 `ParquetFileMeta`，从 object_store 按规范映射定位同小时的 `.ttv` Puffin sidecar，
//! 用 [`IndexHandle::count_term`] 查 `(field, term)`。任一谓词命中 0 文档则剔除该 file。
//!
//! `IndexHandle` 通过 [`ParquetMetaCache`]（容量/TTL 同 `parquet_meta` 层）按 sidecar
//! object_key 缓存，避免重复下载 + 解归档。

use std::{
    collections::HashSet,
    sync::{Arc, OnceLock},
};

use futures::{StreamExt, stream};
use object_store::{ObjectStore, ObjectStoreExt, path::Path};
use prometheus::IntCounter;
use sqlparser::{
    ast::{
        BinaryOperator, Expr, FunctionArg, FunctionArgExpr, FunctionArguments, SetExpr, Statement,
        Value,
    },
    dialect::GenericDialect,
    parser::Parser,
};

use crate::{
    domain::storage::ParquetFileMeta,
    infra::{
        caching::{ParquetMetaCache, TantivyFooterCache, TantivyResultCache, TantivyResultKey},
        search::tantivy_index::{IndexHandle, TantivyArchive, TantivyArchiveOpener},
    },
    shared::metrics::register_int_counter,
};

/// 单个 MATCH 谓词：`MATCH(<field>, '<term>')`。
#[derive(Debug, Clone)]
pub struct MatchPredicate {
    pub field: String,
    pub term: String,
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
    cache: Arc<ParquetMetaCache<Arc<IndexHandle>>>,
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
        cache: Arc<ParquetMetaCache<Arc<IndexHandle>>>,
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

    /// 按 `predicates` 裁剪：保留至少有一个 doc 命中**所有**谓词的 file。
    /// 谓词为空 → 全部保留。
    pub async fn prune(
        &self,
        candidates: Vec<ParquetFileMeta>,
        predicates: &[MatchPredicate],
    ) -> anyhow::Result<Vec<ParquetFileMeta>> {
        if predicates.is_empty() {
            return Ok(candidates);
        }
        // object_store HEAD / footer range-read 是每文件独立的 I/O。串行扫描会把
        // 候选文件数直接叠加到请求延迟，因此做有界并发；最后按原下标恢复
        // 顺序，避免破坏“最新文件优先”的后续扫描约定。
        let mut decisions = stream::iter(candidates.into_iter().enumerate())
            .map(|(index, file)| async move {
                let keep = self.candidate_matches(&file, predicates).await?;
                Ok::<_, anyhow::Error>((index, file, keep))
            })
            .buffer_unordered(PRUNE_CONCURRENCY)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<anyhow::Result<Vec<_>>>()?;
        decisions.sort_unstable_by_key(|(index, _, _)| *index);

        Ok(decisions
            .into_iter()
            .filter_map(|(_, file, keep)| {
                if keep {
                    Some(file)
                } else {
                    pruned_counter().inc();
                    None
                }
            })
            .collect())
    }

    async fn candidate_matches(
        &self,
        file: &ParquetFileMeta,
        predicates: &[MatchPredicate],
    ) -> anyhow::Result<bool> {
        // 不规范 key 没有可定位的 sidecar，保守保留交给 Parquet 过滤。
        let Some(index_object_key) = TantivyArchive::key_for(&file.object_key) else {
            return Ok(true);
        };
        let handle = match self.load_handle(&index_object_key).await {
            Ok(Some(handle)) => handle,
            Ok(None) => return Ok(true),
            Err(error) => {
                tracing::warn!(
                    archive = %index_object_key,
                    %error,
                    "tantivy archive load failed; keeping file as candidate"
                );
                return Ok(true);
            }
        };

        for predicate in predicates {
            let key = TantivyResultKey::new(
                index_object_key.as_str(),
                predicate.field.as_str(),
                predicate.term.as_str(),
            );
            let cached = match self.result_cache.as_ref() {
                Some(cache) => cache.get(&key).await,
                None => None,
            };
            let count = match cached {
                Some(count) => count as usize,
                None => match count_term_off_worker(
                    handle.clone(),
                    predicate.field.clone(),
                    predicate.term.clone(),
                )
                .await?
                {
                    Ok(count) => {
                        if let Some(cache) = self.result_cache.as_ref() {
                            cache.insert(key, count as u64).await;
                        }
                        count
                    }
                    // 字段不在该 sidecar schema 中时不能证明文件不命中。
                    Err(_) => return Ok(true),
                },
            };
            if count == 0 {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// 见 [`count_term_off_worker`]。
    ///
    /// 拉 footer → open → 缓存。`None` 表示对应 object 不存在（404）。
    ///
    /// IndexHandle cache miss 时的源数据查找顺序：
    /// 1. `footer_cache` 命中 → `open_with_cached_footer(store, key, footer.object_size,
    ///    footer)`，用缓存里的 object_size **跳过 `object_store.head`**，跳过 footer
    ///    parse 的两次 range-read，但仍由 tantivy 按需 sub-range 读 blob。
    /// 2. miss → `object_store.head(key)` 取 size → `open_from_object_store(...)`
    ///    →（成功）把 footer（含 object_size）写回 cache 供下次短路。
    ///
    /// change `tantivy-puffin-migration`：不再下载整 archive bytes，全程 range read。
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

                // 2. footer cache miss（或命中重开失败）：head 取 size 后走完整
                //    footer parse + open，并把 footer（含 size）写回 cache 供下次短路。
                let head = match store.head(&path).await {
                    Ok(m) => m,
                    Err(object_store::Error::NotFound { .. }) => {
                        return Err(anyhow::anyhow!("__missing__"));
                    }
                    Err(e) => return Err(anyhow::anyhow!("object_store head: {e}")),
                };
                let size = head.size as u64;

                match TantivyArchiveOpener::open_from_object_store(
                    store.clone(),
                    path.clone(),
                    size,
                )
                .await
                {
                    Ok(handle) => {
                        if let Some(fc) = footer_cache.as_ref() {
                            // 重新 parse 一次 footer 以入 cache（轻量；future opt: 让
                            // open_from_object_store 在内部缓存并返回）
                            let source = crate::tantivy::puffin::reader::PuffinBytesReader::new(
                                store.clone(),
                                path.clone(),
                                size,
                            );
                            match source.parse_footer().await {
                                Ok((meta, payload)) => {
                                    // 同步把 sync atomic_read 预物化目标（meta.json
                                    // 等）拉到 map，让 cache 命中路径完全零 IO 重建
                                    // IndexHandle。
                                    let atomic_files = match crate::tantivy::puffin_directory::reader::build_atomic_files(&meta, &source).await {
                                        Ok(m) => m,
                                        Err(e) => {
                                            tracing::warn!(
                                                archive = %key_for_load,
                                                error = %e,
                                                "tantivy atomic_files prefetch failed; cache not populated"
                                            );
                                            fc.record_error();
                                            return Ok(Arc::new(handle));
                                        }
                                    };
                                    let schema = handle.schema().clone();
                                    let footer = crate::tantivy::TantivyFooter {
                                        puffin_meta: Arc::new(meta),
                                        footer_payload_bytes: payload,
                                        schema,
                                        atomic_files: Arc::new(atomic_files),
                                        object_size: size,
                                    };
                                    fc.insert(key_for_load.clone(), Arc::new(footer)).await;
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        archive = %key_for_load,
                                        error = %e,
                                        "tantivy footer parse failed after open; cache not populated"
                                    );
                                    fc.record_error();
                                }
                            }
                        }
                        Ok(Arc::new(handle))
                    }
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

/// 从 SQL 文本中提取 `MATCH(field, 'term')` 调用（spec 8.5：只在 stmt 含 MATCH(...) 时调 pruner）。
/// 同时返回 SQL rewrite 后的字符串：把每处 `MATCH(field, 'term')` 替换为
/// `field LIKE '%term%'` 让 DataFusion 实际执行（避免依赖未注册的 UDF）。
/// 在 blocking 池上跑一次 tantivy term count。
///
/// `IndexHandle::count_term` 是同步 API，且往下会走到 puffin reader 的
/// `ensure_materialized_blocking` —— 那里用 `rx.recv()` **阻塞调用方线程**等一整个 blob
/// 从对象存储下载完。直接在 async 里调等于拿一次网络往返卡住一个 tokio worker。
///
/// 返回值是嵌套的：外层 `Err` 是 join 失败，内层 `Err` 表示字段不在 tantivy schema 里
/// （调用方据此保留候选而非剔除）。
async fn count_term_off_worker(
    handle: Arc<IndexHandle>,
    field: String,
    term: String,
) -> anyhow::Result<anyhow::Result<usize>> {
    tokio::task::spawn_blocking(move || handle.count_term(&field, &term))
        .await
        .map_err(|e| anyhow::anyhow!("tantivy count_term join: {e}"))
}

/// tantivy `default` 分词器丢弃超过此字节数的 token（`RemoveLongFilter`，默认 40）。
const TANTIVY_MAX_TOKEN_BYTES: usize = 40;

/// 该 term 是否会**原样**成为索引里的一个 token —— 即用它调 `count_term` 与 TEXT 字段
/// 的倒排内容一致，可安全用于裁剪。
///
/// indexed 字段以 `TEXT` 建，入索引时过 `default` 分词器（小写化 + 按非字母数字切分 +
/// 丢弃 > 40 字节的长 token）。而 `count_term` 用 `Term::from_field_text` 拿**原始串**查、
/// 不分词。两侧只有在 term 本就是「全小写 ASCII 字母数字、非空、不超长」时才一致；否则
/// 索引里存的是分词后的 token（如 `FAILED`→`failed`、`my-api`→`my`/`api`），用原串
/// `count_term` 恒返 0 → `prune` 把文件整个误裁 → 查询结果静默丢数据。
///
/// 这类 term 直接**不生成裁剪谓词**，纯靠 `extract_match_predicates` 生成的 `LIKE` rewrite
/// 在 DataFusion 侧兜底执行（`LIKE` 才是结果正确性的来源，tantivy 只做候选文件裁剪）。
pub fn can_prune_match_term(term: &str) -> bool {
    !term.is_empty()
        && term.len() <= TANTIVY_MAX_TOKEN_BYTES
        && term
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

pub fn extract_match_predicates(sql: &str) -> (Vec<MatchPredicate>, String) {
    // 每条查询都会调这里，正则只编译一次。
    static RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)MATCH\s*\(\s*([A-Za-z_][A-Za-z_0-9]*)\s*,\s*'([^']*)'\s*\)")
            .expect("static regex compiles")
    });
    let re = &*RE;
    // 只有顶层 WHERE 的合取项才对每一条结果都成立。OR / NOT 分支里的 MATCH
    // 仍改写成 LIKE 交给 Parquet 执行，但绝不能拿来剔除整个文件。
    let conjuncts = conjunct_match_predicates(sql);
    let mut preds = Vec::new();
    let rewritten = re
        .replace_all(sql, |caps: &regex::Captures| {
            let field = caps.get(1).unwrap().as_str().to_string();
            let term = caps.get(2).unwrap().as_str().to_string();
            // LIKE rewrite 恒生成（真正的执行语义）；裁剪谓词仅在 term 与 TEXT 索引内容
            // 一致时才生成，否则跳过裁剪以免分词差异导致误裁漏数据。
            if conjuncts.contains(&(field.clone(), term.clone())) && can_prune_match_term(&term) {
                preds.push(MatchPredicate {
                    field: field.clone(),
                    term: term.clone(),
                });
            }
            format!("{field} LIKE '%{}%'", term.replace('\'', "''"))
        })
        .to_string();
    (preds, rewritten)
}

fn conjunct_match_predicates(sql: &str) -> HashSet<(String, String)> {
    let Ok(statements) = Parser::parse_sql(&GenericDialect, sql) else {
        return HashSet::new();
    };
    let mut matches = HashSet::new();
    for statement in statements {
        if let Statement::Query(query) = statement
            && let SetExpr::Select(select) = query.body.as_ref()
            && let Some(selection) = &select.selection
        {
            collect_conjunct_matches(selection, &mut matches);
        }
    }
    matches
}

fn collect_conjunct_matches(expr: &Expr, matches: &mut HashSet<(String, String)>) {
    match expr {
        Expr::BinaryOp {
            left,
            op: BinaryOperator::And,
            right,
        } => {
            collect_conjunct_matches(left, matches);
            collect_conjunct_matches(right, matches);
        }
        Expr::Nested(inner) => collect_conjunct_matches(inner, matches),
        Expr::Function(function) if function.name.to_string().eq_ignore_ascii_case("match") => {
            if let Some(pair) = match_arguments(&function.args) {
                matches.insert(pair);
            }
        }
        _ => {}
    }
}

fn match_arguments(arguments: &FunctionArguments) -> Option<(String, String)> {
    let FunctionArguments::List(arguments) = arguments else {
        return None;
    };
    let [field, term] = arguments.args.as_slice() else {
        return None;
    };
    let field = match unnamed_expression(field)? {
        Expr::Identifier(identifier) => identifier.value.clone(),
        Expr::CompoundIdentifier(identifiers) => identifiers.last()?.value.clone(),
        _ => return None,
    };
    let Expr::Value(value) = unnamed_expression(term)? else {
        return None;
    };
    let Value::SingleQuotedString(term) = &value.value else {
        return None;
    };
    Some((field, term.clone()))
}

fn unnamed_expression(argument: &FunctionArg) -> Option<&Expr> {
    match argument {
        FunctionArg::Unnamed(FunctionArgExpr::Expr(expression)) => Some(expression),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
