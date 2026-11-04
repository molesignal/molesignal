// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Partition-local greedy compaction grouping. Only segments written with the same field partition
//! (or none) are merged: the merged segment records that partition for every one of its rows.

use std::collections::BTreeMap;

use crate::{
    domain::{
        storage::{DataSegment, Partition, PhysicalDatasetId},
        stream::FieldPartitions,
    },
    shared::{Error, Result},
};

type PartitionKey = (i64, i64, u16, FieldPartitions);

/// Most inputs one merge reads. A merge holds a window of every input at once; the segments
/// past this many are merged with the result in a later round.
const MAX_GROUP_SEGMENTS: usize = 64;

const DAY_MICROS: i64 = 24 * 60 * 60 * 1_000_000;

/// How long after a day ends its segments are merged: late rows of the last hours have arrived.
const DAY_SETTLE_MICROS: i64 = 60 * 60 * 1_000_000;

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
                segment.column_stats.partitions.clone(),
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

            let full = current.len() >= MAX_GROUP_SEGMENTS
                || size.saturating_add(segment_size) > target_bytes;

            if !current.is_empty() && full {
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
            || segment.column_stats.partitions != first.column_stats.partitions
            || !time_range_within_partition(segment)
        {
            return Err(Error::invalid(
                "compactor refuses a group spanning dataset, organization, partition, or field partition",
            ));
        }
    }

    Ok(first.partition)
}

/// The partition fields every segment of `group` was written with; none unless they share them.
pub(super) fn common_partitions(group: &[DataSegment]) -> FieldPartitions {
    let Some(first) = group.first() else {
        return FieldPartitions::default();
    };

    let partitions = &first.column_stats.partitions;

    if group
        .iter()
        .all(|segment| segment.column_stats.partitions == *partitions)
    {
        partitions.clone()
    } else {
        FieldPartitions::default()
    }
}

/// The days to merge into one segment each, with the segments of each: a day that ended at least
/// [`DAY_SETTLE_MICROS`] before `now`, whose segments lie in more than one partition inside it
/// and together fit in `target_bytes`. A stream that writes little then keeps one file a day
/// instead of one an hour, so a query over weeks opens a few dozen files instead of hundreds.
pub(super) fn daily_groups(
    segments: impl IntoIterator<Item = DataSegment>,
    target_bytes: u64,
    now: i64,
) -> Vec<(Partition, Vec<DataSegment>)> {
    let mut days: BTreeMap<(i64, u16, FieldPartitions), Vec<DataSegment>> = BTreeMap::new();

    for segment in segments {
        let day = segment.partition.start_micros.div_euclid(DAY_MICROS) * DAY_MICROS;

        let inside = time_range_within_partition(&segment)
            && segment.partition.end_micros <= day.saturating_add(DAY_MICROS)
            && day.saturating_add(DAY_MICROS) <= now.saturating_sub(DAY_SETTLE_MICROS);

        if inside {
            days.entry((
                day,
                segment.partition.shard,
                segment.column_stats.partitions.clone(),
            ))
            .or_default()
            .push(segment);
        }
    }

    days.into_iter()
        .filter_map(|((day, shard, _), mut segments)| {
            let partitions = segments
                .iter()
                .map(|segment| segment.partition)
                .collect::<std::collections::HashSet<_>>();

            let size = segments
                .iter()
                .map(|segment| segment.primary.object.size_bytes)
                .fold(0_u64, u64::saturating_add);

            if segments.len() < 2 || partitions.len() < 2 || size > target_bytes {
                return None;
            }

            segments.sort_by(|left, right| {
                left.time_range
                    .start
                    .cmp(&right.time_range.start)
                    .then_with(|| left.id.as_str().cmp(right.id.as_str()))
            });

            segments.truncate(MAX_GROUP_SEGMENTS);

            let partition = Partition {
                start_micros: day,
                end_micros: day + DAY_MICROS,
                shard,
            };

            Some((partition, segments))
        })
        .collect()
}

