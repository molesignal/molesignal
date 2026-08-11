// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 进程内多级缓存：
//!
//! 三层独立 LRU + TTL：
//! - [`ParquetFileMetaCache`]   `(org, stream, stream_type, time_bucket_hour)` → `Arc<Vec<ParquetFileMeta>>`
//! - [`ParquetMetaCache`] `object_key` → 调用方自定义 `V`（实际是 `Arc<ParquetMetaData>`
//!   或 Tantivy `IndexHandle`）
//! - [`QueryResultCache`] `blake3(stmt + org + time_range + role)` → `Arc<QueryResult>`
//!
//! 每层各自一组 `cache_<level>_{hits,misses,evictions}_total` 计数器 + `cache_<level>_hit_ratio` Gauge。

pub mod parquet_file_meta;
mod parquet_meta;
mod query_result;

pub mod billing_state;
pub mod disk_cache;
mod metrics;
pub mod org_schema;
pub mod stream_agg;
pub mod tantivy;

pub use billing_state::BillingStateCache;
pub use disk_cache::{DiskCacheSettings, ParquetDiskCache};
pub use org_schema::OrgSchemaCache;
pub use parquet_file_meta::{
    ParquetFileMetaCache, ParquetFileMetaPrefix, TimeBucketHour, bucket_of_hour,
    dump::{DumpCacheKey, ParquetFileMetaDumpCache, ParquetFileMetaDumpCacheRef},
};
pub use parquet_meta::ParquetMetaCache;
pub use query_result::{QUERY_FRESH_WINDOW_MICROS, QueryResultCache};
pub use stream_agg::{CachedLabels, SealedSeries, StreamingAggCache};
pub use tantivy::{
    footer::{TantivyFooterCache, TantivyFooterCacheRef},
    result::{TantivyResultCache, TantivyResultCacheRef, TantivyResultKey},
};

pub use crate::config::CacheLayerSettings;
