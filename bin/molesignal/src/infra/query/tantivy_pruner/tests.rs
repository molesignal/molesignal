// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use object_store::{ObjectStore, ObjectStoreExt, PutPayload, local::LocalFileSystem};

use super::*;
use crate::{
    config::{CacheLayerSettings, TantivyResultCacheSettings},
    domain::stream::{
        FieldDef, FieldType, Retention, Schema, StreamDefinition, StreamIndexType, StreamType,
    },
    infra::{
        caching::TantivyResultCache, query::parser::like_atom,
        search::tantivy_index::TantivyArchiveBuilder,
    },
    shared::{ids::Id, time::TimestampMicros},
};

/// 每条谓词各是一个原子、全部用 `AND` 连起来的条件树。
fn conjunction(predicates: &[MatchPredicate]) -> (Vec<IndexedAtom>, Refutations) {
    let atoms = (0..predicates.len())
        .map(|atom| format!("p{atom}"))
        .collect::<Vec<_>>();

    let tree = WhereTree::parse(&format!("SELECT * FROM t WHERE {}", atoms.join(" AND ")));

    let indexed = predicates
        .iter()
        .enumerate()
        .map(|(atom, predicate)| IndexedAtom {
            atom,
            predicates: vec![predicate.clone()],
        })
        .collect();

    (indexed, Refutations::new(tree))
}

impl TantivyPruner {
    /// 每条谓词都要成立时留下的文件。
    async fn prune_with_index_keys(
        &self,
        candidates: Vec<QueryFile>,
        predicates: &[MatchPredicate],
        index_keys: &HashMap<String, String>,
    ) -> anyhow::Result<Vec<QueryFile>> {
        let (atoms, mut refutations) = conjunction(predicates);

        Ok(self
            .prune_counting(candidates, &atoms, &mut refutations, index_keys)
            .await?
            .files)
    }
}

/// `sql` 改写 `MATCH` 之后的语句，与条件树里每个 `LIKE` / `ILIKE` 原子的子串谓词（按原子的
/// 次序排开）。
fn substrings(sql: &str) -> (Vec<MatchPredicate>, String) {
    let rewritten = rewrite_match_calls(sql);

    let predicates = WhereTree::parse(&rewritten)
        .atoms()
        .filter_map(|(_, expr)| like_atom(expr))
        .flat_map(|set| match_sql::like_predicates(&set))
        .collect();

    (predicates, rewritten)
}

/// 条件树里文本含 `needle` 的那些原子的位。
fn bits(tree: &WhereTree, needle: &str) -> Refuted {
    tree.atoms()
        .filter(|(_, expr)| expr.to_string().contains(needle))
        .fold(0, |bits, (atom, _)| bits | atom_bit(atom))
}

fn logs_stream() -> StreamDefinition {
    StreamDefinition {
        id: Id::new(),
        org_id: Id::from_string("orga"),
        name: "logs".into(),
        stream_type: StreamType::LOGS,
        schema: Schema {
            fields: vec![FieldDef {
                name: "message".into(),
                data_type: FieldType::Utf8,
                nullable: false,
                index_type: None,
                indexed: true,
                encrypted: false,
                exact: false,
            }],
        },
        retention: Some(Retention { days: 7 }),
        created_at: TimestampMicros::now(),
        updated_at: TimestampMicros::now(),
    }
}

