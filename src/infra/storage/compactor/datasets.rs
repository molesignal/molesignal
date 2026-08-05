// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Physical datasets managed by compaction and retention for each stream type.

use crate::domain::{storage::PhysicalDatasetKind, stream::StreamType};

pub(super) fn for_stream_type(stream_type: StreamType) -> &'static [PhysicalDatasetKind] {
    use PhysicalDatasetKind::{
        MetricCatalog, MetricRollup, Raw, RumErrorSummary, RumSessionSummary, TraceSummary,
    };

    match stream_type {
        StreamType::Logs => &[Raw, RumSessionSummary, RumErrorSummary],
        StreamType::Metrics => &[Raw, MetricCatalog, MetricRollup],
        StreamType::Traces => &[Raw, TraceSummary],
        // Profile sample payloads live in the dedicated compressed archive;
        // the Parquet stream is already a narrow metadata table.
        StreamType::Profiles => &[Raw],
        StreamType::Extend => &[Raw],
    }
}
