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
    ops::ControlFlow,
    sync::{Arc, OnceLock},
};

use futures::{StreamExt, stream};
use object_store::{ObjectStore, ObjectStoreExt, path::Path};
use prometheus::IntCounter;
use sqlparser::{
    ast::{
        BinaryOperator, Expr, FunctionArg, FunctionArgExpr, FunctionArguments, SetExpr, Statement,
        Value, visit_expressions,
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
    // 每条查询都会调这里，正则只编译一次。`MATCH_TEXT` 在前避免被 `MATCH` 前缀吞掉。
    static RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"(?i)(MATCH_TEXT|MATCH)\s*\(\s*([A-Za-z_][A-Za-z_0-9]*)\s*,\s*'([^']*)'\s*\)",
        )
        .expect("static regex compiles")
    });
    let re = &*RE;
    // 只有顶层 WHERE 的合取项才对每一条结果都成立。OR / NOT 分支里的 MATCH / MATCH_TEXT
    // 仍改写成 ILIKE 交给 Parquet 执行，但绝不能拿来剔除整个文件。
    let conjuncts = conjunct_match_predicates(sql);
    let mut preds = Vec::new();
    let rewritten = re
        .replace_all(sql, |caps: &regex::Captures| {
            let func = caps.get(1).unwrap().as_str();
            let field = caps.get(2).unwrap().as_str().to_string();
            let arg = caps.get(3).unwrap().as_str().to_string();
            if func.eq_ignore_ascii_case("match") {
                // MATCH：通用子串匹配（ILIKE，大小写不敏感）。`%`/`_`/`\` 一律按字面量
                // 转义（`*` 不是 LIKE 通配符，保持普通字符）。ILIKE rewrite 恒生成（真正的
                // 执行语义）；裁剪谓词仅在 term 与 TEXT 索引内容一致时才生成，否则跳过裁剪
                // 以免分词差异导致误裁漏数据。
                let like = escape_like_literal_text(&arg);
                if conjuncts.contains(&(field.clone(), arg.clone())) && can_prune_match_term(&arg) {
                    preds.push(MatchPredicate {
                        field: field.clone(),
                        term: arg.clone(),
                    });
                }
                format!("{field} ILIKE '%{like}%'")
            } else {
                // MATCH_TEXT：解析查询语法 → ILIKE 表达式树；裁剪谓词按 D6 的严格条件生成。
                let query = parse_match_text(&arg);
                if conjuncts.contains(&(field.clone(), arg.clone())) {
                    collect_match_text_prune_predicates(&query, &field, &mut preds);
                }
                rewrite_match_text(&query, &field)
            }
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
        Expr::Function(function)
            if function.name.to_string().eq_ignore_ascii_case("match")
                || function.name.to_string().eq_ignore_ascii_case("match_text") =>
        {
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

/// `MATCH_TEXT(field, 'query')` 查询语法（设计 D4，spec text-match-functions）解析后的 AST：
///
/// - 空格分隔的多个词 = token 级 AND（可分散出现）；
/// - `"..."` = 短语（引号内作为连续子串）；
/// - `*` = 通配符（前缀 / 后缀 / 包含 / 短语内），rewrite 时转为 ILIKE 的 `%`；
/// - `a OR b` = 或；
/// - `-a` = 排除（NOT）；
/// - `\` = 转义：`\*`、`\"`、`\\`（以及 `\%`、`\_`）表示字面量；
/// - 空 query 或全部为空 token = 恒 false（[`MatchTextQuery::AlwaysFalse`]）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchTextQuery {
    /// 空 query 或全部为空 token：恒不匹配任何行。
    AlwaysFalse,
    /// 空格分隔的 token 级合取（纯 AND 树）。
    Conjunction(Vec<MatchTextQuery>),
    /// `a OR b`。
    Or(Box<MatchTextQuery>, Box<MatchTextQuery>),
    /// `-a`：排除。
    Not(Box<MatchTextQuery>),
    /// 单个检索项（词或短语）。`like` 是已转义为 ILIKE pattern 的内容（不含外围 `%`，
    /// 短语时含内部空格）；`wildcard` 为 true 表示含未转义 `*`（已转成 `%`），此时该片段
    /// 本身就是完整 pattern（前缀 / 后缀 / 包含位置由用户指定），不能再加外围 `%`；
    /// `prune` = Some(小写化后的字面 token) 当且仅当无 `*` 通配符——只有这种叶子才可能
    /// 用于 tantivy 裁剪（D6）。
    Term {
        like: String,
        wildcard: bool,
        prune: Option<String>,
    },
}

/// 解析 `MATCH_TEXT` 查询。空串或全部为空 token → [`MatchTextQuery::AlwaysFalse`]。
pub fn parse_match_text(query: &str) -> MatchTextQuery {
    let items = tokenize_match_text(query);
    // 顶层按独立 `OR` 关键字分组（短语 / 转义内的 `OR` 不受影响）。
    let groups: Vec<&[MatchTextItem]> = items
        .split(|item| matches!(item, MatchTextItem::Or))
        .collect();
    let branches: Vec<MatchTextQuery> = groups
        .iter()
        .map(|group| build_conjunction(group))
        .collect();
    combine_or(branches)
}

/// 把解析出的 AST 展开为 DataFusion 可执行的 ILIKE 表达式树（D1：执行语义）。
/// `field` 为 SQL 中的字段标识符（原样保留）。空 / 全空 query 展开为 `FALSE`。
fn rewrite_match_text(query: &MatchTextQuery, field: &str) -> String {
    match query {
        MatchTextQuery::AlwaysFalse => "FALSE".to_string(),
        MatchTextQuery::Term { like, wildcard, .. } => {
            if *wildcard {
                // 用户显式给了 `*`（前缀/后缀/包含位置），该片段已是完整 pattern。
                format!("{field} ILIKE '{like}'")
            } else {
                // 普通词 / 短语：子串包含语义，包上外围 `%`。
                format!("{field} ILIKE '%{like}%'")
            }
        }
        MatchTextQuery::Conjunction(terms) => {
            let inner = terms
                .iter()
                .map(|term| rewrite_match_text(term, field))
                .collect::<Vec<_>>()
                .join(" AND ");
            format!("({inner})")
        }
        MatchTextQuery::Or(left, right) => format!(
            "({} OR {})",
            rewrite_match_text(left, field),
            rewrite_match_text(right, field)
        ),
        MatchTextQuery::Not(expr) => format!("NOT ({})", rewrite_match_text(expr, field)),
    }
}

/// 按设计 D6 收集 `MATCH_TEXT` 可裁剪谓词：仅当 query 解析为纯 AND 树、且每个叶子都是
/// 无 `*` 通配符的单 token（小写化后过 `can_prune_match_term`）时，才为每个叶子生成
/// [`MatchPredicate`]；短语 / 通配符 / OR / NOT 分支一律只执行不裁剪。
fn collect_match_text_prune_predicates(
    query: &MatchTextQuery,
    field: &str,
    out: &mut Vec<MatchPredicate>,
) {
    let mut tokens = Vec::new();
    if pure_and_prunable_tokens(query, &mut tokens) {
        for token in tokens {
            if can_prune_match_term(&token) {
                out.push(MatchPredicate {
                    field: field.to_string(),
                    term: token,
                });
            }
        }
    }
}

/// 返回 true 当且仅当 `query` 是纯 AND 树且每个叶子都是无 `*` 通配符的单 token；此时把各
/// 叶子的（已小写化）字面 token 收进 `tokens`，由调用方再过 `can_prune_match_term` 过滤。
fn pure_and_prunable_tokens(query: &MatchTextQuery, tokens: &mut Vec<String>) -> bool {
    match query {
        MatchTextQuery::AlwaysFalse => false,
        MatchTextQuery::Term {
            prune: Some(token), ..
        } => {
            tokens.push(token.clone());
            true
        }
        MatchTextQuery::Term { prune: None, .. } => false,
        MatchTextQuery::Conjunction(terms) => terms
            .iter()
            .all(|term| pure_and_prunable_tokens(term, tokens)),
        MatchTextQuery::Or(..) | MatchTextQuery::Not(..) => false,
    }
}

/// 提取 SQL 中所有 `MATCH_TEXT(field, 'query')` 调用的字段名。走 sqlparser AST 遍历
/// （`visit_expressions`），忽略注释 / 字符串字面量里的伪调用；解析失败返回空 vec。
pub fn match_text_fields(sql: &str) -> Vec<String> {
    let Ok(statements) = Parser::parse_sql(&GenericDialect, sql) else {
        return Vec::new();
    };
    let mut fields = Vec::new();
    let _ = visit_expressions(&statements, |expr| {
        if let Expr::Function(function) = expr
            && function.name.to_string().eq_ignore_ascii_case("match_text")
            && let Some((field, _)) = match_arguments(&function.args)
        {
            fields.push(field);
        }
        ControlFlow::<()>::Continue(())
    });
    fields
}

/// 把 MATCH 的 term 转为 ILIKE pattern 内容：`%`/`_`/`\` 一律按字面量转义（`*` 是普通字符，
/// 不是 LIKE 通配符，保持原样）。
fn escape_like_literal_text(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        out.push_str(&escape_like_literal(c));
    }
    out
}

/// 把单个字符转义为 ILIKE pattern 中的字面量。LIKE 的 `%` / `_` 通配符与 `\` 转义符
/// 前置 `\` 后按字面量匹配（arrow like 内核以 `\` 为默认转义，见
/// `arrow-string::like` 的 `like_escape` 测试）。
fn escape_like_literal(c: char) -> String {
    match c {
        '%' => "\\%".to_string(),
        '_' => "\\_".to_string(),
        '\\' => "\\\\".to_string(),
        other => other.to_string(),
    }
}

/// 词法元素：单个词 / 引号短语 / 独立 `OR` 关键字。`negated` 表示元素前有 `-` 前缀。
#[derive(Debug, Clone)]
enum MatchTextItem {
    Term {
        like: String,
        wildcard: bool,
        prune: Option<String>,
        negated: bool,
    },
    Phrase {
        like: String,
        wildcard: bool,
        negated: bool,
    },
    Or,
}

fn tokenize_match_text(query: &str) -> Vec<MatchTextItem> {
    let chars: Vec<char> = query.chars().collect();
    let mut items = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        // `-` 前缀 = 排除（NOT），作用于紧随其后的词 / 短语。
        let negated = if chars[i] == '-' {
            i += 1;
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            true
        } else {
            false
        };
        if i >= chars.len() {
            break; // 裸 `-`：没有可排除的目标，忽略。
        }
        if chars[i] == '"' {
            let (like, wildcard, next) = scan_phrase(&chars, i);
            items.push(MatchTextItem::Phrase {
                like,
                wildcard,
                negated,
            });
            i = next;
        } else if negated && is_or_keyword(&chars, i) {
            // `- OR ...`：`-` 退化为字面 token，`OR` 留待下一轮。
            items.push(MatchTextItem::Term {
                like: "-".to_string(),
                wildcard: false,
                prune: None,
                negated: false,
            });
        } else if is_or_keyword(&chars, i) {
            items.push(MatchTextItem::Or);
            i += 2;
        } else {
            let (like, wildcard, prune, next) = scan_token(&chars, i);
            items.push(MatchTextItem::Term {
                like,
                wildcard,
                prune,
                negated,
            });
            i = next;
        }
    }
    items
}