async fn seed_archive(store: &Arc<dyn ObjectStore>, index_object_key: &str, msgs: &[&str]) {
    let stream = logs_stream();
    let mut b = TantivyArchiveBuilder::try_new(&stream).unwrap().unwrap();

    for m in msgs {
        let mut v = std::collections::HashMap::new();
        v.insert("message", *m);
        b.add_doc(None, &v).unwrap();
    }

    let bytes = b.commit_and_archive().unwrap();

    store
        .put(
            &Path::from(index_object_key),
            PutPayload::from_bytes(bytes.into()),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn catalog_pruning_uses_only_explicit_artifact_relationships() {
    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
    let primary_key = "artifacts/v1/orga/dataset/p-0-00/segment/primary.parquet";
    let explicit_index_key = "artifacts/v1/orga/dataset/p-0-00/segment/index-artifact.ttv";
    seed_archive(&store, explicit_index_key, &["healthy request"]).await;

    let handle_cache: Arc<IndexHandleCache<Arc<IndexHandle>>> =
        Arc::new(IndexHandleCache::new(CacheLayerSettings::new(100, 60)));
    let pruner = TantivyPruner::new(handle_cache, store);
    let file = fm_with(primary_key).await;
    let predicates = vec![MatchPredicate::new(
        "message",
        "panic",
        TermMatch::Substring,
    )];

    let without_artifact = pruner
        .prune_with_index_keys(vec![file.clone()], &predicates, &HashMap::new())
        .await
        .unwrap();
    assert_eq!(
        without_artifact.len(),
        1,
        "missing Artifact must fall back to Parquet"
    );

    let indexes = HashMap::from([(primary_key.to_string(), explicit_index_key.to_string())]);
    let with_artifact = pruner
        .prune_with_index_keys(vec![file], &predicates, &indexes)
        .await
        .unwrap();
    assert!(
        with_artifact.is_empty(),
        "explicit index proves the file cannot match"
    );
}

/// 字段从全文改成精确后，改之前写下的归档里它仍是分词字段：整值谓词在那里答不出，文件留下。
#[tokio::test]
async fn a_value_predicate_keeps_files_whose_archive_split_the_field_into_words() {
    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
    let primary_key = "artifacts/v1/orga/dataset/p-0-00/segment/primary.parquet";
    let index_key = "artifacts/v1/orga/dataset/p-0-00/segment/index-artifact.ttv";
    seed_archive(&store, index_key, &["Checkout-API", "error"]).await;

    let handle_cache: Arc<IndexHandleCache<Arc<IndexHandle>>> =
        Arc::new(IndexHandleCache::new(CacheLayerSettings::new(100, 60)));
    let pruner = TantivyPruner::new(handle_cache, store);
    let file = fm_with(primary_key).await;

    for value in ["Checkout-API", "error", "ERROR", "absent"] {
        let predicates = vec![MatchPredicate::new("message", value, TermMatch::Value)];

        let kept = pruner
            .prune_with_index_keys(
                vec![file.clone()],
                &predicates,
                &explicit_indexes(primary_key, index_key),
            )
            .await
            .unwrap();

        assert_eq!(kept.len(), 1, "{value}");
    }
}

async fn fm_with(parquet_object_key: &str) -> QueryFile {
    QueryFile {
        id: Id::new(),
        org_id: Id::from_string("orga"),
        stream: "log_app".into(),
        stream_type: StreamType::LOGS,
        dataset_type: crate::domain::storage::primary_dataset_type(StreamType::LOGS).unwrap(),
        object_key: parquet_object_key.to_string(),
        checksum: None,
        etag: None,
        time_range: crate::shared::time::TimeRange::new(TimestampMicros(0), TimestampMicros(1)),
        rows: 0,
        size_bytes: 0,
        min_values: serde_json::Map::new(),
        max_values: serde_json::Map::new(),
        value_sets: serde_json::Map::new(),
        address_ranges: serde_json::Map::new(),
        null_counts: serde_json::Map::new(),
        indexed_at_micros: None,
        shard: None,
    }
}

fn explicit_indexes(parquet_object_key: &str, index_object_key: &str) -> HashMap<String, String> {
    HashMap::from([(parquet_object_key.to_owned(), index_object_key.to_owned())])
}

#[tokio::test]
async fn result_cache_hit_on_second_prune_increments_metric() {
    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
    let parquet_key = "primary/result-cache";
    let index_object_key = "indexes/result-cache";
    seed_archive(&store, index_object_key, &["panic 1", "panic 2", "ok"]).await;
    let indexes = explicit_indexes(parquet_key, index_object_key);

    let handle_cache: Arc<IndexHandleCache<Arc<IndexHandle>>> =
        Arc::new(IndexHandleCache::new(CacheLayerSettings::new(100, 60)));
    let result_cache = Arc::new(TantivyResultCache::new(&TantivyResultCacheSettings {
        capacity: 100,
        ttl_secs: 60,
    }));
    let pruner =
        TantivyPruner::new(handle_cache, store.clone()).with_result_cache(result_cache.clone());

    let preds = vec![MatchPredicate::new(
        "message",
        "panic",
        TermMatch::Substring,
    )];
    let fm = fm_with(parquet_key).await;
    let kept1 = pruner
        .prune_with_index_keys(vec![fm.clone()], &preds, &indexes)
        .await
        .unwrap();
    assert_eq!(kept1.len(), 1);
    let hits_before = crate::shared::metrics::gather_text().unwrap();
    let n_before = scrape_counter(&hits_before, "cache_tantivy_result_hits_total");

    let kept2 = pruner
        .prune_with_index_keys(vec![fm], &preds, &indexes)
        .await
        .unwrap();
    assert_eq!(kept2.len(), 1);
    let hits_after = crate::shared::metrics::gather_text().unwrap();
    let n_after = scrape_counter(&hits_after, "cache_tantivy_result_hits_total");
    assert!(
        n_after >= n_before + 1.0,
        "second prune must increment hits_total: before={n_before}, after={n_after}"
    );
}

#[tokio::test]
async fn all_eight_tantivy_cache_metrics_visible_after_prune() {
    use crate::{
        config::{TantivyFooterCacheSettings, TantivyResultCacheSettings},
        infra::caching::TantivyFooterCache,
    };

    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
    let parquet_key = "primary/all-metrics";
    let index_object_key = "indexes/all-metrics";
    seed_archive(&store, index_object_key, &["panic msg"]).await;
    let indexes = explicit_indexes(parquet_key, index_object_key);

    let handle_cache: Arc<IndexHandleCache<Arc<IndexHandle>>> =
        Arc::new(IndexHandleCache::new(CacheLayerSettings::new(100, 60)));
    let result_cache = Arc::new(TantivyResultCache::new(&TantivyResultCacheSettings {
        capacity: 100,
        ttl_secs: 60,
    }));
    let footer_cache = Arc::new(TantivyFooterCache::new(&TantivyFooterCacheSettings {
        capacity: 100,
        ttl_secs: 60,
    }));
    let pruner = TantivyPruner::new(handle_cache, store.clone())
        .with_result_cache(result_cache.clone())
        .with_footer_cache(footer_cache.clone());
    let preds = vec![MatchPredicate::new(
        "message",
        "panic",
        TermMatch::Substring,
    )];
    let fm = fm_with(parquet_key).await;
    // 两轮 prune：第一轮 miss → 写两层 cache；第二轮 result cache 命中。
    let _ = pruner
        .prune_with_index_keys(vec![fm.clone()], &preds, &indexes)
        .await
        .unwrap();
    let _ = pruner
        .prune_with_index_keys(vec![fm], &preds, &indexes)
        .await
        .unwrap();

    let text = crate::shared::metrics::gather_text().unwrap();
    // 8 个核心指标 + 2 个 errors_total 全部应当出现在 /metrics。
    for name in [
        "cache_tantivy_result_hits_total",
        "cache_tantivy_result_misses_total",
        "cache_tantivy_result_evictions_total",
        "cache_tantivy_result_hit_ratio",
        "cache_tantivy_result_errors_total",
        "cache_tantivy_footer_hits_total",
        "cache_tantivy_footer_misses_total",
        "cache_tantivy_footer_evictions_total",
        "cache_tantivy_footer_hit_ratio",
        "cache_tantivy_footer_errors_total",
    ] {
        assert!(text.contains(name), "metric {name} must register");
    }
    let result_hits = scrape_counter(&text, "cache_tantivy_result_hits_total");
    let result_ratio = scrape_counter(&text, "cache_tantivy_result_hit_ratio");
    assert!(
        result_hits >= 1.0,
        "expected result hits >= 1, got {result_hits}"
    );
    assert!(
        result_ratio > 0.0,
        "expected result hit_ratio > 0, got {result_ratio}"
    );
}

#[tokio::test]
async fn footer_cache_hit_short_circuits_object_store_get() {
    use crate::{config::TantivyFooterCacheSettings, infra::caching::TantivyFooterCache};

    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
    let parquet_key = "primary/footer-cache";
    let index_object_key = "indexes/footer-cache";
    seed_archive(&store, index_object_key, &["panic 1", "panic 2"]).await;
    let indexes = explicit_indexes(parquet_key, index_object_key);

    // 第一次 prune：fresh handle cache + 共享 footer cache → footer 写入 cache。
    let footer_cache = Arc::new(TantivyFooterCache::new(&TantivyFooterCacheSettings {
        capacity: 100,
        ttl_secs: 60,
    }));
    let handle_cache_1: Arc<IndexHandleCache<Arc<IndexHandle>>> =
        Arc::new(IndexHandleCache::new(CacheLayerSettings::new(100, 60)));
    let pruner1 =
        TantivyPruner::new(handle_cache_1, store.clone()).with_footer_cache(footer_cache.clone());
    let preds = vec![MatchPredicate::new(
        "message",
        "panic",
        TermMatch::Substring,
    )];
    let fm = fm_with(parquet_key).await;
    let k1 = pruner1
        .prune_with_index_keys(vec![fm.clone()], &preds, &indexes)
        .await
        .unwrap();
    assert_eq!(k1.len(), 1);

    // 删掉对象：模拟 IndexHandle 失效后的 worst case。
    store.delete(&Path::from(index_object_key)).await.unwrap();
    // 校验：对象确实没了，object_store GET 会 NotFound。
    let direct = store.get(&Path::from(index_object_key)).await;
    assert!(matches!(direct, Err(object_store::Error::NotFound { .. })));

    // 第二次 prune：fresh handle cache（IndexHandle 已 evict 等价），共享 footer cache。
    let handle_cache_2: Arc<IndexHandleCache<Arc<IndexHandle>>> =
        Arc::new(IndexHandleCache::new(CacheLayerSettings::new(100, 60)));
    let pruner2 =
        TantivyPruner::new(handle_cache_2, store.clone()).with_footer_cache(footer_cache.clone());
    let k2 = pruner2
        .prune_with_index_keys(vec![fm], &preds, &indexes)
        .await
        .unwrap();
    assert_eq!(
        k2.len(),
        1,
        "footer cache hit must let prune succeed even though object_store no longer has the archive"
    );
    assert!(
        footer_cache.hit_ratio() > 0.0,
        "footer cache must record at least one hit"
    );
}

#[tokio::test]
async fn result_cache_capacity_zero_falls_through_to_tantivy() {
    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
    let parquet_key = "primary/cache-disabled";
    let index_object_key = "indexes/cache-disabled";
    seed_archive(&store, index_object_key, &["panic only"]).await;
    let indexes = explicit_indexes(parquet_key, index_object_key);

    let handle_cache: Arc<IndexHandleCache<Arc<IndexHandle>>> =
        Arc::new(IndexHandleCache::new(CacheLayerSettings::new(100, 60)));
    let result_cache = Arc::new(TantivyResultCache::new(&TantivyResultCacheSettings {
        capacity: 0, // disabled
        ttl_secs: 60,
    }));
    let pruner =
        TantivyPruner::new(handle_cache, store.clone()).with_result_cache(result_cache.clone());

    let preds = vec![MatchPredicate::new(
        "message",
        "panic",
        TermMatch::Substring,
    )];
    let fm = fm_with(parquet_key).await;
    // 两次 prune 都应当正确：capacity=0 走 no-op cache，结果与不挂 cache 等价。
    let k1 = pruner
        .prune_with_index_keys(vec![fm.clone()], &preds, &indexes)
        .await
        .unwrap();
    let k2 = pruner
        .prune_with_index_keys(vec![fm], &preds, &indexes)
        .await
        .unwrap();
    assert_eq!(k1.len(), 1);
    assert_eq!(k2.len(), 1);
    // capacity=0 时 inner cache 是 None，get/insert 都直接早退；本测试覆盖到该路径
    // 不踩 panic 就证明 no-op 工作正常。hit_ratio 是进程级共享指标，受其它测试影响，
    // 不在此 assert。
}

fn scrape_counter(text: &str, name: &str) -> f64 {
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let rest = l.strip_prefix(name)?;
            let after = rest.chars().next()?;

            if after != ' ' && after != '{' {
                return None;
            }

            rest.split_whitespace().next_back()?.parse::<f64>().ok()
        })
        .next_back()
        .unwrap_or(0.0)
}

