// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 存储上下文。
//!
//! 新模型（统一物理文件目录）：LogicalStream → [`PhysicalDataset`] →
//! [`DataSegment`] → [`Artifact`] → StoredObject，全部关系由 [`FileCatalog`]
//! 显式描述；类型维度经 [`StreamTypeRegistry`] 开放扩展。
//!
//! Query readers consume a read-only Catalog projection; there is no parallel file-metadata
//! repository or dump tier.

mod artifact;
mod catalog;
mod dataset;
mod ids;
mod manifest;
mod query_file;
mod registry;
mod segment;
pub mod type_id;
mod wal;

pub use artifact::{Artifact, ArtifactRole, ArtifactState, StoredObject};
pub use catalog::{
    ArtifactUpdate, CatalogInvariant, CatalogInvariantIssue, CatalogSnapshot, CommitFlush,
    DatasetSelection, DatasetSnapshot, DatasetTransformResult, FileCatalog, FlushCommitResult,
    GcQueueEntry, GcReason, IndexRebuildTask, OrganizationScope, PublishDatasetTransform,
    ReplaceSegments, TombstoneSegments, UpdateArtifact, WalCheckpointView,
};
pub use dataset::{
    DatasetState, IndexPolicy, PartitionGranularity, PartitionPolicy, PhysicalDataset,
    PhysicalDatasetSpec, StoragePolicy,
};
pub use ids::{
    ArtifactId, FlushId, ObjectChecksum, ObjectKey, PhysicalDatasetId, SchemaFingerprint,
    SegmentId, SequenceRange, WalSequence, WriterEpoch, WriterNodeId,
};
pub use manifest::{
    CatalogObject, PartitionManifest, PartitionManifestPointer, PublishPartitionManifest,
};
pub use query_file::{QueryFile, QueryFileSource};
pub use registry::{
    DatasetTypeDescriptor, StreamTypeDescriptor, StreamTypeRegistry, builtin_registry,
    logical_query_dataset_types, primary_dataset_type,
};
pub use segment::{ColumnStats, DataSegment, Partition, SegmentState};
pub use type_id::{ArtifactTypeId, DatasetTypeId, IndexTypeId, StreamTypeId, WalCodecId};
pub use wal::{FlushProvenance, WalStreamId};
