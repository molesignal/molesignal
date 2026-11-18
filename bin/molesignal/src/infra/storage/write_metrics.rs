// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What writing segments costs. The rows and bytes each stage writes show how many times
//! compaction writes a row again (all rows written over the rows flushed), and the time of each
//! stage of encoding a flushed segment shows where the writing CPU goes.

use std::{sync::OnceLock, time::Instant};

use domain::{storage::DataSegment, stream::StreamType};
use prometheus::{HistogramVec, IntCounterVec};
use signals::metrics::{register_histogram_vec, register_int_counter_vec};

/// The write that produced a segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteStage {
    /// Rows from the intake buffer, written for the first time.
    Flush,
    /// Rows of segments merged into one, written again.
    Compaction,
}

impl WriteStage {
    fn as_str(self) -> &'static str {
        match self {
            Self::Flush => "flush",
            Self::Compaction => "compaction",
        }
    }
}

/// A step of encoding a flushed segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EncodeStep {
    /// Putting the rows in the storage order.
    Sort,
    /// Encoding the Parquet file.
    Parquet,
    /// Building the index archive.
    Index,
    /// Building the filters, the column statistics and the minute counts.
    Filters,
}

impl EncodeStep {
    fn as_str(self) -> &'static str {
        match self {
            Self::Sort => "sort",
            Self::Parquet => "parquet",
            Self::Index => "index",
            Self::Filters => "filters",
        }
    }
}

static ROWS: OnceLock<IntCounterVec> = OnceLock::new();
static BYTES: OnceLock<IntCounterVec> = OnceLock::new();
static ENCODE: OnceLock<HistogramVec> = OnceLock::new();

fn rows() -> &'static IntCounterVec {
    ROWS.get_or_init(|| {
        register_int_counter_vec(
            "segment_rows_written_total",
            "rows written into segments, by stream type and write stage",
            &["stream_type", "stage"],
        )
    })
}

fn bytes() -> &'static IntCounterVec {
    BYTES.get_or_init(|| {
        register_int_counter_vec(
            "segment_bytes_written_total",
            "bytes of the data, index and filter files of written segments, by stream type and \
             write stage",
            &["stream_type", "stage"],
        )
    })
}

fn encode() -> &'static HistogramVec {
    ENCODE.get_or_init(|| {
        register_histogram_vec(
            "segment_encode_seconds",
            "seconds one step of encoding a flushed segment takes",
            &["step"],
            prometheus::exponential_buckets(0.001, 2.0, 17).expect("valid buckets"),
        )
    })
}

/// Counts the rows and bytes of `segments`, written by `stage` for a stream of `stream_type`.
pub(crate) fn record_written(
    stream_type: &StreamType,
    stage: WriteStage,
    segments: &[DataSegment],
) {
    let labels = [stream_type.as_str(), stage.as_str()];

    let row_count: u64 = segments.iter().map(|segment| segment.row_count).sum();

    let byte_count: u64 = segments
        .iter()
        .flat_map(DataSegment::artifacts)
        .map(|artifact| artifact.object.size_bytes)
        .sum();

    rows().with_label_values(&labels).inc_by(row_count);
    bytes().with_label_values(&labels).inc_by(byte_count);
}

/// Runs `work` and records how long it took as `step`.
pub(crate) fn timed<T>(step: EncodeStep, work: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let made = work();

    encode()
        .with_label_values(&[step.as_str()])
        .observe(started.elapsed().as_secs_f64());

    made
}