#[test]
fn extract_single_match() {
    let (preds, rewritten) = substrings("SELECT * FROM logs WHERE MATCH(message, 'panic')");
    assert_eq!(preds.len(), 1);
    assert_eq!(preds[0].field, "message");
    assert_eq!(preds[0].terms, ["panic"]);
    // MATCH 语义定稿为 ILIKE（大小写不敏感）。
    assert_eq!(
        rewritten,
        "SELECT * FROM logs WHERE message ILIKE '%panic%'"
    );
}

#[test]
fn extract_multiple_match_predicates() {
    let (preds, rewritten) =
        substrings("SELECT * FROM logs WHERE MATCH(message,'fatal') AND MATCH(level,'error')");
    assert_eq!(preds.len(), 2);
    assert!(rewritten.contains("message ILIKE '%fatal%'"));
    assert!(rewritten.contains("level ILIKE '%error%'"));
}

/// `MATCH` 在 OR 里照样改写；它的词证明不成立时，文件还可能经另一支命中。
#[test]
fn match_inside_or_is_rewritten_and_prunes_only_with_the_other_branch() {
    let (preds, rewritten) =
        substrings("SELECT * FROM logs WHERE MATCH(message, 'panic') OR level = 'error'");
    assert_eq!(preds.len(), 1);
    assert_eq!(
        rewritten,
        "SELECT * FROM logs WHERE message ILIKE '%panic%' OR level = 'error'"
    );

    let tree = WhereTree::parse(&rewritten);
    assert!(tree.may_hold(bits(&tree, "panic")));
    assert!(!tree.may_hold(bits(&tree, "panic") | bits(&tree, "level")));
}