/// Every segment of `group` belongs to `dataset_id` and one organization and lies inside `day`,
/// in the same shard.
pub(super) fn validate_day(
    group: &[DataSegment],
    dataset_id: &PhysicalDatasetId,
    day: Partition,
) -> Result<()> {
    let first = group
        .first()
        .ok_or_else(|| Error::invalid("compactor group is empty"))?;

    let fits = group.iter().all(|segment| {
        segment.dataset_id == *dataset_id
            && segment.organization_id == first.organization_id
            && segment.partition.shard == day.shard
            && segment.column_stats.partitions == first.column_stats.partitions
            && day.start_micros <= segment.partition.start_micros
            && segment.partition.end_micros <= day.end_micros
            && time_range_within_partition(segment)
    });

    if !fits {
        return Err(Error::invalid(
            "compactor refuses a daily group spanning dataset, organization, shard or day",
        ));
    }

    Ok(())
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
        domain::{
            storage::{
                Artifact, ArtifactId, ArtifactRole, ArtifactState, ArtifactTypeId, ColumnStats,
                ObjectChecksum, ObjectKey, SchemaFingerprint, SegmentId, SegmentState,
                StoredObject, type_id,
            },
            stream::{FieldPartition, PartitionScheme},
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

    /// Old unpartitioned files and files of a field partition share shard 0, but merged they would
    /// claim the partition for rows that never followed it.
    #[test]
    fn never_groups_across_field_partitions() {
        let dataset = PhysicalDatasetId::from_string("dataset");
        let partitioned = FieldPartitions::new(vec![FieldPartition {
            field: "service".into(),
            scheme: PartitionScheme::Hash { buckets: 4 },
        }]);

        let mut segments = vec![
            segment("a", &dataset, 0, 10),
            segment("b", &dataset, 0, 10),
            segment("c", &dataset, 0, 10),
            segment("d", &dataset, 0, 10),
        ];

        for segment in &mut segments[2..] {
            segment.column_stats.partitions = partitioned.clone();
        }

        let (groups, _) = build_groups(segments, 100);

        assert_eq!(groups.len(), 2);
        assert!(
            groups
                .iter()
                .all(|group| validate_group(group, &dataset).is_ok())
        );
        assert_eq!(
            groups
                .iter()
                .map(|group| common_partitions(group))
                .collect::<Vec<_>>(),
            [FieldPartitions::default(), partitioned]
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

    #[test]
    fn small_finished_days_merge_across_their_hours() {
        let dataset = PhysicalDatasetId::from_string("dataset");
        let hour = 3_600_000_000;
        let day = DAY_MICROS;

        let segments = vec![
            // Day 0: three hours, small together.
            segment("a", &dataset, 0, 10),
            segment("b", &dataset, hour, 10),
            segment("c", &dataset, 5 * hour, 10),
            // Day 1: one hour only, already one file.
            segment("d", &dataset, day, 10),
            // Day 2: two hours too large together.
            segment("e", &dataset, 2 * day, 60),
            segment("f", &dataset, 2 * day + hour, 60),
            // Day 3: not over long enough.
            segment("g", &dataset, 3 * day, 10),
            segment("h", &dataset, 3 * day + hour, 10),
        ];

        let groups = daily_groups(segments, 100, 4 * day);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].0.start_micros, 0);
        assert_eq!(groups[0].0.end_micros, day);
        assert_eq!(
            groups[0]
                .1
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
        assert!(validate_day(&groups[0].1, &dataset, groups[0].0).is_ok());
        assert!(
            validate_day(
                &groups[0].1,
                &dataset,
                Partition {
                    start_micros: day,
                    end_micros: 2 * day,
                    shard: 0,
                }
            )
            .is_err()
        );
    }

    #[test]
    fn a_group_reads_at_most_max_group_segments() {
        let dataset = PhysicalDatasetId::from_string("dataset");

        let segments = (0..2 * MAX_GROUP_SEGMENTS + 1)
            .map(|index| segment(&format!("s{index:03}"), &dataset, 0, 1))
            .collect::<Vec<_>>();

        let (groups, _) = build_groups(segments, u64::MAX);

        assert_eq!(
            groups.iter().map(Vec::len).collect::<Vec<_>>(),
            vec![MAX_GROUP_SEGMENTS, MAX_GROUP_SEGMENTS]
        );
    }
}
