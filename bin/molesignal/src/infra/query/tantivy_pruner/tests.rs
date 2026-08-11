// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use object_store::{ObjectStore, PutPayload, local::LocalFileSystem};

use super::*;
use crate::{
    config::{CacheLayerSettings, TantivyResultCacheSettings},
    domain::stream::{FieldDef, FieldType, Retention, Schema, StreamDefinition, StreamType},
    infra::{caching::TantivyResultCache, search::tantivy_index::TantivyArchiveBuilder},
    shared::{ids::Id, time::TimestampMicros},
};

fn logs_stream() -> StreamDefinition {
    StreamDefinition {
        id: Id::new(),
        org_id: Id::from_string("orga"),
        name: "logs".into(),
        stream_type: StreamType::Logs,
        schema: Schema {
            fields: vec![FieldDef {
                name: "message".into(),
                data_type: FieldType::Utf8,
                nullable: false,
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
        b.add_doc(&v).unwrap();
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

async fn fm_with(parquet_object_key: &str) -> ParquetFileMeta {
    ParquetFileMeta {
        id: Id::new(),
        org_id: Id::from_string("orga"),
        stream: "log_app".into(),
        stream_type: StreamType::Logs,
        dataset_kind: crate::domain::storage::PhysicalDatasetKind::Raw,
        // Parquet object_key 必须是带 dataset/hour 的规范物理路径，才能由
        // `key_for` 唯一映射到同一文件的 `.ttv` sidecar。
        object_key: parquet_object_key.to_string(),
        time_range: crate::shared::time::TimeRange::new(TimestampMicros(0), TimestampMicros(1)),
        rows: 0,
        size_bytes: 0,
        min_values: serde_json::Map::new(),
        max_values: serde_json::Map::new(),
        deleted: false,
    }
}

/// 给定一个 parquet object_key 计算其 .ttv sidecar 的 storage key（用于 seed）。
fn sidecar_key(parquet_object_key: &str) -> String {
    TantivyArchive::key_for(parquet_object_key).expect("canonical parquet key")
}

#[tokio::test]
async fn result_cache_hit_on_second_prune_increments_metric() {
    let tmp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(tmp.path()).unwrap());
    let parquet_key = "orgA/logs/raw/log_app/2026/01/15/09/obj-a.parquet";
    let index_object_key = sidecar_key(parquet_key);
    seed_archive(&store, &index_object_key, &["panic 1", "panic 2", "ok"]).await;

    let handle_cache: Arc<ParquetMetaCache<Arc<IndexHandle>>> =
        Arc::new(ParquetMetaCache::new(CacheLayerSettings::new(100, 60)));
    let result_cache = Arc::new(TantivyResultCache::new(&TantivyResultCacheSettings {
        capacity: 100,
        ttl_secs: 60,
    }));
    let pruner =
        TantivyPruner::new(handle_cache, store.clone()).with_result_cache(result_cache.clone());

    let preds = vec![MatchPredicate {
        field: "message".into(),
        term: "panic".into(),
    }];
    let fm = fm_with(parquet_key).await;
    let kept1 = pruner.prune(vec![fm.clone()], &preds).await.unwrap();
    assert_eq!(kept1.len(), 1);
    let hits_before = crate::shared::metrics::gather_text().unwrap();
    let n_before = scrape_counter(&hits_before, "cache_tantivy_result_hits_total");

    let kept2 = pruner.prune(vec![fm], &preds).await.unwrap();
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
    let parquet_key = "orgA/logs/raw/log_app/2026/01/15/09/obj-m.parquet";
    let index_object_key = sidecar_key(parquet_key);
    seed_archive(&store, &index_object_key, &["panic msg"]).await;

    let handle_cache: Arc<ParquetMetaCache<Arc<IndexHandle>>> =
        Arc::new(ParquetMetaCache::new(CacheLayerSettings::new(100, 60)));
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
    let preds = vec![MatchPredicate {
        field: "message".into(),
        term: "panic".into(),
    }];
    let fm = fm_with(parquet_key).await;
    // 两轮 prune：第一轮 miss → 写两层 cache；第二轮 result cache 命中。
    let _ = pruner.prune(vec![fm.clone()], &preds).await.unwrap();
    let _ = pruner.prune(vec![fm], &preds).await.unwrap();

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
    let parquet_key = "orgA/logs/raw/log_app/2026/01/15/09/obj-f.parquet";
    let index_object_key = sidecar_key(parquet_key);
    seed_archive(&store, &index_object_key, &["panic 1", "panic 2"]).await;

    // 第一次 prune：fresh handle cache + 共享 footer cache → footer 写入 cache。
    let footer_cache = Arc::new(TantivyFooterCache::new(&TantivyFooterCacheSettings {
        capacity: 100,
        ttl_secs: 60,
    }));
    let handle_cache_1: Arc<ParquetMetaCache<Arc<IndexHandle>>> =
        Arc::new(ParquetMetaCache::new(CacheLayerSettings::new(100, 60)));
    let pruner1 =
        TantivyPruner::new(handle_cache_1, store.clone()).with_footer_cache(footer_cache.clone());
    let preds = vec![MatchPredicate {
        field: "message".into(),
        term: "panic".into(),
    }];
    let fm = fm_with(parquet_key).await;
    let k1 = pruner1.prune(vec![fm.clone()], &preds).await.unwrap();
    assert_eq!(k1.len(), 1);

    // 删掉对象：模拟 IndexHandle 失效后的 worst case。
    store
        .delete(&Path::from(index_object_key.clone()))
        .await
        .unwrap();
    // 校验：对象确实没了，object_store GET 会 NotFound。
    let direct = store.get(&Path::from(index_object_key.clone())).await;
    assert!(matches!(direct, Err(object_store::Error::NotFound { .. })));

    // 第二次 prune：fresh handle cache（IndexHandle 已 evict 等价），共享 footer cache。
    let handle_cache_2: Arc<ParquetMetaCache<Arc<IndexHandle>>> =
        Arc::new(ParquetMetaCache::new(CacheLayerSettings::new(100, 60)));
    let pruner2 =
        TantivyPruner::new(handle_cache_2, store.clone()).with_footer_cache(footer_cache.clone());
    let k2 = pruner2.prune(vec![fm], &preds).await.unwrap();
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
    let parquet_key = "orgA/logs/raw/log_app/2026/01/15/09/obj-z.parquet";
    let index_object_key = sidecar_key(parquet_key);
    seed_archive(&store, &index_object_key, &["panic only"]).await;

    let handle_cache: Arc<ParquetMetaCache<Arc<IndexHandle>>> =
        Arc::new(ParquetMetaCache::new(CacheLayerSettings::new(100, 60)));
    let result_cache = Arc::new(TantivyResultCache::new(&TantivyResultCacheSettings {
        capacity: 0, // disabled
        ttl_secs: 60,
    }));
    let pruner =
        TantivyPruner::new(handle_cache, store.clone()).with_result_cache(result_cache.clone());

    let preds = vec![MatchPredicate {
        field: "message".into(),
        term: "panic".into(),
    }];
    let fm = fm_with(parquet_key).await;
    // 两次 prune 都应当正确：capacity=0 走 no-op cache，结果与不挂 cache 等价。
    let k1 = pruner.prune(vec![fm.clone()], &preds).await.unwrap();
    let k2 = pruner.prune(vec![fm], &preds).await.unwrap();
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
    let (preds, rewritten) =
        extract_match_predicates("SELECT * FROM logs WHERE MATCH(message, 'panic')");
    assert_eq!(preds.len(), 1);
    assert_eq!(preds[0].field, "message");
    assert_eq!(preds[0].term, "panic");
    // MATCH 语义定稿为 ILIKE（大小写不敏感）。
    assert_eq!(
        rewritten,
        "SELECT * FROM logs WHERE message ILIKE '%panic%'"
    );
}

#[test]
fn extract_multiple_match_predicates() {
    let (preds, rewritten) = extract_match_predicates(
        "SELECT * FROM logs WHERE MATCH(message,'fatal') AND MATCH(level,'error')",
    );
    assert_eq!(preds.len(), 2);
    assert!(rewritten.contains("message ILIKE '%fatal%'"));
    assert!(rewritten.contains("level ILIKE '%error%'"));
}

#[test]
fn match_inside_or_is_rewritten_but_never_prunes_files() {
    let (preds, rewritten) = extract_match_predicates(
        "SELECT * FROM logs WHERE MATCH(message, 'panic') OR level = 'error'",
    );
    assert!(preds.is_empty());
    assert_eq!(
        rewritten,
        "SELECT * FROM logs WHERE message ILIKE '%panic%' OR level = 'error'"
    );

    let (preds, _) = extract_match_predicates(
        "SELECT * FROM logs WHERE service = 'api' AND (MATCH(message, 'panic') OR level = 'error')",
    );
    assert!(preds.is_empty());
}

#[test]
fn no_match_predicates() {
    let (preds, rewritten) =
        extract_match_predicates("SELECT count(*) FROM logs WHERE level = 'info'");
    assert!(preds.is_empty());
    assert_eq!(rewritten, "SELECT count(*) FROM logs WHERE level = 'info'");
}

/// 含大写 / 标点的 term 与 TEXT 索引（小写化 + 按非字母数字切分）内容对不上，用它做
/// tantivy 裁剪会误裁漏数据。这类 term 必须**不生成裁剪谓词**，但 LIKE rewrite 照常生成
/// —— 结果正确性由 LIKE 兜底，裁剪只是可选加速。
#[test]
fn match_term_with_uppercase_or_punctuation_is_not_used_for_pruning() {
    // 大写：索引里是 `failed`，原串 `FAILED` count_term 会假命中 0。
    let (preds, rewritten) =
        extract_match_predicates("SELECT * FROM logs WHERE MATCH(message, 'FAILED')");
    assert!(preds.is_empty(), "大写 term 不得进裁剪谓词，实得 {preds:?}");
    assert_eq!(
        rewritten, "SELECT * FROM logs WHERE message ILIKE '%FAILED%'",
        "ILIKE rewrite 必须保留原样（大小写不变），由 DataFusion 兜底执行"
    );

    // 标点：索引里被切成 `my`/`api`，原串 `my-api` count_term 会假命中 0。
    let (preds, _) = extract_match_predicates("SELECT * FROM logs WHERE MATCH(path, 'my-api')");
    assert!(preds.is_empty(), "带连字符的 term 不得进裁剪谓词");

    // 超过 tantivy 40 字节 token 上限的 term 同样会被索引丢弃 → 不可用于裁剪。
    let long = "a".repeat(41);
    let (preds, _) = extract_match_predicates(&format!(
        "SELECT * FROM logs WHERE MATCH(message, '{long}')"
    ));
    assert!(preds.is_empty(), "超长 term 不得进裁剪谓词");
}

/// 全小写字母数字、不超长的 term 与 TEXT 索引内容一致，仍照常用于裁剪（修复不能误伤
/// 正常场景 —— 这是绝大多数裁剪收益的来源）。
#[test]
fn plain_lowercase_term_still_prunes() {
    let (preds, _) = extract_match_predicates("SELECT * FROM logs WHERE MATCH(message, 'panic')");
    assert_eq!(preds.len(), 1);
    assert_eq!(preds[0].term, "panic");

    let (preds, _) = extract_match_predicates("SELECT * FROM logs WHERE MATCH(trace, 'a1b2c3d4')");
    assert_eq!(preds.len(), 1, "hex 串（trace_id 形态）应仍可裁剪");
}

// ===== MATCH_TEXT 查询语法解析（D4）与 ILIKE rewrite =====

#[test]
fn match_text_token_and_rewrites_to_parenthesized_iliike() {
    let (preds, rewritten) =
        extract_match_predicates("SELECT * FROM logs WHERE MATCH_TEXT(message, 'panic disk')");
    assert_eq!(
        rewritten,
        "SELECT * FROM logs WHERE (message ILIKE '%panic%' AND message ILIKE '%disk%')"
    );
    // 纯 AND 树 + 两个无通配符单 token → 每个都生成裁剪谓词。
    assert_eq!(preds.len(), 2);
    assert_eq!(preds[0].term, "panic");
    assert_eq!(preds[1].term, "disk");
}

#[test]
fn match_text_phrase_is_contiguous_substring() {
    let (_, rewritten) =
        extract_match_predicates("SELECT * FROM logs WHERE MATCH_TEXT(message, '\"disk full\"')");
    assert_eq!(
        rewritten,
        "SELECT * FROM logs WHERE message ILIKE '%disk full%'"
    );
}

#[test]
fn match_text_mixed_token_and_phrase() {
    let (_, rewritten) = extract_match_predicates(
        "SELECT * FROM logs WHERE MATCH_TEXT(message, 'panic \"disk full\"')",
    );
    assert_eq!(
        rewritten,
        "SELECT * FROM logs WHERE (message ILIKE '%panic%' AND message ILIKE '%disk full%')"
    );
}

#[test]
fn match_text_or_rewrites_to_or_expression() {
    let (preds, rewritten) = extract_match_predicates(
        "SELECT * FROM logs WHERE MATCH_TEXT(message, 'panic OR timeout')",
    );
    assert_eq!(
        rewritten,
        "SELECT * FROM logs WHERE (message ILIKE '%panic%' OR message ILIKE '%timeout%')"
    );
    assert!(preds.is_empty(), "含 OR 不生成裁剪谓词，实得 {preds:?}");
}

#[test]
fn match_text_not_rewrites_to_not_expression() {
    let (preds, rewritten) =
        extract_match_predicates("SELECT * FROM logs WHERE MATCH_TEXT(message, 'panic -debug')");
    assert_eq!(
        rewritten,
        "SELECT * FROM logs WHERE (message ILIKE '%panic%' AND NOT (message ILIKE '%debug%'))"
    );
    assert!(preds.is_empty(), "含 NOT 不生成裁剪谓词，实得 {preds:?}");
}

#[test]
fn match_text_wildcard_prefix_suffix_contains() {
    let (_, rewritten) =
        extract_match_predicates("SELECT * FROM logs WHERE MATCH_TEXT(message, 'error*')");
    assert_eq!(rewritten, "SELECT * FROM logs WHERE message ILIKE 'error%'");

    let (_, rewritten) =
        extract_match_predicates("SELECT * FROM logs WHERE MATCH_TEXT(message, '*error')");
    assert_eq!(rewritten, "SELECT * FROM logs WHERE message ILIKE '%error'");

    let (_, rewritten) =
        extract_match_predicates("SELECT * FROM logs WHERE MATCH_TEXT(message, '*error*')");
    assert_eq!(
        rewritten,
        "SELECT * FROM logs WHERE message ILIKE '%error%'"
    );
}

#[test]
fn match_text_wildcard_inside_phrase() {
    let (_, rewritten) =
        extract_match_predicates("SELECT * FROM logs WHERE MATCH_TEXT(message, '\"api v*\"')");
    assert_eq!(rewritten, "SELECT * FROM logs WHERE message ILIKE 'api v%'");
}

#[test]
fn match_text_escaped_asterisk_is_literal() {
    // `\*` → 字面星号；LIKE 里 `*` 不是通配符，保持原样即可。
    let (_, rewritten) =
        extract_match_predicates("SELECT * FROM logs WHERE MATCH_TEXT(message, '100\\*')");
    assert_eq!(rewritten, "SELECT * FROM logs WHERE message ILIKE '%100*%'");
}

#[test]
fn match_text_percent_is_literal_not_wildcard() {
    // `100%`：裸 `%` 按字面量转义（spec：`usage 100%` 命中、`usage 1000` 不命中）。
    let (_, rewritten) =
        extract_match_predicates("SELECT * FROM logs WHERE MATCH_TEXT(message, '100%')");
    assert_eq!(
        rewritten,
        "SELECT * FROM logs WHERE message ILIKE '%100\\%%'"
    );

    // `100\%`（显式转义）与裸 `%` 行为一致。
    let (_, rewritten) =
        extract_match_predicates("SELECT * FROM logs WHERE MATCH_TEXT(message, '100\\%')");
    assert_eq!(
        rewritten,
        "SELECT * FROM logs WHERE message ILIKE '%100\\%%'"
    );
}

#[test]
fn match_text_underscore_is_literal_not_wildcard() {
    let (_, rewritten) =
        extract_match_predicates("SELECT * FROM logs WHERE MATCH_TEXT(message, 'a_b')");
    assert_eq!(
        rewritten,
        "SELECT * FROM logs WHERE message ILIKE '%a\\_b%'"
    );
}

#[test]
fn match_text_empty_query_is_always_false() {
    let (preds, rewritten) =
        extract_match_predicates("SELECT * FROM logs WHERE MATCH_TEXT(message, '')");
    assert_eq!(rewritten, "SELECT * FROM logs WHERE FALSE");
    assert!(preds.is_empty());

    let (_, rewritten) =
        extract_match_predicates("SELECT * FROM logs WHERE MATCH_TEXT(message, '   ')");
    assert_eq!(rewritten, "SELECT * FROM logs WHERE FALSE");
}

#[test]
fn match_text_case_insensitive_rewrite_preserves_case() {
    // 大小写不敏感由 ILIKE 保证；rewrite 保留原样交给执行层。
    let (preds, rewritten) =
        extract_match_predicates("SELECT * FROM logs WHERE MATCH_TEXT(message, 'FAILED')");
    assert_eq!(
        rewritten,
        "SELECT * FROM logs WHERE message ILIKE '%FAILED%'"
    );
    // 但裁剪谓词先小写化（D7）：`FAILED` 小写化后与 TEXT 索引内容一致，可裁剪。
    assert_eq!(preds.len(), 1);
    assert_eq!(preds[0].term, "failed");
}

#[test]
fn match_text_lowercase_call_is_accepted() {
    let (_, rewritten) =
        extract_match_predicates("SELECT * FROM logs WHERE match_text(message, 'panic')");
    assert_eq!(
        rewritten,
        "SELECT * FROM logs WHERE message ILIKE '%panic%'"
    );
}

#[test]
fn match_text_single_token_equivalent_to_match() {
    // 单 token 的 MATCH_TEXT 退化为 MATCH 语义（spec text-match-functions）。
    let (_, match_text) =
        extract_match_predicates("SELECT * FROM logs WHERE MATCH_TEXT(message, 'failed')");
    let (_, plain_match) =
        extract_match_predicates("SELECT * FROM logs WHERE MATCH(message, 'failed')");
    assert_eq!(match_text, plain_match);
}

#[test]
fn match_text_or_keyword_does_not_swallow_plain_words() {
    // `orange` 以 `or` 开头但不是关键字；`a_or_b` 同理。
    let (_, rewritten) =
        extract_match_predicates("SELECT * FROM logs WHERE MATCH_TEXT(message, 'orange')");
    assert_eq!(
        rewritten,
        "SELECT * FROM logs WHERE message ILIKE '%orange%'"
    );

    let (_, rewritten) =
        extract_match_predicates("SELECT * FROM logs WHERE MATCH_TEXT(message, 'a_or_b')");
    assert_eq!(
        rewritten,
        "SELECT * FROM logs WHERE message ILIKE '%a\\_or\\_b%'"
    );
}

// ===== MATCH_TEXT 裁剪谓词边界（D6） =====

#[test]
fn match_text_pure_and_single_token_prunes() {
    let (preds, _) = extract_match_predicates(
        "SELECT * FROM logs WHERE MATCH_TEXT(message, 'failed') AND _timestamp > '2026-01-01'",
    );
    assert_eq!(preds.len(), 1);
    assert_eq!(preds[0].field, "message");
    assert_eq!(preds[0].term, "failed");
}

#[test]
fn match_text_phrase_never_prunes() {
    let (preds, _) =
        extract_match_predicates("SELECT * FROM logs WHERE MATCH_TEXT(message, '\"disk full\"')");
    assert!(preds.is_empty(), "短语不裁剪，实得 {preds:?}");
}

#[test]
fn match_text_wildcard_never_prunes() {
    let (preds, _) =
        extract_match_predicates("SELECT * FROM logs WHERE MATCH_TEXT(message, 'error*')");
    assert!(preds.is_empty(), "通配符不裁剪，实得 {preds:?}");
}

#[test]
fn match_text_mixed_with_phrase_prunes_nothing() {
    // 纯 AND 树但含短语叶子 → 整次调用不裁剪（D6 严格条件）。
    let (preds, _) = extract_match_predicates(
        "SELECT * FROM logs WHERE MATCH_TEXT(message, 'panic \"disk full\"')",
    );
    assert!(preds.is_empty(), "含短语的调用不裁剪，实得 {preds:?}");
}

#[test]
fn match_text_not_at_top_level_conjunct_never_prunes() {
    let (preds, _) = extract_match_predicates(
        "SELECT * FROM logs WHERE MATCH_TEXT(message, 'panic') OR level = 'info'",
    );
    assert!(preds.is_empty(), "非顶层合取项不裁剪，实得 {preds:?}");

    let (preds, _) = extract_match_predicates(
        "SELECT * FROM logs WHERE service = 'api' AND (MATCH_TEXT(message, 'panic') OR level = 'info')",
    );
    assert!(preds.is_empty(), "OR 分支内不裁剪，实得 {preds:?}");
}

#[test]
fn match_text_multiple_top_level_conjuncts_all_prune() {
    let (preds, _) = extract_match_predicates(
        "SELECT * FROM logs WHERE MATCH_TEXT(message, 'panic') AND MATCH_TEXT(level, 'info')",
    );
    assert_eq!(preds.len(), 2);
    assert_eq!(preds[0].term, "panic");
    assert_eq!(preds[1].term, "info");
}

// ===== 与 TEXT 索引 token 形态一致性（防止重开分词不匹配漏数据缺口） =====

#[test]
fn match_text_prune_token_must_match_indexed_token_shape() {
    // `cats` 与 `cat` 都是 SimpleTokenizer 的合法 token，可裁剪。
    let (preds, _) =
        extract_match_predicates("SELECT * FROM logs WHERE MATCH_TEXT(message, 'cats')");
    assert_eq!(preds.len(), 1);
    let (preds, _) =
        extract_match_predicates("SELECT * FROM logs WHERE MATCH_TEXT(message, 'cat')");
    assert_eq!(preds.len(), 1);

    // `my-api` 在索引里被切成 `my`/`api`，原串 count_term 假命中 0 → 不裁剪。
    let (preds, _) =
        extract_match_predicates("SELECT * FROM logs WHERE MATCH_TEXT(path, 'my-api')");
    assert!(preds.is_empty(), "带连字符 token 不裁剪，实得 {preds:?}");

    // 超过 tantivy 40 字节 token 上限 → 索引丢弃 → 不裁剪。
    let long = "a".repeat(41);
    let (preds, _) = extract_match_predicates(&format!(
        "SELECT * FROM logs WHERE MATCH_TEXT(message, '{long}')"
    ));
    assert!(preds.is_empty(), "超长 token 不裁剪，实得 {preds:?}");
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