/// The explorers' words of an OR stand beside words every row holds: a word
/// every row holds prunes alone, the words of an OR only together with every
/// other branch of it.
#[test]
fn the_words_of_an_or_prune_only_together_with_its_other_branches() {
    let (preds, rewritten) = substrings(
        "SELECT * FROM logs WHERE CAST(_timestamp AS BIGINT) >= 1 AND MATCH(message, 'upstream') \
         AND (MATCH(message, 'timeout') OR \"message\" LIKE '%connection reset%' OR \
         (\"level\" = 'error' AND MATCH(message, 'disk')))",
    );
    assert_eq!(
        preds
            .iter()
            .map(|pred| pred.terms.join("|"))
            .collect::<Vec<_>>(),
        ["upstream", "timeout", "connection reset", "disk"]
    );
    assert!(rewritten.contains("message ILIKE '%timeout%'"));
    assert!(rewritten.contains("message ILIKE '%disk%'"));

    let tree = WhereTree::parse(&rewritten);
    let (timeout, reset, disk) = (
        bits(&tree, "timeout"),
        bits(&tree, "connection reset"),
        bits(&tree, "disk"),
    );

    assert!(!tree.may_hold(bits(&tree, "upstream")));
    assert!(tree.may_hold(timeout | reset));
    assert!(!tree.may_hold(timeout | reset | disk));
}

/// `CIDR_MATCH` 的名字以 MATCH 结尾，但它不是 `MATCH` 调用：改写会把它变成不存在的列。
#[test]
fn a_function_whose_name_ends_in_match_is_left_alone() {
    let sql = "SELECT * FROM logs WHERE CIDR_MATCH(client_ip, '10.0.0.0/8') AND MATCH(msg, 'x')";
    let (preds, rewritten) = substrings(sql);

    assert_eq!(
        rewritten,
        "SELECT * FROM logs WHERE CIDR_MATCH(client_ip, '10.0.0.0/8') AND msg ILIKE '%x%'"
    );
    assert_eq!(
        preds
            .iter()
            .map(|pred| pred.field.as_str())
            .collect::<Vec<_>>(),
        ["msg"]
    );
}

#[test]
fn no_match_predicates() {
    let (preds, rewritten) = substrings("SELECT count(*) FROM logs WHERE level = 'info'");
    assert!(preds.is_empty());
    assert_eq!(rewritten, "SELECT count(*) FROM logs WHERE level = 'info'");
}

/// MATCH 按 `ILIKE` 执行，不分大小写：大写的 term 小写后用于裁剪，改写保留原样。含标点的
/// term 原样交给索引，由它按段判断（`my-api` 的 `my` 与 `api` 各自要出现）；超长的 term 落在
/// 长词里，由长词的三元组判断。只有标点的 term 没有能查的部分，不生成谓词。
#[test]
fn match_terms_prune_lower_cased_with_their_punctuation() {
    let (preds, rewritten) = substrings("SELECT * FROM logs WHERE MATCH(message, 'FAILED')");
    assert_eq!(preds.len(), 1);
    assert_eq!(preds[0].terms, ["failed"]);
    assert_eq!(preds[0].matching, TermMatch::Substring);
    assert_eq!(
        rewritten, "SELECT * FROM logs WHERE message ILIKE '%FAILED%'",
        "ILIKE rewrite 必须保留原样（大小写不变），由 DataFusion 兜底执行"
    );

    let (preds, _) = substrings("SELECT * FROM logs WHERE MATCH(path, 'My-API')");
    assert_eq!(preds[0].terms, ["my-api"]);

    let long = "a".repeat(41);
    let (preds, _) = substrings(&format!(
        "SELECT * FROM logs WHERE MATCH(message, '{long}')"
    ));
    assert_eq!(preds[0].terms, [long]);

    let (preds, _) = substrings("SELECT * FROM logs WHERE MATCH(message, '--')");
    assert!(preds.is_empty(), "只有标点的 term 没有能查的部分");
}

/// 不带引号的字段名按 DataFusion 解析成小写：`MATCH(Message, …)` 过滤的是 `message` 列。
#[test]
fn match_fields_are_named_the_way_datafusion_resolves_them() {
    let (preds, rewritten) = substrings("SELECT * FROM logs WHERE MATCH(Message, 'panic')");
    assert_eq!(preds.len(), 1);
    assert_eq!(preds[0].field, "message");
    assert_eq!(
        rewritten,
        "SELECT * FROM logs WHERE Message ILIKE '%panic%'"
    );

    assert_eq!(
        match_text_fields("SELECT * FROM logs WHERE MATCH_TEXT(Message, 'panic')"),
        vec!["message".to_string()]
    );
}

/// 全小写字母数字、不超长的 term 与 TEXT 索引内容一致，仍照常用于裁剪（修复不能误伤
/// 正常场景 —— 这是绝大多数裁剪收益的来源）。
#[test]
fn plain_lowercase_term_still_prunes() {
    let (preds, _) = substrings("SELECT * FROM logs WHERE MATCH(message, 'panic')");
    assert_eq!(preds.len(), 1);
    assert_eq!(preds[0].terms, ["panic"]);

    let (preds, _) = substrings("SELECT * FROM logs WHERE MATCH(trace, 'a1b2c3d4')");
    assert_eq!(preds.len(), 1, "hex 串（trace_id 形态）应仍可裁剪");
}

// ===== MATCH_TEXT 查询语法解析（D4）与 ILIKE rewrite =====

