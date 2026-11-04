// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A segment's filter artifact: the file-level filters of its `bloom` and `ngram` fields (see
//! [`crate::infra::storage::filters`]). The filters only let queries skip the file; a segment
//! whose filters cannot be built or uploaded is published without them and read as before.

use object_store::ObjectStore;

use super::catalog::{checksum, put_immutable};
use crate::{
    domain::{
        storage::{
            Artifact, ArtifactId, ArtifactRole, ArtifactState, ArtifactTypeId, Partition,
            PhysicalDataset, SegmentId, StoredObject, type_id,
        },
        stream::StreamDefinition,
    },
    infra::storage::{filters::SegmentFilters, layout::StorageLayout},
};

/// Format of the filter objects written now.
const FORMAT_VERSION: u32 = 1;

/// Uploads the filters gathered for the segment and returns their artifact, if any field got one.
pub(super) async fn publish_filters(
    store: &dyn ObjectStore,
    stream: &StreamDefinition,
    dataset: &PhysicalDataset,
    partition: &Partition,
    segment_id: &SegmentId,
    primary: &Artifact,
    filters: SegmentFilters,
) -> Option<Artifact> {
    let filters = filters.finish(segment_id.as_str())?;

    let bytes = match filters.encode() {
        Ok(bytes) => bytes,
        Err(error) => {
            tracing::warn!(segment_id = %segment_id, %error, "segment filters not encoded");
            return None;
        }
    };

    let artifact_id = ArtifactId::generate();
    let artifact_type = ArtifactTypeId::builtin(type_id::builtin::ARTIFACT_BLOOM);

    let key = StorageLayout::artifact_key(
        &dataset.organization_id,
        stream.stream_type,
        &dataset.id,
        partition,
        segment_id,
        &artifact_id,
        &artifact_type,
    );

    let size_bytes = bytes.len() as u64;
    let object_checksum = checksum(&bytes);

    let put = match put_immutable(store, &key, bytes).await {
        Ok(put) => put,
        Err(error) => {
            tracing::warn!(segment_id = %segment_id, %error, "segment filters not uploaded");
            return None;
        }
    };

    Some(Artifact {
        id: artifact_id,
        role: ArtifactRole::Index,
        artifact_type,
        format_version: FORMAT_VERSION,
        object: StoredObject {
            key,
            size_bytes,
            checksum: object_checksum,
            etag: put.e_tag,
        },
        source_artifact_id: Some(primary.id.clone()),
        source_checksum: Some(primary.object.checksum.clone()),
        schema_fingerprint: primary.schema_fingerprint,
        state: ArtifactState::Ready,
        failure_reason: None,
    })
}
