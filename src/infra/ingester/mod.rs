// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Ingester role 的核心组件：WalPool + BufferPool + flush 路径。
//!
//! 数据流：
//! ```text
//!   IngestService::ingest(batch)
//!     ├─ WalPool[(org, stream_type, stream)].append(payload, seq)
//!     └─ BufferPool[(org, stream_type, stream)].push(events, seq)
//!
//!   FlushScheduler tick:
//!     for each buffer where size >= buffer_max_mb OR age >= flush_interval_secs:
//!       (record_batch, high_watermark_seq) = buffer.finish_and_clear()
//!       parquet_writer.flush_with_index(...)
//!       parquet_file_meta_repo.insert(...)
//!       wal_pool.truncate_up_to(high_watermark_seq)
//! ```

pub mod buffer_pool;
pub mod cardinality;
pub mod metrics;
pub(crate) mod physical_schema;
pub mod rotation;
pub mod sink;
pub mod wal_pool;

pub use buffer_pool::{BufferKey, BufferPool, RecordBuilder};
pub use cardinality::{PrometheusSeriesAdmission, SeriesIdentity, SeriesLimitReason};
pub use metrics::{FlushInflightGuard, inc_flush_error, inc_rotation};
pub use rotation::{AdaptiveRotation, RotationReason};
pub use sink::IngesterSink;
pub use wal_pool::{WalKey, WalPool};
