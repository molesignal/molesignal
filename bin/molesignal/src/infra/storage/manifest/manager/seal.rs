// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一轮维护里一个分区要做的事：按保留期删掉过期的段，把可以封存的段写进分区清单。

use domain::storage::{ArtifactState, DataSegment, SegmentId};

use super::PartitionWork;

/// 一个分区这一轮的封存计划。
#[derive(Debug)]
pub(super) struct SealPlan {
    /// 清单里保留期已过的段。
    pub(super) expired_base: Vec<SegmentId>,
    /// 还没封存、保留期已过的段。
    pub(super) expired_overlay: Vec<SegmentId>,
    /// 这一轮写进清单的段。
    pub(super) sealable: Vec<DataSegment>,
}

impl SealPlan {
    /// `work` 这一轮的计划；分区还不冷、或者冷了但这一轮没有要做的事时为 `None`。
    ///
    /// 有清单的分区、或结束时刻早于 `seal_cutoff` 的分区是冷的。冷分区里，写入时刻也早于
    /// `seal_cutoff` 的段才封存：晚到的数据（补写、重放、离线缓冲后才送达）和实时数据一样有
    /// `seal_after_hours` 留给合并，否则每次 flush 写下的小文件会原样封进清单，此后不再合并。
    /// 不封存（`seal_cutoff` 为 `None`）时，有清单的分区新来的段照旧直接写进清单。
    pub(super) fn of(
        work: &PartitionWork,
        seal_cutoff: Option<i64>,
        retention_cutoff: i64,
    ) -> Option<Self> {
        let is_cold = work.pointer.is_some()
            || seal_cutoff.is_some_and(|cutoff| {
                work.overlay
                    .first()
                    .is_some_and(|segment| segment.partition.end_micros <= cutoff)
            });

        if !is_cold {
            return None;
        }

        let expired = |segments: &[DataSegment]| {
            segments
                .iter()
                .filter(|segment| segment.time_range.end.0 <= retention_cutoff)
                .map(|segment| segment.id.clone())
                .collect::<Vec<_>>()
        };

        let sealable = work
            .overlay
            .iter()
            .filter(|segment| {
                segment.time_range.end.0 > retention_cutoff
                    && seal_cutoff.is_none_or(|cutoff| segment.created_at_micros <= cutoff)
                    && segment.primary.state == ArtifactState::Ready
                    && segment
                        .auxiliaries
                        .iter()
                        .all(|artifact| artifact.state == ArtifactState::Ready)
            })
            .cloned()
            .collect::<Vec<_>>();

        let plan = Self {
            expired_base: expired(&work.base),
            expired_overlay: expired(&work.overlay),
            sealable,
        };

        let changed = !plan.sealable.is_empty()
            || !plan.expired_base.is_empty()
            || !plan.expired_overlay.is_empty();

        changed.then_some(plan)
    }
}

#[cfg(test)]
mod tests {
    use domain::storage::{
        Artifact, ArtifactId, ArtifactRole, ArtifactTypeId, ColumnStats, ObjectChecksum, ObjectKey,
        Partition, PhysicalDatasetId, SegmentState, StoredObject, type_id,
    };
    use kernel::{
        ids::Id,
        time::{TimeRange, TimestampMicros},
    };

    use super::*;

    const HOUR: i64 = 3_600_000_000;
    /// 现在减去 `seal_after_hours`。
    const CUTOFF: i64 = 100 * HOUR;
    /// 保留期从这里起算，比所有测试数据都早。
    const RETENTION: i64 = 0;

    /// `hour` 那个小时分区里、在 `written` 写下的一个段。
    fn segment(id: &str, hour: i64, written: i64) -> DataSegment {
        let start = hour * HOUR;

        DataSegment {
            id: SegmentId::from_string(id),
            organization_id: Id::from_string("org"),
            dataset_id: PhysicalDatasetId::from_string("dataset"),
            partition: Partition {
                start_micros: start,
                end_micros: start + HOUR,
                shard: 0,
            },
            time_range: TimeRange::new(TimestampMicros(start + 1), TimestampMicros(start + 2)),
            sequence_range: None,
            row_count: 1,
            schema_fingerprint: None,
            column_stats: ColumnStats::default(),
            flush_id: None,
            output_ordinal: 0,
            primary: Artifact {
                id: ArtifactId::from_string(format!("{id}-data")),
                role: ArtifactRole::PrimaryData,
                artifact_type: ArtifactTypeId::builtin(type_id::builtin::ARTIFACT_PARQUET),
                format_version: 1,
                object: StoredObject {
                    key: ObjectKey::from_string(format!("objects/{id}")),
                    size_bytes: 1,
                    checksum: ObjectChecksum::from_string("b3:test"),
                    etag: None,
                },
                source_artifact_id: None,
                source_checksum: None,
                schema_fingerprint: None,
                state: ArtifactState::Ready,
                failure_reason: None,
            },
            auxiliaries: Vec::new(),
            state: SegmentState::Active,
            visible_from_version: 1,
            retired_at_version: None,
            created_at_micros: written,
        }
    }

