// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{
    BucketDimension, BucketKind, BucketMeasurements, ErrorGroupRecord, ErrorSample, OwnerSnapshot,
    ProjectionGap, ProjectionState, QueryResolution, ServiceObservation, VersionObservation,
};
use crate::shared::{
    Result,
    ids::Id,
    time::{TimeRange, TimestampMicros},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BucketQuery {
    pub org_id: Id,
    pub range: TimeRange,
    pub kind: BucketKind,
    #[serde(default)]
    pub resolution: QueryResolution,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergedBucket {
    pub bucket_at: TimestampMicros,
    pub dimension: BucketDimension,
    pub measurements: BucketMeasurements,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogQuery {
    pub org_id: Id,
    pub range: TimeRange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorGroupQuery {
    pub org_id: Id,
    pub range: TimeRange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotWriteStats {
    pub attempted: u64,
    pub applied: u64,
    pub stale: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollupRequest {
    pub org_id: Id,
    pub hour_at: TimestampMicros,
    pub hot_retention_cutoff: TimestampMicros,
    pub rollup_retention_cutoff: TimestampMicros,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollupCandidate {
    pub org_id: Id,
    pub hour_at: TimestampMicros,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollupStats {
    pub source_rows: u64,
    pub rollup_rows: u64,
    pub deleted_hot_rows: u64,
    pub deleted_rollup_rows: u64,
}

#[async_trait]
pub trait ApmWriteRepository: Send + Sync {
    async fn upsert_catalog(
        &self,
        services: &[ServiceObservation],
        versions: &[VersionObservation],
    ) -> Result<()>;

    /// Replaces an owner snapshot only when `snapshot_seq` is newer.
    async fn replace_owner_snapshots(
        &self,
        snapshots: &[OwnerSnapshot],
    ) -> Result<SnapshotWriteStats>;

    async fn upsert_error_groups(
        &self,
        groups: &[ErrorGroupRecord],
        samples: &[ErrorSample],
        max_samples_per_group: usize,
    ) -> Result<()>;

    async fn record_projection_gaps(&self, gaps: &[ProjectionGap]) -> Result<()>;

    async fn ensure_projection_started(
        &self,
        org_id: &Id,
        started_at: TimestampMicros,
    ) -> Result<ProjectionState>;

    async fn advance_projection_complete(
        &self,
        org_id: &Id,
        bucket_at: TimestampMicros,
    ) -> Result<()>;
}

#[async_trait]
pub trait ApmQueryRepository: Send + Sync {
    async fn query_buckets(&self, query: &BucketQuery) -> Result<Vec<MergedBucket>>;
    async fn list_services(&self, query: &CatalogQuery) -> Result<Vec<ServiceObservation>>;
    async fn list_versions(&self, query: &CatalogQuery) -> Result<Vec<VersionObservation>>;
    async fn list_error_groups(&self, query: &ErrorGroupQuery) -> Result<Vec<ErrorGroupRecord>>;
    async fn list_error_samples(&self, org_id: &Id, fingerprint: &str) -> Result<Vec<ErrorSample>>;
    async fn projection_state(&self, org_id: &Id) -> Result<Option<ProjectionState>>;
    async fn projection_gaps(&self, org_id: &Id, range: TimeRange) -> Result<Vec<ProjectionGap>>;
}

#[async_trait]
pub trait ApmMaintenanceRepository: Send + Sync {
    async fn rollup_candidates(
        &self,
        closed_before: TimestampMicros,
        limit: usize,
    ) -> Result<Vec<RollupCandidate>>;

    async fn rollup_and_retain(&self, request: &RollupRequest) -> Result<RollupStats>;
}

pub trait ApmRepository:
    ApmWriteRepository + ApmQueryRepository + ApmMaintenanceRepository
{
}

impl<T> ApmRepository for T where
    T: ApmWriteRepository + ApmQueryRepository + ApmMaintenanceRepository
{
}
