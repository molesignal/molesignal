// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The intake role's parts: the object-storage WAL, the memory buffers, and the flush path.
//!
//! ```text
//!   IntakeService::intake(batch)          IntakeService::intake_json(body), columnar path
//!     ├─ rows_to_columns(events)          ├─ decode(body) → columns
//!     ├─ DatasetResolver.resolve(stream, dataset_type) → PhysicalDataset
//!     └─ ObjectWal.append(Arrow IPC record): group commit → PUT wal/v1/... → lease still held
//!          → push_batch(columns, epoch, seq) under the buffer lock → acknowledge
//!
//!   flush tick, for each buffer past its size or age:
//!       generation = buffer.begin_flush()      // stays visible to queries while in flight
//!       parquet_writer.flush_catalog(...)
//!       file_catalog.commit_flush(...)         // fenced: only the stream's publisher commits
//!       buffer.complete_flush(generation.flush_id)
//!       object_wal.cleanup(dataset, epoch, generation.sequence_end)
//!
//!   takeover tick: lapsed writers are fenced, their streams claimed and published from objects
//! ```

pub mod buffer_pool;
pub mod cardinality;
pub mod dataset_resolver;
pub mod metrics;
pub mod object_wal;
pub(crate) mod physical_schema;
pub mod rotation;
pub mod row_columns;
pub mod sink;
pub mod wal_record;

pub use buffer_pool::{
    BufferKey, BufferPool, BufferWriter, BufferedRecordBatch, DatasetBuffer, Discarded,
    RecordBuilder,
};
pub use cardinality::{PrometheusSeriesAdmission, SeriesIdentity, SeriesLimitReason};
pub use dataset_resolver::{DatasetResolver, ResolvedDataset};
pub use metrics::{FlushInflightGuard, inc_flush_error, inc_rotation};
pub use object_wal::{
    MemoryWalWriters, ObjectWal, ObjectWalConfig, WalAppend, WalAppendPosition, WalStreamIdentity,
};
pub use rotation::{AdaptiveRotation, RotationReason};
pub use sink::DurableIntakeSink;
