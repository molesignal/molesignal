// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! FileCatalog port：所有持久化文件元数据的唯一事实来源。
//!
//! 层级：LogicalStream → PhysicalDataset → DataSegment → Artifact → StoredObject。
//! 它不读 WAL payload、不执行查询、不解析文件、不管理缓存块，也不理解
//! logs / metrics / traces 的业务语义。
//!
//! 查询侧一次 [`FileCatalog::snapshot`] 可跨多个 Dataset 取得同一事务内的一致
//! 视图；拿到 Snapshot 后不再访问活动 Catalog 状态，防止查询期间 compaction
//! 改变文件集合。

use async_trait::async_trait;

use super::{
    artifact::{Artifact, StoredObject},
    dataset::{PhysicalDataset, PhysicalDatasetSpec},
    ids::{
        ArtifactId, ObjectChecksum, ObjectKey, PhysicalDatasetId, SegmentId, WalSequence,
        WriterEpoch, WriterNodeId,
    },
    manifest::{CatalogObject, PartitionManifestPointer, PublishPartitionManifest},
    segment::{DataSegment, Partition},
    wal::FlushProvenance,
};
use crate::shared::{Result, ids::Id, time::TimeRange};

/// 租户作用域。所有 Catalog 操作显式携带，杜绝只凭 dataset_id 绕过组织过滤。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OrganizationScope {
    pub organization_id: Id,
}

impl OrganizationScope {
    pub fn new(organization_id: Id) -> Self {
        Self { organization_id }
    }
}

/// 一次快照要读取的数据集与范围。跨 Dataset（如 metrics 的 samples + rollup）
/// 必须放进同一个 Selection，避免分别查询产生版本错位。
#[derive(Debug, Clone)]
pub struct DatasetSelection {
    pub dataset_ids: Vec<PhysicalDatasetId>,
    pub time_range: TimeRange,
    /// 限定水平分片；None = 全部。
    pub partition_shard: Option<u16>,
}

/// 某 (writer node, epoch) 已提交进 Catalog 的最高 WAL 序号。
///
/// 序号空间按 (node, epoch) 隔离，因此这是集合而非单值；查询侧做 Buffer 去重时
/// 只对比自己节点 + 当前 epoch 的检查点。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WalCheckpointView {
    pub writer_node_id: WriterNodeId,
    pub writer_epoch: WriterEpoch,
    pub committed_sequence: WalSequence,
}

/// 单个 Dataset 在快照时刻的完整可见状态。
#[derive(Debug, Clone)]
pub struct DatasetSnapshot {
    pub dataset_id: PhysicalDatasetId,
    pub catalog_version: u64,
    pub wal_checkpoints: Vec<WalCheckpointView>,
    /// 快照时刻 Active、且与选择范围相交的 Segment（含全部 Artifact 行）。
    pub segments: Vec<DataSegment>,
    /// Active immutable sealed bases. Hot `segments` are overlays on top of these pointers.
    pub manifests: Vec<PartitionManifestPointer>,
}

impl DatasetSnapshot {
    /// 指定 (node, epoch) 的已提交序号；无记录视为 0（全部未提交）。
    pub fn committed_sequence(&self, node: &WriterNodeId, epoch: WriterEpoch) -> WalSequence {
        self.wal_checkpoints
            .iter()
            .find(|c| c.writer_node_id == *node && c.writer_epoch == epoch)
            .map(|c| c.committed_sequence)
            .unwrap_or_default()
    }
}

/// 跨 Dataset 的一致性快照。
#[derive(Debug, Clone, Default)]
pub struct CatalogSnapshot {
    pub datasets: Vec<DatasetSnapshot>,
}

impl CatalogSnapshot {
    pub fn dataset(&self, id: &PhysicalDatasetId) -> Option<&DatasetSnapshot> {
        self.datasets.iter().find(|d| d.dataset_id == *id)
    }
}

/// flush 发布命令：一笔事务写入 flush 溯源、Segment、Artifact，推进 WAL
/// checkpoint 并递增 catalog_version。`segments` 内各 Segment 的
/// `flush_id` / `sequence_range` / `output_ordinal` 必须与溯源一致。
#[derive(Debug, Clone)]
pub struct CommitFlush {
    pub dataset_id: PhysicalDatasetId,
    pub provenance: FlushProvenance,
    pub segments: Vec<DataSegment>,
}

/// flush 提交结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlushCommitResult {
    pub catalog_version: u64,
    /// true = 该 flush_id 此前已提交（幂等重试），本次未写入任何行。
    pub already_committed: bool,
}