/// 独立 `OR` 关键字（大小写不敏感）：前后必须是词边界，避免把 `orange` 这类词误判。
fn is_or_keyword(chars: &[char], i: usize) -> bool {
    if i + 2 > chars.len() {
        return false;
    }
    if !(chars[i].eq_ignore_ascii_case(&'o') && chars[i + 1].eq_ignore_ascii_case(&'r')) {
        return false;
    }
    chars
        .get(i + 2)
        .is_none_or(|c| c.is_whitespace() || *c == '"' || *c == '-')
}

/// 扫描单个词到空白 / 引号：`*` 转 `%`（通配符），`\X` 转义字面量，`%`/`_` 字面量转义。
/// 返回 (ILIKE pattern 片段, 是否含 `*` 通配符, 可裁剪的小写 token 或 None, 下一个位置)。
fn scan_token(chars: &[char], start: usize) -> (String, bool, Option<String>, usize) {
    let mut i = start;
    let mut like = String::new();
    let mut literal = String::new();
    let mut wildcard = false;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() || c == '"' {
            break;
        }
        match c {
            '\\' => {
                i += 1;
                if let Some(next) = chars.get(i) {
                    like.push_str(&escape_like_literal(*next));
                    literal.push(*next);
                    i += 1;
                } else {
                    // 结尾裸 `\`：字面反斜杠。
                    like.push_str("\\\\");
                    literal.push('\\');
                }
            }
            '*' => {
                like.push('%');
                wildcard = true;
                i += 1;
            }
            '%' | '_' => {
                like.push_str(&escape_like_literal(c));
                literal.push(c);
                i += 1;
            }
            _ => {
                like.push(c);
                literal.push(c);
                i += 1;
            }
        }
    }
    let prune = if wildcard {
        None
    } else {
        Some(literal.to_lowercase())
    };
    (like, wildcard, prune, i)
}