#[test]
fn match_text_token_and_rewrites_to_parenthesized_iliike() {
    let (preds, rewritten) =
        substrings("SELECT * FROM logs WHERE MATCH_TEXT(message, 'panic disk')");
    assert_eq!(
        rewritten,
        "SELECT * FROM logs WHERE (message ILIKE '%panic%' AND message ILIKE '%disk%')"
    );
    // 每个合取的词各生成一个裁剪谓词。
    assert_eq!(preds.len(), 2);
    assert_eq!(preds[0].terms, ["panic"]);
    assert_eq!(preds[1].terms, ["disk"]);
}

#[test]
fn match_text_phrase_is_contiguous_substring() {
    let (_, rewritten) =
        substrings("SELECT * FROM logs WHERE MATCH_TEXT(message, '\"disk full\"')");
    assert_eq!(
        rewritten,
        "SELECT * FROM logs WHERE message ILIKE '%disk full%'"
    );
}

#[test]
fn match_text_mixed_token_and_phrase() {
    let (_, rewritten) =
        substrings("SELECT * FROM logs WHERE MATCH_TEXT(message, 'panic \"disk full\"')");
    assert_eq!(
        rewritten,
        "SELECT * FROM logs WHERE (message ILIKE '%panic%' AND message ILIKE '%disk full%')"
    );
}

#[test]
fn match_text_or_rewrites_to_or_expression() {
    let (preds, rewritten) =
        substrings("SELECT * FROM logs WHERE MATCH_TEXT(message, 'panic OR timeout')");
    assert_eq!(
        rewritten,
        "SELECT * FROM logs WHERE (message ILIKE '%panic%' OR message ILIKE '%timeout%')"
    );
    assert_eq!(
        preds
            .iter()
            .map(|pred| pred.terms.join("|"))
            .collect::<Vec<_>>(),
        ["panic", "timeout"],
        "OR 的每一支各是一个原子"
    );
}

#[test]
fn match_text_not_rewrites_to_not_expression() {
    let (preds, rewritten) =
        substrings("SELECT * FROM logs WHERE MATCH_TEXT(message, 'panic -debug')");
    assert_eq!(
        rewritten,
        "SELECT * FROM logs WHERE (message ILIKE '%panic%' AND NOT (message ILIKE '%debug%'))"
    );
    assert_eq!(preds.len(), 1, "NOT 的词不裁剪，其余合取的词照常");
    assert_eq!(preds[0].terms, ["panic"]);
}

#[test]
fn match_text_wildcard_prefix_suffix_contains() {
    let (_, rewritten) = substrings("SELECT * FROM logs WHERE MATCH_TEXT(message, 'error*')");
    assert_eq!(rewritten, "SELECT * FROM logs WHERE message ILIKE 'error%'");

    let (_, rewritten) = substrings("SELECT * FROM logs WHERE MATCH_TEXT(message, '*error')");
    assert_eq!(rewritten, "SELECT * FROM logs WHERE message ILIKE '%error'");

    let (_, rewritten) = substrings("SELECT * FROM logs WHERE MATCH_TEXT(message, '*error*')");
    assert_eq!(
        rewritten,
        "SELECT * FROM logs WHERE message ILIKE '%error%'"
    );
}

#[test]
fn match_text_wildcard_inside_phrase() {
    let (_, rewritten) = substrings("SELECT * FROM logs WHERE MATCH_TEXT(message, '\"api v*\"')");
    assert_eq!(rewritten, "SELECT * FROM logs WHERE message ILIKE 'api v%'");
}

#[test]
fn match_text_escaped_asterisk_is_literal() {
    // `\*` → 字面星号；LIKE 里 `*` 不是通配符，保持原样即可。
    let (_, rewritten) = substrings("SELECT * FROM logs WHERE MATCH_TEXT(message, '100\\*')");
    assert_eq!(rewritten, "SELECT * FROM logs WHERE message ILIKE '%100*%'");
}

#[test]
fn match_text_percent_is_literal_not_wildcard() {
    // `100%`：裸 `%` 按字面量转义（spec：`usage 100%` 命中、`usage 1000` 不命中）。
    let (_, rewritten) = substrings("SELECT * FROM logs WHERE MATCH_TEXT(message, '100%')");
    assert_eq!(
        rewritten,
        "SELECT * FROM logs WHERE message ILIKE '%100\\%%'"
    );

    // `100\%`（显式转义）与裸 `%` 行为一致。
    let (_, rewritten) = substrings("SELECT * FROM logs WHERE MATCH_TEXT(message, '100\\%')");
    assert_eq!(
        rewritten,
        "SELECT * FROM logs WHERE message ILIKE '%100\\%%'"
    );
}

#[test]
fn match_text_underscore_is_literal_not_wildcard() {
    let (_, rewritten) = substrings("SELECT * FROM logs WHERE MATCH_TEXT(message, 'a_b')");
    assert_eq!(
        rewritten,
        "SELECT * FROM logs WHERE message ILIKE '%a\\_b%'"
    );
}

#[test]
fn match_text_empty_query_is_always_false() {
    let (preds, rewritten) = substrings("SELECT * FROM logs WHERE MATCH_TEXT(message, '')");
    assert_eq!(rewritten, "SELECT * FROM logs WHERE FALSE");
    assert!(preds.is_empty());

    let (_, rewritten) = substrings("SELECT * FROM logs WHERE MATCH_TEXT(message, '   ')");
    assert_eq!(rewritten, "SELECT * FROM logs WHERE FALSE");
}

#[test]
fn match_text_case_insensitive_rewrite_preserves_case() {
    // 大小写不敏感由 ILIKE 保证；rewrite 保留原样交给执行层。
    let (preds, rewritten) = substrings("SELECT * FROM logs WHERE MATCH_TEXT(message, 'FAILED')");
    assert_eq!(
        rewritten,
        "SELECT * FROM logs WHERE message ILIKE '%FAILED%'"
    );
    // 但裁剪谓词先小写化（D7）：`FAILED` 小写化后与 TEXT 索引内容一致，可裁剪。
    assert_eq!(preds.len(), 1);
    assert_eq!(preds[0].terms, ["failed"]);
}