    fn work(overlay: Vec<DataSegment>, sealed: bool) -> PartitionWork {
        PartitionWork {
            pointer: sealed.then(|| domain::storage::PartitionManifestPointer {
                organization_id: Id::from_string("org"),
                dataset_id: PhysicalDatasetId::from_string("dataset"),
                partition: overlay[0].partition,
                generation: 1,
                object: StoredObject {
                    key: ObjectKey::from_string("manifests/1.parquet"),
                    size_bytes: 1,
                    checksum: ObjectChecksum::from_string("b3:test"),
                    etag: None,
                },
                segment_count: 1,
                created_at_micros: 0,
            }),
            base: Vec::new(),
            overlay,
        }
    }

    fn sealed_ids(plan: Option<SealPlan>) -> Vec<String> {
        plan.map(|plan| plan.sealable)
            .unwrap_or_default()
            .iter()
            .map(|segment| segment.id.as_str().to_owned())
            .collect()
    }

    #[test]
    fn an_old_partition_written_long_ago_is_sealed() {
        let old = work(vec![segment("a", 10, 11 * HOUR)], false);

        assert_eq!(
            sealed_ids(SealPlan::of(&old, Some(CUTOFF), RETENTION)),
            ["a"]
        );
    }

    /// 补写的旧数据：分区早已结束，段刚写下。合并之前不封存，合并的结果也一样等。
    #[test]
    fn an_old_partition_written_just_now_waits_for_compaction() {
        let backfill = work(
            vec![
                segment("a", 10, CUTOFF + HOUR),
                segment("b", 10, CUTOFF + HOUR),
            ],
            false,
        );

        assert!(SealPlan::of(&backfill, Some(CUTOFF), RETENTION).is_none());

        let settled = work(
            vec![segment("a", 10, CUTOFF - 1), segment("b", 10, CUTOFF + 1)],
            false,
        );

        assert_eq!(
            sealed_ids(SealPlan::of(&settled, Some(CUTOFF), RETENTION)),
            ["a"]
        );
    }

    /// 已经封存的分区晚到的数据也先留给合并。
    #[test]
    fn late_data_in_a_sealed_partition_waits_too() {
        let late = work(vec![segment("late", 10, CUTOFF + HOUR)], true);
        assert!(SealPlan::of(&late, Some(CUTOFF), RETENTION).is_none());

        let aged = work(vec![segment("late", 10, CUTOFF)], true);
        assert_eq!(
            sealed_ids(SealPlan::of(&aged, Some(CUTOFF), RETENTION)),
            ["late"]
        );
    }

    #[test]
    fn a_recent_partition_is_not_cold() {
        let recent = work(vec![segment("a", 100, 101 * HOUR)], false);

        assert!(SealPlan::of(&recent, Some(CUTOFF), RETENTION).is_none());
    }

    /// 不封存时，已有清单的分区新来的段照旧直接写进清单。
    #[test]
    fn without_sealing_a_sealed_partition_still_takes_new_segments() {
        let late = work(vec![segment("late", 10, CUTOFF + HOUR)], true);

        assert_eq!(sealed_ids(SealPlan::of(&late, None, RETENTION)), ["late"]);
        assert!(SealPlan::of(&work(vec![segment("a", 10, 0)], false), None, RETENTION).is_none());
    }

    #[test]
    fn expired_segments_are_dropped_whatever_their_age() {
        let fresh = work(vec![segment("a", 10, CUTOFF + HOUR)], true);
        let plan = SealPlan::of(&fresh, Some(CUTOFF), 11 * HOUR).unwrap();

        assert!(plan.sealable.is_empty());
        assert_eq!(plan.expired_overlay, [SegmentId::from_string("a")]);
    }
}
