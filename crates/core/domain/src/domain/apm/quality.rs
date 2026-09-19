// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

use crate::shared::{
    ids::Id,
    time::{TimeRange, TimestampMicros},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionGapReason {
    QueueFull,
    RepositoryUnavailable,
    FlushFailed,
    LateDropped,
    CardinalityRejected,
    ShutdownTimeout,
}

impl ProjectionGapReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::QueueFull => "queue_full",
            Self::RepositoryUnavailable => "repository_unavailable",
            Self::FlushFailed => "flush_failed",
            Self::LateDropped => "late_dropped",
            Self::CardinalityRejected => "cardinality_rejected",
            Self::ShutdownTimeout => "shutdown_timeout",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "queue_full" => Self::QueueFull,
            "repository_unavailable" => Self::RepositoryUnavailable,
            "flush_failed" => Self::FlushFailed,
            "late_dropped" => Self::LateDropped,
            "cardinality_rejected" => Self::CardinalityRejected,
            "shutdown_timeout" => Self::ShutdownTimeout,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectionGap {
    pub org_id: Id,
    pub range: TimeRange,
    pub reason: ProjectionGapReason,
    pub dropped_facts: u64,
    pub recorded_at: TimestampMicros,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectionState {
    pub org_id: Id,
    pub projection_started_at: TimestampMicros,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_complete_bucket_at: Option<TimestampMicros>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_rollup_bucket_at: Option<TimestampMicros>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataQuality {
    #[serde(default)]
    pub partial: bool,
    #[serde(default)]
    pub gaps: Vec<ProjectionGap>,
    #[serde(default)]
    pub overflow_dimensions: Vec<String>,
}
