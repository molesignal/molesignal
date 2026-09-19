// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 进程内多级缓存：
//!
//! 独立 LRU + TTL 缓存：
//! - [`IndexHandleCache`] immutable Index Artifact key → opened index handle
//! - [`QueryResultCache`] `blake3(stmt + org + time_range + role)` → `Arc<QueryResult>`
//! - Tantivy predicate results and immutable footer metadata
//!
//! 每层各自一组 `cache_<level>_{hits,misses,evictions}_total` 计数器 + `cache_<level>_hit_ratio` Gauge。

mod index_handle;
mod query_result;

pub mod billing_state;
mod metrics;
pub mod org_schema;
pub mod stream_agg;
pub mod tantivy;

pub use billing_state::BillingStateCache;
pub use index_handle::IndexHandleCache;
pub use org_schema::OrgSchemaCache;
pub use query_result::{QUERY_FRESH_WINDOW_MICROS, QueryResultCache};
pub use stream_agg::{CachedLabels, SealedSeries, StreamingAggCache};
pub use tantivy::{
    footer::{TantivyFooterCache, TantivyFooterCacheRef},
    result::{TantivyResultCache, TantivyResultCacheRef, TantivyResultKey},
};

pub use crate::config::CacheLayerSettings;
