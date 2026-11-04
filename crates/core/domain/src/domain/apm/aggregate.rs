// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

use super::{
    ApmOutcome, ApmSpanFact, DependencyIdentity, ErrorIdentity, HistogramSchema, LatencyHistogram,
    ServiceIdentity, TransactionIdentity, TransactionKind,
};
use crate::shared::{Result, ids::Id, time::TimestampMicros};

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
    /// The requests a service served, one instance at a time.
    Instance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceExemplar {
    pub trace_id: String,
    pub span_id: String,
    pub event_time: TimestampMicros,
    pub duration: u64,
    #[serde(default)]
    pub trace_available: bool,
}

/// The most status codes a bucket counts one by one; past them the rest of a
/// protocol's are counted as `<protocol>:other`.
pub const MAX_STATUS_CODES: usize = 24;

/// The most exemplars buckets merged together keep.
pub const MERGED_EXEMPLAR_CAP: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BucketMeasurements {
    pub request_count: u64,
    pub error_count: u64,
    #[serde(default)]
    pub overflow_count: u64,
    pub latency: LatencyHistogram,
    #[serde(default)]
    pub exemplars: Vec<TraceExemplar>,
    /// The requests by what they answered: `http:404`, `grpc:14`. Buckets
    /// written before status codes were counted have none.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub status_codes: BTreeMap<String, u64>,
    /// Requests that ran out of time. Buckets written before these were
    /// counted have none.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub timeout_count: u64,
    /// Requests sent again.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub retry_count: u64,
}

impl BucketMeasurements {
    /// No request yet, its latency to be held in `schema`'s layout.
    pub fn empty(schema: &HistogramSchema) -> Self {
        Self {
            request_count: 0,
            error_count: 0,
            overflow_count: 0,
            latency: LatencyHistogram::empty(schema),
            exemplars: Vec::new(),
            status_codes: BTreeMap::new(),
            timeout_count: 0,
            retry_count: 0,
        }
    }

    /// Counts the request of `fact`, `overflow` when it lost a dimension to
    /// a cardinality limit; its latency is observed apart, in the layout.
    pub fn count(&mut self, fact: &ApmSpanFact, overflow: bool) {
        let add = |value: &mut u64, holds: bool| *value = value.saturating_add(u64::from(holds));

        add(&mut self.request_count, true);
        add(&mut self.error_count, fact.outcome == ApmOutcome::Error);
        add(&mut self.overflow_count, overflow);
        add(&mut self.timeout_count, fact.timed_out);
        add(&mut self.retry_count, fact.retried);

        if let Some(status) = &fact.response_status {
            self.count_status(status, 1);
        }
    }

    /// Counts `count` requests that answered `status`, within [`MAX_STATUS_CODES`].
    pub fn count_status(&mut self, status: &str, count: u64) {
        count_status(&mut self.status_codes, status, count);
    }

    /// Adds `other`'s requests to these: counts summed, latencies merged,
    /// exemplars kept up to [`MERGED_EXEMPLAR_CAP`]. Fails, changing nothing,
    /// when the two hold their latencies in different layouts.
    pub fn merge(&mut self, other: &BucketMeasurements) -> Result<()> {
        self.latency.merge(&other.latency)?;
        self.request_count = self.request_count.saturating_add(other.request_count);
        self.error_count = self.error_count.saturating_add(other.error_count);
        self.overflow_count = self.overflow_count.saturating_add(other.overflow_count);
        self.timeout_count = self.timeout_count.saturating_add(other.timeout_count);
        self.retry_count = self.retry_count.saturating_add(other.retry_count);

        for (status, count) in &other.status_codes {
            self.count_status(status, *count);
        }

        let mut seen = self
            .exemplars
            .iter()
            .map(|item| (item.trace_id.clone(), item.span_id.clone()))
            .collect::<HashSet<_>>();

        for exemplar in &other.exemplars {
            if self.exemplars.len() >= MERGED_EXEMPLAR_CAP {
                break;
            }

            if seen.insert((exemplar.trace_id.clone(), exemplar.span_id.clone())) {
                self.exemplars.push(exemplar.clone());
            }
        }

        Ok(())
    }
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

/// Adds `count` to `status` in `codes`, a code past [`MAX_STATUS_CODES`]
/// going to its protocol's `other`.
pub fn count_status(codes: &mut BTreeMap<String, u64>, status: &str, count: u64) {
    let key = if codes.contains_key(status) || codes.len() < MAX_STATUS_CODES {
        status.to_owned()
    } else {
        let protocol = status
            .split_once(':')
            .map_or("other", |(protocol, _)| protocol);
        format!("{protocol}:other")
    };
    let counted = codes.entry(key).or_default();
    *counted = counted.saturating_add(count);
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
    /// What one instance of the service served: its `service.instance.id`,
    /// else its pod, else its host; [`super::OTHER_DIMENSION`] past the
    /// instance limit.
    Instance {
        service: ServiceIdentity,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<String>,
        instance_id: String,
    },
}

impl BucketDimension {
    pub fn kind(&self) -> BucketKind {
        match self {
            Self::Service { .. } => BucketKind::Service,
            Self::Transaction { .. } => BucketKind::Transaction,
            Self::Dependency { .. } => BucketKind::Dependency,
            Self::Error { .. } => BucketKind::Error,
            Self::Instance { .. } => BucketKind::Instance,
        }
    }

    pub fn service(&self) -> &ServiceIdentity {
        match self {
            Self::Service { service, .. }
            | Self::Transaction { service, .. }
            | Self::Dependency { service, .. }
            | Self::Error { service, .. }
            | Self::Instance { service, .. } => service,
        }
    }

    pub fn service_mut(&mut self) -> &mut ServiceIdentity {
        match self {
            Self::Service { service, .. }
            | Self::Transaction { service, .. }
            | Self::Dependency { service, .. }
            | Self::Error { service, .. }
            | Self::Instance { service, .. } => service,
        }
    }

    pub fn version(&self) -> Option<&str> {
        match self {
            Self::Service { version, .. }
            | Self::Transaction { version, .. }
            | Self::Dependency { version, .. }
            | Self::Error { version, .. }
            | Self::Instance { version, .. } => version.as_deref(),
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
    /// The kinds of entry the service was seen serving (HTTP, RPC, messages,
    /// internal work), sorted: what kind of service it is.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entry_kinds: Vec<TransactionKind>,
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
