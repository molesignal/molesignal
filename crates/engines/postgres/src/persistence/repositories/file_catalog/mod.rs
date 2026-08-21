// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! [`FileCatalog`] 的 PostgreSQL 实现：统一文件元数据的唯一事实来源。
//!
//! 表结构：`physical_datasets` → `data_segments` → `artifacts`，辅以
//! `storage_flush_commits`（flush 幂等）、`wal_checkpoints`（恢复与 Buffer
//! 去重）、`object_gc_queue`（延迟删除）。所有查询显式携带 `org_id`，
//! 子表通过 `(org_id, ...)` 复合外键关联，杜绝跨租户串行。

use async_trait::async_trait;
use sqlx::PgPool;

use super::sqlx_err;
use crate::{
    domain::storage::{
        CatalogInvariantIssue, CatalogObject, CatalogSnapshot, CommitFlush, DatasetSelection,
        DatasetTransformResult, FileCatalog, FlushCommitResult, GcQueueEntry, IndexRebuildTask,
        ObjectKey, OrganizationScope, PhysicalDataset, PhysicalDatasetId, PhysicalDatasetSpec,
        PublishDatasetTransform, PublishPartitionManifest, ReplaceSegments, TombstoneSegments,
        UpdateArtifact, WalCheckpointView,
    },
    shared::{Result, ids::Id},
};

mod artifact;
mod datasets;
mod flush;
mod gc;
mod maintenance;
mod manifest;
mod replacement;
mod rows;
mod snapshot;
mod writes;

pub struct PgFileCatalog {
    pool: PgPool,
}

impl PgFileCatalog {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl FileCatalog for PgFileCatalog {
    async fn ensure_datasets(
        &self,
        scope: &OrganizationScope,
        logical_stream_id: &Id,
        specs: &[PhysicalDatasetSpec],
    ) -> Result<Vec<PhysicalDataset>> {
        datasets::ensure_datasets(&self.pool, scope, logical_stream_id, specs).await
    }

    async fn list_datasets(
        &self,
        scope: &OrganizationScope,
        logical_stream_id: &Id,
    ) -> Result<Vec<PhysicalDataset>> {
        datasets::list_datasets(&self.pool, scope, logical_stream_id).await
    }

    async fn snapshot(
        &self,
        scope: &OrganizationScope,
        selection: DatasetSelection,
    ) -> Result<CatalogSnapshot> {
        snapshot::snapshot(&self.pool, scope, selection).await
    }

    async fn commit_flush(
        &self,
        scope: &OrganizationScope,
        command: CommitFlush,
    ) -> Result<FlushCommitResult> {
        flush::commit_flush(&self.pool, scope, command).await
    }

    async fn replace_segments(
        &self,
        scope: &OrganizationScope,
        command: ReplaceSegments,
    ) -> Result<u64> {
        replacement::replace_segments(&self.pool, scope, command).await
    }

    async fn publish_dataset_transform(
        &self,
        scope: &OrganizationScope,
        command: PublishDatasetTransform,
    ) -> Result<DatasetTransformResult> {
        replacement::publish_dataset_transform(&self.pool, scope, command).await
    }

    async fn update_artifact(
        &self,
        scope: &OrganizationScope,
        command: UpdateArtifact,
    ) -> Result<()> {
        artifact::update_artifact(&self.pool, scope, command).await
    }

    async fn tombstone_segments(
        &self,
        scope: &OrganizationScope,
        command: TombstoneSegments,
    ) -> Result<u64> {
        replacement::tombstone_segments(&self.pool, scope, command).await
    }

    async fn wal_checkpoints(
        &self,
        scope: &OrganizationScope,
        dataset_id: &PhysicalDatasetId,
    ) -> Result<Vec<WalCheckpointView>> {
        snapshot::wal_checkpoints(&self.pool, scope, dataset_id).await
    }

    async fn index_rebuild_tasks(
        &self,
        scope: &OrganizationScope,
        updated_before_micros: i64,
        limit: u32,
    ) -> Result<Vec<IndexRebuildTask>> {
        maintenance::index_rebuild_tasks(&self.pool, scope, updated_before_micros, limit).await
    }

    async fn publish_partition_manifest(
        &self,
        scope: &OrganizationScope,
        command: PublishPartitionManifest,
    ) -> Result<u64> {
        manifest::publish(&self.pool, scope, command).await
    }

    async fn claim_gc_entries(
        &self,
        scope: &OrganizationScope,
        now_micros: i64,
        lease_expired_before_micros: i64,
        limit: u32,
    ) -> Result<Vec<GcQueueEntry>> {
        gc::claim_entries(
            &self.pool,
            scope,
            now_micros,
            lease_expired_before_micros,
            limit,
        )
        .await
    }

    async fn gc_entry_is_safe(
        &self,
        scope: &OrganizationScope,
        entry: &GcQueueEntry,
    ) -> Result<bool> {
        gc::entry_is_safe(&self.pool, scope, entry).await
    }

    async fn complete_gc_entry(
        &self,
        scope: &OrganizationScope,
        object_key: &ObjectKey,
    ) -> Result<()> {
        gc::complete_entry(&self.pool, scope, object_key).await
    }

    async fn retry_gc_entry(
        &self,
        scope: &OrganizationScope,
        object_key: &ObjectKey,
        error: &str,
        not_before_micros: i64,
    ) -> Result<()> {
        gc::retry_entry(&self.pool, scope, object_key, error, not_before_micros).await
    }

    async fn catalog_objects(
        &self,
        scope: &OrganizationScope,
        after: Option<&ObjectKey>,
        limit: u32,
    ) -> Result<Vec<CatalogObject>> {
        maintenance::catalog_objects(&self.pool, scope, after, limit).await
    }

    async fn object_is_referenced(
        &self,
        scope: &OrganizationScope,
        object_key: &ObjectKey,
    ) -> Result<bool> {
        maintenance::object_is_referenced(&self.pool, scope, object_key).await
    }

    async fn enqueue_orphan(
        &self,
        scope: &OrganizationScope,
        object_key: &ObjectKey,
        not_before_micros: i64,
    ) -> Result<()> {
        maintenance::enqueue_orphan(&self.pool, scope, object_key, not_before_micros).await
    }

    async fn catalog_invariant_issues(
        &self,
        scope: &OrganizationScope,
        gc_stuck_before_micros: i64,
    ) -> Result<Vec<CatalogInvariantIssue>> {
        maintenance::catalog_invariant_issues(&self.pool, scope, gc_stuck_before_micros).await
    }
}
