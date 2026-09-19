// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

use super::{
    DependencyIdentity, ErrorIdentity, LatencyHistogram, ServiceIdentity, TransactionIdentity,
};
use crate::shared::{ids::Id, time::TimestampMicros};

pub const APM_PERSISTENCE_SCHEMA_VERSION: u16 = 1;

fn persistence_schema_version() -> u16 {
    APM_PERSISTENCE_SCHEMA_VERSION
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BucketKind {
    Service,
    Transaction,
    Dependency,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceExemplar {
    pub trace_id: String,
    pub span_id: String,
    pub event_time: TimestampMicros,
    pub duration_micros: u64,
    #[serde(default)]
    pub trace_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BucketMeasurements {
    pub request_count: u64,
    pub error_count: u64,
    #[serde(default)]
    pub overflow_count: u64,
    pub latency: LatencyHistogram,
    #[serde(default)]
    pub exemplars: Vec<TraceExemplar>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BucketDimension {
    Service {
        service: ServiceIdentity,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<String>,
    },
    Transaction {
        service: ServiceIdentity,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<String>,
        transaction: TransactionIdentity,
    },
    Dependency {
        service: ServiceIdentity,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<String>,
        dependency: DependencyIdentity,
    },
    Error {
        service: ServiceIdentity,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<String>,
        error: ErrorIdentity,
    },
}

impl BucketDimension {
    pub fn kind(&self) -> BucketKind {
        match self {
            Self::Service { .. } => BucketKind::Service,
            Self::Transaction { .. } => BucketKind::Transaction,
            Self::Dependency { .. } => BucketKind::Dependency,
            Self::Error { .. } => BucketKind::Error,
        }
    }

    pub fn service(&self) -> &ServiceIdentity {
        match self {
            Self::Service { service, .. }
            | Self::Transaction { service, .. }
            | Self::Dependency { service, .. }
            | Self::Error { service, .. } => service,
        }
    }

    pub fn version(&self) -> Option<&str> {
        match self {
            Self::Service { version, .. }
            | Self::Transaction { version, .. }
            | Self::Dependency { version, .. }
            | Self::Error { version, .. } => version.as_deref(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnerSnapshot {
    #[serde(default = "persistence_schema_version")]
    pub schema_version: u16,
    pub org_id: Id,
    pub owner_id: String,
    pub bucket_at: TimestampMicros,
    pub snapshot_seq: u64,
    pub dimension: BucketDimension,
    pub measurements: BucketMeasurements,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HourlyRollup {
    #[serde(default = "persistence_schema_version")]
    pub schema_version: u16,
    pub org_id: Id,
    pub bucket_at: TimestampMicros,
    pub dimension: BucketDimension,
    pub measurements: BucketMeasurements,
    pub source_minute_count: u16,
    pub completed_at: TimestampMicros,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceObservation {
    pub org_id: Id,
    pub service: ServiceIdentity,
    pub first_seen_at: TimestampMicros,
    pub last_seen_at: TimestampMicros,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telemetry_sdk_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telemetry_sdk_version: Option<String>,
    pub recent_instance_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionObservation {
    pub org_id: Id,
    pub service: ServiceIdentity,
    pub version: String,
    pub first_seen_at: TimestampMicros,
    pub last_seen_at: TimestampMicros,
    pub observation_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorSample {
    pub org_id: Id,
    pub error: ErrorIdentity,
    pub service: ServiceIdentity,
    pub event_time: TimestampMicros,
    pub trace_id: String,
    pub span_id: String,
    #[serde(default)]
    pub trace_available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub representative_message: Option<String>,
    #[serde(default)]
    pub representative_stack: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorGroupRecord {
    pub org_id: Id,
    pub error: ErrorIdentity,
    pub service: ServiceIdentity,
    pub first_seen_at: TimestampMicros,
    pub last_seen_at: TimestampMicros,
    pub occurrence_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub representative_message: Option<String>,
    #[serde(default)]
    pub representative_stack: Vec<String>,
}