/// 扫描引号短语：引号内文本作为连续子串（含空白与内部通配符），`\X` 转义字面量。
/// 未闭合的引号视作短语延伸到行末，保持可检索性。
/// 返回 (ILIKE pattern 片段, 是否含 `*` 通配符, 下一个扫描位置)。
fn scan_phrase(chars: &[char], start: usize) -> (String, bool, usize) {
    let mut i = start + 1;
    let mut like = String::new();
    let mut wildcard = false;
    while i < chars.len() {
        match chars[i] {
            '\\' => {
                i += 1;
                if let Some(next) = chars.get(i) {
                    like.push_str(&escape_like_literal(*next));
                    i += 1;
                } else {
                    like.push_str("\\\\");
                }
            }
            '"' => {
                i += 1;
                break;
            }
            '*' => {
                like.push('%');
                wildcard = true;
                i += 1;
            }
            '%' | '_' => {
                like.push_str(&escape_like_literal(chars[i]));
                i += 1;
            }
            c => {
                like.push(c);
                i += 1;
            }
        }
    }
    (like, wildcard, i)
}

/// 单个 OR 分组（一个合取段）→ 合取表达式；全空 → [`MatchTextQuery::AlwaysFalse`]。
fn build_conjunction(items: &[MatchTextItem]) -> MatchTextQuery {
    let terms: Vec<MatchTextQuery> = items
        .iter()
        .map(|item| match item {
            MatchTextItem::Term {
                like,
                wildcard,
                prune,
                negated,
            } => {
                let expr = MatchTextQuery::Term {
                    like: like.clone(),
                    wildcard: *wildcard,
                    prune: prune.clone(),
                };
                if *negated {
                    MatchTextQuery::Not(Box::new(expr))
                } else {
                    expr
                }
            }
            MatchTextItem::Phrase {
                like,
                wildcard,
                negated,
            } => {
                // 短语整串（含空格）与 TEXT 索引 token 形态不一致，永不生成裁剪谓词。
                let expr = MatchTextQuery::Term {
                    like: like.clone(),
                    wildcard: *wildcard,
                    prune: None,
                };
                if *negated {
                    MatchTextQuery::Not(Box::new(expr))
                } else {
                    expr
                }
            }
            MatchTextItem::Or => unreachable!("Or 已在顶层分组时消费"),
        })
        .collect();
    match terms.len() {
        0 => MatchTextQuery::AlwaysFalse,
        1 => terms.into_iter().next().expect("len == 1"),
        _ => MatchTextQuery::Conjunction(terms),
    }
}

/// 把多个 OR 分支结合为 Or 树；全部分支为空 → [`MatchTextQuery::AlwaysFalse`]。
fn combine_or(branches: Vec<MatchTextQuery>) -> MatchTextQuery {
    let mut rest: Vec<MatchTextQuery> = branches
        .into_iter()
        .filter(|branch| !matches!(branch, MatchTextQuery::AlwaysFalse))
        .collect();
    match rest.len() {
        0 => MatchTextQuery::AlwaysFalse,
        1 => rest.pop().expect("len == 1"),
        _ => {
            let mut iter = rest.into_iter();
            let mut acc = iter.next().expect("len >= 2");
            for next in iter {
                acc = MatchTextQuery::Or(Box::new(acc), Box::new(next));
            }
            acc
        }
    }
}

#[cfg(test)]
mod tests;
