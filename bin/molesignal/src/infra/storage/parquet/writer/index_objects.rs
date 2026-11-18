// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A segment's small index objects, derived from its primary file: the file-level filters of its
//! `bloom` and `ngram` fields (see [`crate::infra::storage::filters`]) and its minute counts (see
//! [`crate::infra::storage::minute_counts`]). They only spare queries work, so a segment whose
//! object cannot be built or uploaded is published without it and read as before.

use bytes::Bytes;
use domain::{
    storage::{
        Artifact, ArtifactId, ArtifactRole, ArtifactState, ArtifactTypeId, Partition,
        PhysicalDataset, SegmentId, StoredObject, type_id,
    },
    stream::StreamDefinition,
};
use object_store::ObjectStore;

use super::catalog::{checksum, put_immutable};
use crate::infra::storage::{
    filters::SegmentFilters,
    layout::StorageLayout,
    minute_counts::{FORMAT_VERSION as MINUTE_COUNTS_FORMAT, MinuteCounts},
};

/// Format of the filter objects written now.
const FILTERS_FORMAT: u32 = 1;

/// The segment the objects belong to, and where they go.
pub(super) struct SegmentObjects<'a> {
    pub(super) store: &'a dyn ObjectStore,
    pub(super) stream: &'a StreamDefinition,
    pub(super) dataset: &'a PhysicalDataset,
    pub(super) partition: &'a Partition,
    pub(super) segment_id: &'a SegmentId,
    pub(super) primary: &'a Artifact,
}

impl SegmentObjects<'_> {
    /// Uploads the filters gathered for the segment and returns their artifact, if any field got
    /// one.
    pub(super) async fn publish_filters(&self, filters: SegmentFilters) -> Option<Artifact> {
        let filters = filters.finish(self.segment_id.as_str())?;

        match filters.encode() {
            Ok(bytes) => {
                self.publish(type_id::builtin::ARTIFACT_BLOOM, FILTERS_FORMAT, bytes)
                    .await
            }
            Err(error) => {
                tracing::warn!(segment_id = %self.segment_id, %error, "segment filters not encoded");
                None
            }
        }
    }

    /// Uploads the segment's minute counts and returns their artifact.
    pub(super) async fn publish_minute_counts(&self, counts: MinuteCounts) -> Option<Artifact> {
        match counts.encode() {
            Ok(bytes) => {
                self.publish(
                    type_id::builtin::ARTIFACT_MINUTE_COUNTS,
                    MINUTE_COUNTS_FORMAT,
                    bytes,
                )
                .await
            }
            Err(error) => {
                tracing::warn!(segment_id = %self.segment_id, %error, "minute counts not encoded");
                None
            }
        }
    }

    async fn publish(
        &self,
        artifact_type: &'static str,
        format_version: u32,
        bytes: Bytes,
    ) -> Option<Artifact> {
        let artifact_id = ArtifactId::generate();
        let artifact_type = ArtifactTypeId::builtin(artifact_type);

        let key = StorageLayout::artifact_key(
            &self.dataset.organization_id,
            self.stream.stream_type,
            &self.dataset.id,
            self.partition,
            self.segment_id,
            &artifact_id,
            &artifact_type,
        );

        let size_bytes = bytes.len() as u64;
        let object_checksum = checksum(&bytes);

        let put = match put_immutable(self.store, &key, bytes).await {
            Ok(put) => put,
            Err(error) => {
                tracing::warn!(
                    segment_id = %self.segment_id,
                    artifact_type = artifact_type.as_str(),
                    %error,
                    "segment index object not uploaded"
                );
                return None;
            }
        };

        Some(Artifact {
            id: artifact_id,
            role: ArtifactRole::Index,
            artifact_type,
            format_version,
            object: StoredObject {
                key,
                size_bytes,
                checksum: object_checksum,
                etag: put.e_tag,
            },
            source_artifact_id: Some(self.primary.id.clone()),
            source_checksum: Some(self.primary.object.checksum.clone()),
            schema_fingerprint: self.primary.schema_fingerprint,
            state: ArtifactState::Ready,
            failure_reason: None,
        })
    }
}