/// compaction 替换命令：全有全无——只要有一个被替换 Segment 已不是 Active
/// 就整体失败返回 Conflict，冲突方必须删除自己刚写出的新对象。
#[derive(Debug, Clone)]
pub struct ReplaceSegments {
    pub dataset_id: PhysicalDatasetId,
    pub replaced: Vec<SegmentId>,
    pub replacements: Vec<DataSegment>,
    /// 被替换对象允许物理删除的最早时刻（延迟 GC 下限）。
    pub gc_not_before_micros: i64,
}

/// 跨 Dataset 的原子变换发布（例如 metrics raw → rollup）。输入退役与输出可见
/// 必须在同一事务完成，跨 Dataset 查询的 repeatable-read Snapshot 因而只会看到
/// 变换前或变换后，不会同时读取两份逻辑等价数据。
#[derive(Debug, Clone)]
pub struct PublishDatasetTransform {
    pub input_dataset_id: PhysicalDatasetId,
    pub input_partition: Partition,
    /// Present when the transformed input includes a sealed base. The transaction must retire
    /// exactly this generation; a concurrent generation switch makes the command conflict.
    pub input_manifest: Option<PartitionManifestPointer>,
    pub input_segment_ids: Vec<SegmentId>,
    pub output_dataset_id: PhysicalDatasetId,
    pub output_segments: Vec<DataSegment>,
    pub gc_not_before_micros: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DatasetTransformResult {
    pub input_catalog_version: u64,
    pub output_catalog_version: u64,
}

/// Artifact 状态更新（索引构建完成 / 失败 / 重建替换）。
#[derive(Debug, Clone)]
pub enum ArtifactUpdate {
    /// Pending → Ready，登记最终对象信息。
    MarkReady { object: StoredObject },
    /// Pending → Failed。
    MarkFailed { reason: String },
    /// Failed → Pending（rebuild worker 重试前复位）。
    Retry,
    /// 用重建产物替换：旧 Artifact 置 Tombstoned 入延迟 GC，插入新行。
    /// 新行的 role / artifact_type 槽位必须与旧行一致。
    Replace {
        replacement: Artifact,
        gc_not_before_micros: i64,
    },
}

#[derive(Debug, Clone)]
pub struct UpdateArtifact {
    pub dataset_id: PhysicalDatasetId,
    pub segment_id: SegmentId,
    pub artifact_id: ArtifactId,
    pub update: ArtifactUpdate,
}

/// retention / 显式删除：只改 Catalog 状态并入 GC 队列，不直接删对象。
#[derive(Debug, Clone)]
pub struct TombstoneSegments {
    pub dataset_id: PhysicalDatasetId,
    pub segment_ids: Vec<SegmentId>,
    pub gc_not_before_micros: i64,
}

/// 对象进入 GC 队列的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GcReason {
    Replaced,
    Tombstoned,
    IndexRebuilt,
    Orphan,
}

impl GcReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Replaced => "replaced",
            Self::Tombstoned => "tombstoned",
            Self::IndexRebuilt => "index_rebuilt",
            Self::Orphan => "orphan",
        }
    }
}

/// 待延迟删除的对象（GC 队列行的领域视图）。
#[derive(Debug, Clone)]
pub struct GcQueueEntry {
    pub organization_id: Id,
    pub object_key: ObjectKey,
    pub checksum: Option<ObjectChecksum>,
    pub reason: GcReason,
    pub not_before_micros: i64,
    pub attempt_count: u32,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct IndexRebuildTask {
    pub dataset: PhysicalDataset,
    pub segment: DataSegment,
    pub artifact: Artifact,
}

/// Catalog-only invariants checked without reading object payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogInvariant {
    ActiveSegmentMissingReadyPrimary,
    WalCheckpointFlushMismatch,
    GcStuck,
    StaleIndexSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogInvariantIssue {
    pub invariant: CatalogInvariant,
    pub occurrences: u64,
}

/// 持久化文件元数据的唯一入口。实现位于 PostgreSQL engine。
#[async_trait]
pub trait FileCatalog: Send + Sync {
    /// 幂等建集：`(org, logical_stream, dataset_type)` 已存在的直接返回现有行。
    async fn ensure_datasets(
        &self,
        scope: &OrganizationScope,
        logical_stream_id: &Id,
        specs: &[PhysicalDatasetSpec],
    ) -> Result<Vec<PhysicalDataset>>;

