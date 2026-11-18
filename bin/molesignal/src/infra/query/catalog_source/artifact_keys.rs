// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Which of a segment's ready index artifacts a query may read, by the object they are keyed by:
//! each was built from the segment's own primary file.

use std::collections::HashMap;

use domain::storage::{ArtifactRole, ArtifactState, DataSegment, type_id};

use crate::infra::{
    search::tantivy_index,
    storage::minute_counts::{self, MinuteCountsKeys},
};

/// The segment's Tantivy archive of its primary file: primary object key → archive object key.
pub(super) fn segment_tantivy_index(segment: &DataSegment) -> Option<(String, String)> {
    segment
        .auxiliaries
        .iter()
        .find(|artifact| {
            segment.primary.role == ArtifactRole::PrimaryData
                && segment.primary.state == ArtifactState::Ready
                && segment.primary.artifact_type.as_str() == type_id::builtin::ARTIFACT_PARQUET
                && segment.primary.format_version == 1
                && segment.schema_fingerprint == segment.primary.schema_fingerprint
                && artifact.role == ArtifactRole::Index
                && artifact.state == ArtifactState::Ready
                && artifact.artifact_type.as_str() == type_id::builtin::ARTIFACT_TANTIVY
                && (1..=tantivy_index::FORMAT_VERSION).contains(&artifact.format_version)
                && artifact.source_artifact_id.as_ref() == Some(&segment.primary.id)
                && artifact.source_checksum.as_ref() == Some(&segment.primary.object.checksum)
                && artifact.schema_fingerprint == segment.primary.schema_fingerprint
        })
        .map(|artifact| {
            (
                segment.primary.object.key.as_str().to_owned(),
                artifact.object.key.as_str().to_owned(),
            )
        })
}

/// The segment's own ready filter artifact, built from its primary file: segment id → object key.
pub(super) fn segment_filter_key(segment: &DataSegment) -> Option<(String, String)> {
    own_object(segment, type_id::builtin::ARTIFACT_BLOOM, 1)
}

/// Where the segment's ready minute counts, built from its primary file, are kept: segment id →
/// its own object, and the object of its sealed partition in `sealed` (by segment id), which
/// holds them too unless it could not be written.
pub(super) fn segment_minute_counts_keys(
    segment: &DataSegment,
    sealed: &HashMap<String, String>,
) -> Option<(String, MinuteCountsKeys)> {
    let (id, own) = own_object(
        segment,
        type_id::builtin::ARTIFACT_MINUTE_COUNTS,
        minute_counts::FORMAT_VERSION,
    )?;

    let sealed = sealed.get(&id).cloned();

    Some((id, MinuteCountsKeys { own, sealed }))
}

/// The segment's ready index artifact of `artifact_type`, built from its primary file in a format
/// up to `format_version`: segment id → object key.
fn own_object(
    segment: &DataSegment,
    artifact_type: &str,
    format_version: u32,
) -> Option<(String, String)> {
    segment
        .auxiliaries
        .iter()
        .find(|artifact| {
            artifact.role == ArtifactRole::Index
                && artifact.state == ArtifactState::Ready
                && artifact.artifact_type.as_str() == artifact_type
                && artifact.format_version <= format_version
                && artifact.source_artifact_id.as_ref() == Some(&segment.primary.id)
        })
        .map(|artifact| {
            (
                segment.id.as_str().to_owned(),
                artifact.object.key.as_str().to_owned(),
            )
        })
}
