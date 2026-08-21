// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Intake role 的核心组件：WalPool + BufferPool + flush 路径。
//!
//! 数据流：
//! ```text
//!   IntakeService::intake(batch)
//!     ├─ DatasetResolver.resolve(stream, dataset_type) → PhysicalDataset
//!     └─ BufferPool[dataset_id] lock:
//!          WalPool[dataset_id].append(payload) → (epoch, seq) → push(events, position)
//!
//!   FlushScheduler tick:
//!     for each buffer where size >= buffer_max_mb OR age >= flush_interval_secs:
//!       generation = buffer.begin_flush() // remains query-visible as in-flight
//!       parquet_writer.flush_catalog(...)
//!       file_catalog.commit_flush(...)
//!       buffer.complete_flush(generation.flush_id)
//!       wal_pool.truncate_up_to(dataset_id, generation.sequence_end)
//! ```

pub mod buffer_pool;
pub mod cardinality;
pub mod dataset_resolver;
pub mod metrics;
pub(crate) mod physical_schema;
pub mod rotation;
pub mod sink;
pub mod wal_pool;

pub use buffer_pool::{
    BufferKey, BufferPool, BufferWriter, BufferedRecordBatch, DatasetBuffer, RecordBuilder,
};
pub use cardinality::{PrometheusSeriesAdmission, SeriesIdentity, SeriesLimitReason};
pub use dataset_resolver::{DatasetResolver, ResolvedDataset};
pub use metrics::{FlushInflightGuard, inc_flush_error, inc_rotation};
pub use rotation::{AdaptiveRotation, RotationReason};
pub use sink::DurableIntakeSink;
pub use wal_pool::{WalAppendPosition, WalEpochDir, WalPool, WalRecoverySource, WalStreamIdentity};