    async fn list_datasets(
        &self,
        scope: &OrganizationScope,
        logical_stream_id: &Id,
    ) -> Result<Vec<PhysicalDataset>>;

    /// 单一一致性事务内读取全部所选 Dataset 的版本、checkpoint 与 Segment。
    async fn snapshot(
        &self,
        scope: &OrganizationScope,
        selection: DatasetSelection,
    ) -> Result<CatalogSnapshot>;

    async fn commit_flush(
        &self,
        scope: &OrganizationScope,
        command: CommitFlush,
    ) -> Result<FlushCommitResult>;

    /// 返回新的 catalog_version。
    async fn replace_segments(
        &self,
        scope: &OrganizationScope,
        command: ReplaceSegments,
    ) -> Result<u64>;

    async fn publish_dataset_transform(
        &self,
        _scope: &OrganizationScope,
        _command: PublishDatasetTransform,
    ) -> Result<DatasetTransformResult> {
        Err(crate::shared::Error::unavailable(
            "cross-dataset transforms are not supported by this FileCatalog",
        ))
    }

    async fn update_artifact(
        &self,
        scope: &OrganizationScope,
        command: UpdateArtifact,
    ) -> Result<()>;

    /// 返回新的 catalog_version。
    async fn tombstone_segments(
        &self,
        scope: &OrganizationScope,
        command: TombstoneSegments,
    ) -> Result<u64>;

    /// WAL 恢复用：某 Dataset 全部 (node, epoch) 的已提交序号。
    async fn wal_checkpoints(
        &self,
        scope: &OrganizationScope,
        dataset_id: &PhysicalDatasetId,
    ) -> Result<Vec<WalCheckpointView>>;

    /// Pending/Failed optional indexes eligible for a background rebuild.
    async fn index_rebuild_tasks(
        &self,
        _scope: &OrganizationScope,
        _updated_before_micros: i64,
        _limit: u32,
    ) -> Result<Vec<IndexRebuildTask>> {
        Ok(Vec::new())
    }

    /// Atomically switch the active manifest generation and seal/tombstone its input rows.
    async fn publish_partition_manifest(
        &self,
        _scope: &OrganizationScope,
        _command: PublishPartitionManifest,
    ) -> Result<u64> {
        Err(crate::shared::Error::unavailable(
            "partition manifests are not supported by this FileCatalog",
        ))
    }

    /// Claim due GC rows. `lease_expired_before_micros` recovers abandoned claims.
    async fn claim_gc_entries(
        &self,
        _scope: &OrganizationScope,
        _now_micros: i64,
        _lease_expired_before_micros: i64,
        _limit: u32,
    ) -> Result<Vec<GcQueueEntry>> {
        Ok(Vec::new())
    }

    /// Final pre-delete guard against any live Artifact, manifest, or index-source reference.
    async fn gc_entry_is_safe(
        &self,
        _scope: &OrganizationScope,
        _entry: &GcQueueEntry,
    ) -> Result<bool> {
        Ok(false)
    }

    async fn complete_gc_entry(
        &self,
        _scope: &OrganizationScope,
        _object_key: &ObjectKey,
    ) -> Result<()> {
        Ok(())
    }

    async fn retry_gc_entry(
        &self,
        _scope: &OrganizationScope,
        _object_key: &ObjectKey,
        _error: &str,
        _not_before_micros: i64,
    ) -> Result<()> {
        Ok(())
    }

    /// Objects that should exist for active Catalog state, used by the reconciler.
    async fn catalog_objects(
        &self,
        _scope: &OrganizationScope,
        _after: Option<&ObjectKey>,
        _limit: u32,
    ) -> Result<Vec<CatalogObject>> {
        Ok(Vec::new())
    }

    async fn object_is_referenced(
        &self,
        _scope: &OrganizationScope,
        _object_key: &ObjectKey,
    ) -> Result<bool> {
        Ok(true)
    }

    async fn enqueue_orphan(
        &self,
        _scope: &OrganizationScope,
        _object_key: &ObjectKey,
        _not_before_micros: i64,
    ) -> Result<()> {
        Ok(())
    }

    /// Bounded aggregate invariant checks used by StorageReconciler.
    async fn catalog_invariant_issues(
        &self,
        _scope: &OrganizationScope,
        _gc_stuck_before_micros: i64,
    ) -> Result<Vec<CatalogInvariantIssue>> {
        Ok(Vec::new())
    }
}