#[test]
fn match_text_lowercase_call_is_accepted() {
    let (_, rewritten) = substrings("SELECT * FROM logs WHERE match_text(message, 'panic')");
    assert_eq!(
        rewritten,
        "SELECT * FROM logs WHERE message ILIKE '%panic%'"
    );
}

#[test]
fn match_text_single_token_equivalent_to_match() {
    // 单 token 的 MATCH_TEXT 退化为 MATCH 语义（spec text-match-functions）。
    let (_, match_text) = substrings("SELECT * FROM logs WHERE MATCH_TEXT(message, 'failed')");
    let (_, plain_match) = substrings("SELECT * FROM logs WHERE MATCH(message, 'failed')");
    assert_eq!(match_text, plain_match);
}

#[test]
fn match_text_or_keyword_does_not_swallow_plain_words() {
    // `orange` 以 `or` 开头但不是关键字；`a_or_b` 同理。
    let (_, rewritten) = substrings("SELECT * FROM logs WHERE MATCH_TEXT(message, 'orange')");
    assert_eq!(
        rewritten,
        "SELECT * FROM logs WHERE message ILIKE '%orange%'"
    );

    let (_, rewritten) = substrings("SELECT * FROM logs WHERE MATCH_TEXT(message, 'a_or_b')");
    assert_eq!(
        rewritten,
        "SELECT * FROM logs WHERE message ILIKE '%a\\_or\\_b%'"
    );
}

// ===== MATCH_TEXT 裁剪谓词边界（D6） =====

#[test]
fn match_text_pure_and_single_token_prunes() {
    let (preds, _) = substrings(
        "SELECT * FROM logs WHERE MATCH_TEXT(message, 'failed') AND _timestamp > '2026-01-01'",
    );
    assert_eq!(preds.len(), 1);
    assert_eq!(preds[0].field, "message");
    assert_eq!(preds[0].terms, ["failed"]);
}

/// 短语整个是一个子串，索引按段判断它的每个词；通配符之间的每段字面量各自要出现。
#[test]
fn match_text_phrases_and_wildcards_prune_by_their_literals() {
    let (preds, _) = substrings("SELECT * FROM logs WHERE MATCH_TEXT(message, '\"disk full\"')");
    assert_eq!(preds[0].terms, ["disk full"]);

    let (preds, _) = substrings("SELECT * FROM logs WHERE MATCH_TEXT(message, 'error*code')");
    assert_eq!(
        preds
            .iter()
            .map(|pred| pred.terms.join("|"))
            .collect::<Vec<_>>(),
        ["error", "code"]
    );

    let (preds, _) =
        substrings("SELECT * FROM logs WHERE MATCH_TEXT(message, 'panic \"disk full\"')");
    assert_eq!(preds.len(), 2);
}

/// 语句本身写的 `LIKE` / `ILIKE`，日志筛选器的「包含」，与 `MATCH` 一样裁剪。
#[test]
fn like_conditions_of_the_statement_prune_too() {
    let (preds, _) = substrings(
        "SELECT * FROM logs WHERE CAST(\"message\" AS VARCHAR) LIKE '%Timeout%' \
         AND host ILIKE 'api-%'",
    );
    assert_eq!(
        preds
            .iter()
            .map(|pred| (pred.field.as_str(), pred.terms.join("|")))
            .collect::<Vec<_>>(),
        [
            ("message", "timeout".to_string()),
            ("host", "api-".to_string())
        ]
    );
}

#[test]
fn match_text_inside_an_or_prunes_only_with_every_branch() {
    let rewritten = rewrite_match_calls(
        "SELECT * FROM logs WHERE service = 'api' AND (MATCH_TEXT(message, 'panic') OR level = 'info')",
    );
    let tree = WhereTree::parse(&rewritten);

    assert!(tree.may_hold(bits(&tree, "panic")), "OR 的另一支还可能成立");
    assert!(!tree.may_hold(bits(&tree, "panic") | bits(&tree, "level")));
    assert!(!tree.may_hold(bits(&tree, "service")));
}

#[test]
fn match_text_multiple_top_level_conjuncts_all_prune() {
    let (preds, _) = substrings(
        "SELECT * FROM logs WHERE MATCH_TEXT(message, 'panic') AND MATCH_TEXT(level, 'info')",
    );
    assert_eq!(preds.len(), 2);
    assert_eq!(preds[0].terms, ["panic"]);
    assert_eq!(preds[1].terms, ["info"]);
}

// ===== 与 TEXT 索引 token 形态一致性（防止重开分词不匹配漏数据缺口） =====

#[test]
fn match_text_prune_token_must_match_indexed_token_shape() {
    // `cats` 与 `cat` 都是 SimpleTokenizer 的合法 token，可裁剪。
    let (preds, _) = substrings("SELECT * FROM logs WHERE MATCH_TEXT(message, 'cats')");
    assert_eq!(preds.len(), 1);
    let (preds, _) = substrings("SELECT * FROM logs WHERE MATCH_TEXT(message, 'cat')");
    assert_eq!(preds.len(), 1);

    // `my-api` 在索引里被切成 `my`/`api`：整串交给索引按段判断，不用 count_term。
    let (preds, _) = substrings("SELECT * FROM logs WHERE MATCH_TEXT(path, 'my-api')");
    assert_eq!(preds[0].terms, ["my-api"]);
    assert_eq!(preds[0].matching, TermMatch::Substring);
}

// ===== MATCH_TEXT 字段提取（门槛校验用） =====

#[test]
fn match_text_fields_extracts_field_names_only_from_real_calls() {
    assert_eq!(
        match_text_fields("SELECT * FROM logs WHERE MATCH_TEXT(message, 'panic')"),
        vec!["message"]
    );
    assert_eq!(
        match_text_fields("SELECT * FROM logs WHERE MATCH_TEXT(a, 'x') OR MATCH_TEXT(b, 'y')"),
        vec!["a", "b"]
    );
    // MATCH 不是 MATCH_TEXT，不提取。
    assert_eq!(
        match_text_fields("SELECT * FROM logs WHERE MATCH(message, 'panic')"),
        Vec::<String>::new()
    );
    // 注释 / 字符串字面量里的伪调用不提取（AST 遍历）。
    assert_eq!(
        match_text_fields("SELECT * FROM logs -- MATCH_TEXT(foo, 'x')\nWHERE level = 'info'"),
        Vec::<String>::new()
    );
    assert_eq!(
        match_text_fields("SELECT 'MATCH_TEXT(foo, x)'"),
        Vec::<String>::new()
    );
    // 解析失败 → 空。
    assert_eq!(match_text_fields("SELECT * FROM"), Vec::<String>::new());
}

