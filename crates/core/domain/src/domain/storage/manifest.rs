// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Immutable cold-partition manifest domain model.

use serde::{Deserialize, Serialize};

use super::{DataSegment, Partition, PhysicalDatasetId, StoredObject};
use crate::shared::ids::Id;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PartitionManifest {
    pub organization_id: Id,
    pub dataset_id: PhysicalDatasetId,
    pub partition: Partition,
    pub generation: u64,
    pub segments: Vec<DataSegment>,
}

impl PartitionManifest {
    pub fn validate(&self) -> crate::shared::Result<()> {
        if self.generation == 0
            || self.partition.start_micros >= self.partition.end_micros
            || self.segments.is_empty()
        {
            return Err(crate::shared::Error::invalid(
                "partition manifest requires a positive generation, valid partition, and segments",
            ));
        }
        let mut segment_ids = std::collections::HashSet::with_capacity(self.segments.len());
        for segment in &self.segments {
            segment.validate()?;
            if segment.organization_id != self.organization_id
                || segment.dataset_id != self.dataset_id
                || segment.partition != self.partition
                || segment.state != super::SegmentState::Sealed
                || !segment_ids.insert(segment.id.clone())
            {
                return Err(crate::shared::Error::invalid(format!(
                    "segment {} is duplicated, unsealed, or does not belong to manifest generation {}",
                    segment.id, self.generation
                )));
            }
        }
        Ok(())
    }
}

/// PostgreSQL keeps only this active pointer for a sealed partition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartitionManifestPointer {
    pub organization_id: Id,
    pub dataset_id: PhysicalDatasetId,
    pub partition: Partition,
    pub generation: u64,
    pub object: StoredObject,
    pub segment_count: u32,
    pub created_at_micros: i64,
}

impl PartitionManifestPointer {
    pub fn cache_key(&self) -> String {
        format!(
            "{}\0{}",
            self.object.key.as_str(),
            self.object.checksum.as_str()
        )
    }
}

/// Atomic pointer switch. `new_manifest = None` retires a partition completely (retention).
#[derive(Debug, Clone)]
pub struct PublishPartitionManifest {
    pub dataset_id: PhysicalDatasetId,
    pub partition: Partition,
    pub expected_generation: Option<u64>,
    pub new_manifest: Option<PartitionManifestPointer>,
    /// Active overlay rows included in the new immutable generation.
    pub seal_segment_ids: Vec<super::SegmentId>,
    /// Rows excluded by retention; their objects enter delayed GC.
    pub tombstone_segment_ids: Vec<super::SegmentId>,
    pub gc_not_before_micros: i64,
}

/// Reconciler uses this without knowing an Artifact's concrete reader.
#[derive(Debug, Clone)]
pub struct CatalogObject {
    pub dataset_id: PhysicalDatasetId,
    pub segment_id: Option<super::SegmentId>,
    pub artifact_id: Option<super::ArtifactId>,
    pub role: Option<super::ArtifactRole>,
    pub state: Option<super::ArtifactState>,
    pub object: StoredObject,
    pub manifest: Option<PartitionManifestPointer>,
}
