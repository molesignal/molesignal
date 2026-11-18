// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The columnar intake path for native log writes.
//!
//! The row path turns a request body into a map per event and then checks, extends the schema,
//! measures, serializes and converts it field by field: a profile of the benchmark's intake put
//! two fifths of the node's CPU there. This path decodes the body once with simd-json straight
//! into the buffer's Arrow columns (see [`decode`]), applying the same rules, and hands the batch
//! to the sink whole.
//!
//! It takes log streams, with write-time masking, encrypted fields (encrypted by the sink) and
//! real-time alert rules (matched against the fields they read). A stream whose events are changed
//! by code keeps the row path: one with a real-time pipeline, and the RUM streams that merge or
//! track their rows. A stream that does not exist yet does too, so that it is created as before.

mod decode;
mod json_text;
mod service;

use arrow::array::RecordBatch;
use async_trait::async_trait;
use domain::{intake::IntakeResult, storage::DatasetTypeId, stream::StreamType};
use kernel::{Result, ids::Id};

use crate::app::intake::usage::ServiceTally;

/// A batch in the buffer's columns, ready for the WAL and the buffer.
#[derive(Debug, Clone)]
pub struct ColumnarBatch {
    pub org_id: Id,
    pub stream: String,
    pub stream_type: StreamType,
    /// Usage bytes of the batch's events.
    pub collected_bytes: Option<u64>,
    pub columns: RecordBatch,
}

/// Where columnar batches are written: the intake role's sink.
#[async_trait]
pub trait ColumnarSink: Send + Sync {
    async fn write_columns(
        &self,
        dataset_type: DatasetTypeId,
        batch: ColumnarBatch,
    ) -> Result<IntakeResult>;
}

/// What a JSON request body came to: the intake result, and its events by service for usage.
#[derive(Debug)]
pub struct JsonIntake {
    pub result: IntakeResult,
    pub services: ServiceTally,
}
