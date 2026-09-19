// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! cache hit 行为端到端验证。
//!
//! - 同一 closed-window SQL 连发两次：第二次 `cache_hit = true`，loader 仅跑一次。
//! - open-window SQL（time_range.end > now - 5min）两次都 miss。
//! - 命中后 `cache_query_result_hits_total` counter += 1（caching::metrics 内部）。

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use molesignal::{
    config::CacheLayerSettings,
    domain::query::{QueryLanguage, QueryRequest, QueryResult},
    infra::caching::QueryResultCache,
    shared::{
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

fn req(now: i64, lookback_us: i64, end_off_us: i64, stmt: &str) -> QueryRequest {
    QueryRequest {
        org_id: Id::from_string("orga"),
        language: QueryLanguage::Sql,
        statement: stmt.into(),
        time_range: TimeRange::new(
            TimestampMicros(now - lookback_us),
            TimestampMicros(now - end_off_us),
        ),
        stream: None,
        limit: None,
        federation_clusters: Vec::new(),
    }
}

#[tokio::test]
async fn closed_window_second_call_hits_cache() {
    let cache = QueryResultCache::new(CacheLayerSettings::new(100, 60));
    let calls = Arc::new(AtomicUsize::new(0));
    let now: i64 = 10_000_000_000;
    // end = now - 10min（远超 5min 直通窗口）
    let r = req(now, 100_000_000_000, 10 * 60_000_000, "SELECT 1");

    let mk = |c: Arc<AtomicUsize>| {
        move || {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok(QueryResult {
                    columns: vec!["x".into()],
                    rows: vec![vec![serde_json::json!(1)]],
                    scanned_rows: 1,
                    took_ms: 0,
                    federation: None,
                })
            }
        }
    };

    let (_r1, h1) = cache
        .get_or_insert(&r, "role", now, mk(calls.clone()))
        .await
        .unwrap();
    let (_r2, h2) = cache
        .get_or_insert(&r, "role", now, mk(calls.clone()))
        .await
        .unwrap();
    assert!(!h1, "first must be miss");
    assert!(h2, "second must be cache hit");
    assert_eq!(calls.load(Ordering::SeqCst), 1, "loader ran exactly once");
}

#[tokio::test]
async fn open_window_never_caches() {
    let cache = QueryResultCache::new(CacheLayerSettings::new(100, 60));
    let calls = Arc::new(AtomicUsize::new(0));
    let now: i64 = 10_000_000_000;
    // end = now - 1min（在 5min 直通窗口内）→ 两次都直通
    let r = req(now, 100_000_000_000, 60_000_000, "SELECT 1");

    let mk = |c: Arc<AtomicUsize>| {
        move || {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok(QueryResult {
                    columns: vec!["x".into()],
                    rows: vec![vec![serde_json::json!(1)]],
                    scanned_rows: 1,
                    took_ms: 0,
                    federation: None,
                })
            }
        }
    };

    let (_r1, h1) = cache
        .get_or_insert(&r, "role", now, mk(calls.clone()))
        .await
        .unwrap();
    let (_r2, h2) = cache
        .get_or_insert(&r, "role", now, mk(calls.clone()))
        .await
        .unwrap();
    assert!(!h1 && !h2, "open-window never reports cache_hit");
    assert_eq!(calls.load(Ordering::SeqCst), 2, "loader ran twice");
}