async fn prune_one(messages: &[&str], term: &str) -> usize {
    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
    let (parquet_key, index_object_key) = ("primary/substring", "indexes/substring");
    seed_archive(&store, index_object_key, messages).await;

    let handle_cache: Arc<IndexHandleCache<Arc<IndexHandle>>> =
        Arc::new(IndexHandleCache::new(CacheLayerSettings::new(100, 60)));
    let pruner = TantivyPruner::new(handle_cache, store);

    let predicates = vec![MatchPredicate::new("message", term, TermMatch::Substring)];

    pruner
        .prune_with_index_keys(
            vec![fm_with(parquet_key).await],
            &predicates,
            &explicit_indexes(parquet_key, index_object_key),
        )
        .await
        .unwrap()
        .len()
}

/// MATCH 的词按子串执行：词典里有包含它的词（`panicked` 含 `panic`）就保留文件；词典里没有
/// 的时候，被分词丢掉的长词（这里是 64 位的十六进制摘要）也可能含有它；两处都没有才剔除。
#[tokio::test]
async fn substring_terms_prune_only_files_that_cannot_hold_them() {
    const DIGEST: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
    let line = format!("upload sha256={DIGEST}");

    assert_eq!(prune_one(&["worker panicked at main.rs"], "panic").await, 1);
    assert_eq!(
        prune_one(&["worker panicked at main.rs"], "panics").await,
        0
    );
    assert_eq!(prune_one(&[line.as_str()], "0c55ad").await, 1);
    assert_eq!(prune_one(&[line.as_str()], "cafe").await, 0);
    assert_eq!(prune_one(&[line.as_str()], "upload").await, 1);
    assert_eq!(prune_one(&[line.as_str()], "upload sha256=").await, 1);
    assert_eq!(prune_one(&[line.as_str()], "download sha256=").await, 0);
    assert_eq!(prune_one(&["用户登录失败"], "登录失败").await, 1);
    assert_eq!(prune_one(&["用户登录失败"], "注册失败").await, 0);
}

/// `IN` 列表与同一字段上的 `OR`：任一个词可能出现就保留，全都一定没有才剔除；整值谓词的行数是
/// 各个值的行数之和。
#[tokio::test]
async fn any_term_of_a_predicate_keeps_the_file() {
    assert_eq!(
        prune_terms(&["worker panicked"], &["timeout", "panic"]).await,
        1
    );
    assert_eq!(
        prune_terms(&["worker panicked"], &["timeout", "refused"]).await,
        0
    );
}

async fn prune_terms(messages: &[&str], terms: &[&str]) -> usize {
    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
    let (parquet_key, index_object_key) = ("primary/terms", "indexes/terms");
    seed_archive(&store, index_object_key, messages).await;

    let handle_cache: Arc<IndexHandleCache<Arc<IndexHandle>>> =
        Arc::new(IndexHandleCache::new(CacheLayerSettings::new(100, 60)));
    let pruner = TantivyPruner::new(handle_cache, store);

    let predicates = vec![MatchPredicate {
        field: "message".into(),
        terms: terms.iter().map(|term| (*term).to_owned()).collect(),
        matching: TermMatch::Substring,
    }];

    pruner
        .prune_with_index_keys(
            vec![fm_with(parquet_key).await],
            &predicates,
            &explicit_indexes(parquet_key, index_object_key),
        )
        .await
        .unwrap()
        .len()
}

/// 整值谓词数出的命中数随留下的文件给出：按时间取一页的查询据此决定先读哪些文件。
#[tokio::test]
async fn value_predicates_count_the_rows_a_kept_file_can_give() {
    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
    let (parquet_key, index_object_key) = ("primary/value", "indexes/value");

    let mut stream = logs_stream();
    stream.schema.fields[0].exact = true;
    let mut builder = TantivyArchiveBuilder::try_new(&stream).unwrap().unwrap();

    for message in ["a", "a", "b"] {
        let values = std::collections::HashMap::from([("message", message)]);
        builder.add_doc(None, &values).unwrap();
    }

    store
        .put(
            &Path::from(index_object_key),
            PutPayload::from_bytes(builder.commit_and_archive().unwrap().into()),
        )
        .await
        .unwrap();

    let handle_cache: Arc<IndexHandleCache<Arc<IndexHandle>>> =
        Arc::new(IndexHandleCache::new(CacheLayerSettings::new(100, 60)));
    let pruner = TantivyPruner::new(handle_cache, store);

    let count = |sql: &str| {
        let tree = WhereTree::parse(sql);
        let atoms = indexed_atoms(&tree, &stream);
        (atoms, Refutations::new(tree))
    };

    let (atoms, mut refutations) = count("SELECT * FROM t WHERE message = 'a'");

    let pruned = pruner
        .prune_counting(
            vec![fm_with(parquet_key).await],
            &atoms,
            &mut refutations,
            &explicit_indexes(parquet_key, index_object_key),
        )
        .await
        .unwrap();

    assert_eq!(pruned.files.len(), 1);
    assert_eq!(pruned.rows.get(parquet_key), Some(&2));

    for (sql, rows, why) in [
        (
            "SELECT * FROM t WHERE message IN ('a', 'b', 'c')",
            3,
            "an IN list sums its values' rows",
        ),
        (
            "SELECT * FROM t WHERE message = 'b' OR message = 'c'",
            1,
            "an OR sums its branches' rows",
        ),
        (
            "SELECT * FROM t WHERE message = 'a' AND (message = 'b' OR other = 'x')",
            2,
            "an AND takes its least bound, a branch without one gives the OR none",
        ),
    ] {
        let (atoms, mut refutations) = count(sql);

        let pruned = pruner
            .prune_counting(
                vec![fm_with(parquet_key).await],
                &atoms,
                &mut refutations,
                &explicit_indexes(parquet_key, index_object_key),
            )
            .await
            .unwrap();

        assert_eq!(pruned.rows.get(parquet_key), Some(&rows), "{why}");
    }

    let (atoms, mut refutations) = count("SELECT * FROM t WHERE message = 'c' OR message = 'd'");

    let pruned = pruner
        .prune_counting(
            vec![fm_with(parquet_key).await],
            &atoms,
            &mut refutations,
            &explicit_indexes(parquet_key, index_object_key),
        )
        .await
        .unwrap();

    assert!(pruned.files.is_empty(), "every branch of the OR is refuted");
}

