// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Partition-local greedy compaction grouping.

use std::collections::BTreeMap;

use crate::{
    domain::storage::{DataSegment, Partition, PhysicalDatasetId},
    shared::{Error, Result},
};

type PartitionKey = (i64, i64, u16);

pub(super) fn build_groups(
    segments: impl IntoIterator<Item = DataSegment>,
    target_bytes: u64,
) -> (Vec<Vec<DataSegment>>, Vec<DataSegment>) {
    let mut partitions: BTreeMap<PartitionKey, Vec<DataSegment>> = BTreeMap::new();
    let mut invalid = Vec::new();
    for segment in segments {
        if !time_range_within_partition(&segment) {
            invalid.push(segment);
            continue;
        }
        let partition = segment.partition;
        partitions
            .entry((
                partition.start_micros,
                partition.end_micros,
                partition.shard,
            ))
            .or_default()
            .push(segment);
    }

    let mut groups = Vec::new();
    for segments in partitions.values_mut() {
        segments.sort_by(|left, right| {
            left.time_range
                .start
                .cmp(&right.time_range.start)
                .then_with(|| left.id.as_str().cmp(right.id.as_str()))
        });
        let mut current = Vec::new();
        let mut size = 0_u64;
        for segment in segments.drain(..) {
            let segment_size = segment.primary.object.size_bytes;
            if !current.is_empty() && size.saturating_add(segment_size) > target_bytes {
                if current.len() >= 2 {
                    groups.push(std::mem::take(&mut current));
                } else {
                    current.clear();
                }
                size = 0;
            }
            size = size.saturating_add(segment_size);
            current.push(segment);
        }
        if current.len() >= 2 {
            groups.push(current);
        }
    }
    (groups, invalid)
}

pub(super) fn validate_group(
    group: &[DataSegment],
    dataset_id: &PhysicalDatasetId,
) -> Result<Partition> {
    let first = group
        .first()
        .ok_or_else(|| Error::invalid("compactor group is empty"))?;
    for segment in group {
        if segment.dataset_id != *dataset_id
            || segment.organization_id != first.organization_id
            || segment.partition != first.partition
            || !time_range_within_partition(segment)
        {
            return Err(Error::invalid(
                "compactor refuses a group spanning dataset, organization, or partition",
            ));
        }
    }
    Ok(first.partition)
}

fn time_range_within_partition(segment: &DataSegment) -> bool {
    segment.partition.start_micros <= segment.time_range.start.0
        && segment.time_range.end.0 < segment.partition.end_micros
        && segment.time_range.start.0 <= segment.time_range.end.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::storage::{
            Artifact, ArtifactId, ArtifactRole, ArtifactState, ArtifactTypeId, ColumnStats,
            ObjectChecksum, ObjectKey, SchemaFingerprint, SegmentId, SegmentState, StoredObject,
            type_id,
        },
        shared::{
            ids::Id,
            time::{TimeRange, TimestampMicros},
        },
    };

    fn segment(
        id: &str,
        dataset: &PhysicalDatasetId,
        partition_start: i64,
        size: u64,
    ) -> DataSegment {
        let partition = Partition {
            start_micros: partition_start,
            end_micros: partition_start + 3_600_000_000,
            shard: 0,
        };
        DataSegment {
            id: SegmentId::from_string(id),
            organization_id: Id::from_string("org"),
            dataset_id: dataset.clone(),
            partition,
            time_range: TimeRange::new(
                TimestampMicros(partition_start + 1),
                TimestampMicros(partition_start + 2),
            ),
            sequence_range: None,
            row_count: 1,
            schema_fingerprint: Some(SchemaFingerprint(1)),
            column_stats: ColumnStats::default(),
            flush_id: None,
            output_ordinal: 0,
            primary: Artifact {
                id: ArtifactId::generate(),
                role: ArtifactRole::PrimaryData,
                artifact_type: ArtifactTypeId::builtin(type_id::builtin::ARTIFACT_PARQUET),
                format_version: 1,
                object: StoredObject {
                    key: ObjectKey::from_string(format!("objects/{id}")),
                    size_bytes: size,
                    checksum: ObjectChecksum::from_string("b3:test"),
                    etag: None,
                },
                source_artifact_id: None,
                source_checksum: None,
                schema_fingerprint: Some(SchemaFingerprint(1)),
                state: ArtifactState::Ready,
                failure_reason: None,
            },
            auxiliaries: Vec::new(),
            state: SegmentState::Active,
            visible_from_version: 1,
            retired_at_version: None,
            created_at_micros: 0,
        }
    }

    #[test]
    fn never_groups_across_catalog_partitions() {
        let dataset = PhysicalDatasetId::from_string("dataset");
        let segments = vec![
            segment("a", &dataset, 0, 10),
            segment("b", &dataset, 0, 10),
            segment("c", &dataset, 3_600_000_000, 10),
            segment("d", &dataset, 3_600_000_000, 10),
        ];
        let (groups, invalid) = build_groups(segments, 100);
        assert!(invalid.is_empty());
        assert_eq!(groups.len(), 2);
        assert!(
            groups
                .iter()
                .all(|group| validate_group(group, &dataset).is_ok())
        );
    }

    #[test]
    fn rejects_segment_outside_declared_partition() {
        let dataset = PhysicalDatasetId::from_string("dataset");
        let mut invalid = segment("x", &dataset, 0, 10);
        invalid.time_range.end = TimestampMicros(3_600_000_000);
        let (groups, invalid) = build_groups(vec![invalid], 100);
        assert!(groups.is_empty());
        assert_eq!(invalid.len(), 1);
    }
}
