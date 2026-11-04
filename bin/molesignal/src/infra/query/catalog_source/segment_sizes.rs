// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What a stream's datasets hold in a time range, read from the catalog and the sealed manifests
//! alone: no data file is opened. For a question about files rather than about the rows in them.

use std::collections::HashMap;

use futures::{StreamExt, TryStreamExt};

use super::CatalogQuerySource;
use crate::{
    domain::{
        storage::{
            DataSegment, DatasetSelection, DatasetSnapshot, DatasetState, OrganizationScope,
            PhysicalDataset, PhysicalDatasetId,
        },
        stream::StreamDefinition,
    },
    shared::{Result, ids::Id, time::TimeRange},
};

/// Sealed manifests of one dataset read at once. They are small objects, most of them cached.
const MANIFEST_READS: usize = 8;

/// What goes when a segment goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentSize {
    pub time_range: TimeRange,
    pub rows: u64,
    /// Its data file and every file derived from it, such as its indexes.
    pub stored_bytes: u64,
}

impl From<&DataSegment> for SegmentSize {
    fn from(segment: &DataSegment) -> Self {
        Self {
            time_range: segment.time_range,
            rows: segment.row_count,
            stored_bytes: segment
                .auxiliaries
                .iter()
                .fold(segment.primary.object.size_bytes, |total, artifact| {
                    total.saturating_add(artifact.object.size_bytes)
                }),
        }
    }
}

impl CatalogQuerySource {
    /// Every active dataset of a stream, whatever its type: retention applies to all of them, not
    /// only to the ones a query reads.
    pub async fn active_datasets(&self, stream: &StreamDefinition) -> Result<Vec<PhysicalDataset>> {
        Ok(self
            .catalog
            .list_datasets(&OrganizationScope::new(stream.org_id.clone()), &stream.id)
            .await?
            .into_iter()
            .filter(|dataset| dataset.state == DatasetState::Active)
            .collect())
    }

    /// The segments of `dataset_ids` that overlap `range`, hot and sealed alike, by dataset. A
    /// sealed manifest that cannot be read fails its own dataset; the others still answer.
    pub async fn segment_sizes(
        &self,
        organization_id: &Id,
        dataset_ids: Vec<PhysicalDatasetId>,
        range: TimeRange,
    ) -> Result<HashMap<PhysicalDatasetId, Result<Vec<SegmentSize>>>> {
        if dataset_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let snapshot = self
            .catalog
            .snapshot(
                &OrganizationScope::new(organization_id.clone()),
                DatasetSelection {
                    dataset_ids,
                    time_range: range,
                    partition_shard: None,
                },
            )
            .await?;

        let mut sizes = HashMap::with_capacity(snapshot.datasets.len());

        for dataset in &snapshot.datasets {
            sizes.insert(
                dataset.dataset_id.clone(),
                self.dataset_segment_sizes(dataset, range).await,
            );
        }

        Ok(sizes)
    }

    async fn dataset_segment_sizes(
        &self,
        dataset: &DatasetSnapshot,
        range: TimeRange,
    ) -> Result<Vec<SegmentSize>> {
        // The closure takes each pointer by value: one over a reference would be higher-ranked, and
        // a handler's future cannot be shown `Send` through that.
        let manifests = futures::stream::iter(dataset.manifests.iter().cloned())
            .map(|pointer| async move { self.manifests.load(&pointer).await })
            .buffer_unordered(MANIFEST_READS)
            .try_collect::<Vec<_>>()
            .await?;

        let sealed = manifests
            .iter()
            .flat_map(|manifest| &manifest.segments)
            .filter(|segment| segment.time_range.overlaps(range));

        Ok(dataset
            .segments
            .iter()
            .chain(sealed)
            .map(SegmentSize::from)
            .collect())
    }
}