/// 整值字段上的 `LIKE` / `ILIKE` 扫词典：没有哪个值含这段文字，文件就剔除。不计大小写时只比较
/// ASCII 字母，字面量有 `k`、`s` 时答不出（开尔文符号、长 s 不计大小写时等于它们）。
#[tokio::test]
async fn like_on_an_exact_field_reads_the_dictionary() {
    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
    let (parquet_key, index_object_key) = ("primary/like", "indexes/like");

    let mut stream = logs_stream();
    stream.schema.fields[0].exact = true;
    let mut builder = TantivyArchiveBuilder::try_new(&stream).unwrap().unwrap();

    for message in ["checkout-api", "Payments"] {
        let values = std::collections::HashMap::from([("message", message)]);
        builder.add_doc(None, &values).unwrap();
    }

    store
        .put(
            &Path::from(index_object_key),
            PutPayload::from_bytes(builder.commit_and_archive().unwrap().into()),
        )
        .await
        .unwrap();

    let handle_cache: Arc<IndexHandleCache<Arc<IndexHandle>>> =
        Arc::new(IndexHandleCache::new(CacheLayerSettings::new(100, 60)));
    let pruner = TantivyPruner::new(handle_cache, store);

    for (condition, kept) in [
        ("message LIKE '%out-a%'", 1),
        ("message LIKE '%OUT%'", 0),
        ("message LIKE 'zzz%'", 0),
        ("message ILIKE '%PAYMENT%'", 1),
        ("message ILIKE '%order%'", 0),
        ("message ILIKE '%status%'", 1),
        ("message LIKE '%zzz%' OR message = 'Payments'", 1),
    ] {
        let tree = WhereTree::parse(&format!("SELECT * FROM t WHERE {condition}"));
        let atoms = indexed_atoms(&tree, &stream);
        let mut refutations = Refutations::new(tree);

        let pruned = pruner
            .prune_counting(
                vec![fm_with(parquet_key).await],
                &atoms,
                &mut refutations,
                &explicit_indexes(parquet_key, index_object_key),
            )
            .await
            .unwrap();

        assert_eq!(pruned.files.len(), kept, "{condition}");
    }
}

/// JSON 字段的键值索引：`json_text(字段, 'key')` 的 `=`、`IN`、`IS NOT NULL` 按键值词判断，命中数
/// 是行数。值按 `json_text` 的规则写成文本：数字与布尔是它们的 JSON，`null` 没有值。
#[tokio::test]
async fn json_members_prune_and_count_through_the_key_value_index() {
    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
    let (parquet_key, index_object_key) = ("primary/members", "indexes/members");

    let mut stream = logs_stream();
    stream.schema.fields[0].name = "context".into();
    stream.schema.fields[0].data_type = FieldType::Json;
    stream.schema.fields[0].configure_index(true, StreamIndexType::KeyValue);

    let mut builder = TantivyArchiveBuilder::try_new(&stream).unwrap().unwrap();

    for context in [
        r#"{"plan": "pro", "seats": 5, "beta": true}"#,
        r#"{"plan": "free", "beta": null}"#,
        r#"{"plan": "pro"}"#,
        "masked",
    ] {
        let values = std::collections::HashMap::from([("context", context)]);
        builder.add_doc(None, &values).unwrap();
    }

    store
        .put(
            &Path::from(index_object_key),
            PutPayload::from_bytes(builder.commit_and_archive().unwrap().into()),
        )
        .await
        .unwrap();

    let handle_cache: Arc<IndexHandleCache<Arc<IndexHandle>>> =
        Arc::new(IndexHandleCache::new(CacheLayerSettings::new(100, 60)));
    let pruner = TantivyPruner::new(handle_cache, store);

    for (condition, rows) in [
        ("json_text(context, 'plan') = 'pro'", Some(2)),
        (
            "json_text(\"context\", 'plan') IN ('free', 'team')",
            Some(1),
        ),
        ("json_text(context, 'seats') = '5'", Some(1)),
        ("json_text(context, 'beta') = 'true'", Some(1)),
        ("json_text(context, 'beta') IS NOT NULL", Some(1)),
        (
            "NULLIF(json_text(context, 'plan'), '') IS NOT NULL",
            Some(3),
        ),
        ("json_text(context, 'plan') = 'team'", None),
        ("json_text(context, 'region') IS NOT NULL", None),
    ] {
        let tree = WhereTree::parse(&format!("SELECT * FROM t WHERE {condition}"));
        let atoms = indexed_atoms(&tree, &stream);
        assert_eq!(atoms.len(), 1, "{condition}");

        let mut refutations = Refutations::new(tree);

        let pruned = pruner
            .prune_counting(
                vec![fm_with(parquet_key).await],
                &atoms,
                &mut refutations,
                &explicit_indexes(parquet_key, index_object_key),
            )
            .await
            .unwrap();

        assert_eq!(
            pruned.files.len(),
            usize::from(rows.is_some()),
            "{condition}"
        );
        assert_eq!(pruned.rows.get(parquet_key).copied(), rows, "{condition}");
    }
}
