// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Indexes of sealed partitions written in an older format, or under index settings the stream has
//! changed since. A sealed segment is never merged again, so its index is rebuilt and published
//! with the next generation of its partition's manifest, in the transaction that swaps the index
//! rows. A segment indexed before the stream's indexes last changed gets everything rebuilt from
//! its file: its archive, its statistics and its filters in the partition's filter object. The
//! indexes of active segments are rebuilt by [`crate::infra::storage::index_rebuild`]; an active
//! segment indexed under old settings is rebuilt here once it is sealed.

use std::collections::HashMap;

use super::{PartitionManifestManager, PartitionWork, bloom_key};
use crate::{
    domain::{
        storage::{
            ArtifactReplacement, ArtifactRole, ArtifactState, DataSegment, ObjectKey,
            OrganizationScope, PartitionManifestPointer, PhysicalDataset, PublishPartitionManifest,
            type_id,
        },
        stream::StreamDefinition,
    },
    infra::{
        search::tantivy_index,
        storage::{
            filters::Filters,
            index_rebuild::{build_index_artifact, reindex_segment},
        },
    },
    shared::{Error, Result, time::TimestampMicros},
};

/// What a sealed segment needs rebuilt, by its position among the partition's segments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stale {
    /// Its archive, the auxiliary at `archive`, is in an older format.
    Format { segment: usize, archive: usize },
    /// It was indexed before the stream's indexes last changed: everything.
    Settings { segment: usize },
}

/// Indexes rebuilt for one dataset in one round at most.
const MAX_UPGRADES: usize = 4;

impl PartitionManifestManager {
    /// Rebuilds the stale indexes of sealed partitions that nothing else changed this round, the
    /// newest partitions first and at most [`MAX_UPGRADES`] indexes, and publishes each such
    /// partition again. Returns how many partitions switched.
    pub(super) async fn upgrade_indexes(
        &self,
        stream: &StreamDefinition,
        scope: &OrganizationScope,
        dataset: &PhysicalDataset,
        untouched: &[&PartitionWork],
        index_changed_at: Option<i64>,
    ) -> usize {
        let mut budget = MAX_UPGRADES;
        let mut switched = 0;

        for work in untouched.iter().rev() {
            let Some(pointer) = &work.pointer else {
                continue;
            };

            let stale = stale_indexes(&work.base, index_changed_at, budget);

            if stale.is_empty() || !work.overlay.is_empty() {
                continue;
            }

            budget -= stale.len();

            match self
                .publish_rebuilt(stream, scope, dataset, pointer, &work.base, &stale)
                .await
            {
                Ok(true) => switched += 1,
                Ok(false) => {}
                Err(error) => tracing::warn!(
                    dataset_id = %dataset.id,
                    %error,
                    "sealed partition index upgrade failed; will retry next round"
                ),
            }

            if budget == 0 {
                break;
            }
        }

        switched
    }

    /// Rebuilds what is `stale` in `base` and publishes the partition again with it. `false` when
    /// nothing could be rebuilt.
    async fn publish_rebuilt(
        &self,
        stream: &StreamDefinition,
        scope: &OrganizationScope,
        dataset: &PhysicalDataset,
        pointer: &PartitionManifestPointer,
        base: &[DataSegment],
        stale: &[Stale],
    ) -> Result<bool> {
        let mut segments = base.to_vec();
        let mut replacements = Vec::new();
        let mut reindexed: HashMap<String, Filters> = HashMap::new();

        for stale in stale {
            let (segment_index, archive_index) = match *stale {
                Stale::Format { segment, archive } => (segment, Some(archive)),
                Stale::Settings { segment } => (segment, ready_archive(&segments[segment])),
            };

            let segment = &segments[segment_index];

            let rebuilt = match stale {
                Stale::Format { .. } => build_index_artifact(
                    self.object_store.clone(),
                    self.object_store.clone(),
                    stream,
                    &scope.organization_id,
                    &dataset.id,
                    segment,
                )
                .await
                .map(|archive| (Some(archive), None)),
                Stale::Settings { .. } => reindex_segment(
                    self.object_store.clone(),
                    self.object_store.clone(),
                    stream,
                    &scope.organization_id,
                    &dataset.id,
                    segment,
                    archive_index.is_some(),
                )
                .await
                .map(|rebuilt| (rebuilt.archive, Some((rebuilt.filters, rebuilt.stats)))),
            };

            let (archive, reindex) = match rebuilt {
                Ok(rebuilt) => rebuilt,
                Err(error) => {
                    tracing::warn!(segment_id = %segment.id, %error, "sealed index rebuild failed");
                    continue;
                }
            };

            if let (Some(archive), Some(archive_index)) = (archive, archive_index) {
                replacements.push(ArtifactReplacement {
                    segment_id: segment.id.clone(),
                    artifact_id: segment.auxiliaries[archive_index].id.clone(),
                    replacement: archive.clone(),
                });

                segments[segment_index].auxiliaries[archive_index] = archive;
            }

            if let Some((filters, stats)) = reindex {
                reindexed.insert(
                    segments[segment_index].id.as_str().to_owned(),
                    filters.unwrap_or_default(),
                );

                segments[segment_index].column_stats = stats;
            }
        }

        if replacements.is_empty() && reindexed.is_empty() {
            return Ok(false);
        }

        let rebuilt_keys = replacements
            .iter()
            .map(|replacement| replacement.replacement.object.key.clone())
            .collect::<Vec<_>>();

        let generation = pointer
            .generation
            .checked_add(1)
            .ok_or_else(|| Error::internal("partition manifest generation overflow"))?;

        let new_pointer = self
            .write_manifest(
                stream.stream_type,
                scope,
                dataset,
                pointer.partition,
                generation,
                segments.clone(),
            )
            .await?;

        self.write_blooms(stream, &new_pointer, &segments, Some(pointer), &reindexed)
            .await;

        let not_before = TimestampMicros::now()
            .0
            .saturating_add(self.gc_grace_micros);

        let command = PublishPartitionManifest {
            dataset_id: dataset.id.clone(),
            partition: pointer.partition,
            expected_generation: Some(pointer.generation),
            new_manifest: Some(new_pointer.clone()),
            seal_segment_ids: Vec::new(),
            tombstone_segment_ids: Vec::new(),
            replace_artifacts: replacements,
            gc_not_before_micros: not_before,
        };

        let Err(error) = self
            .catalog
            .publish_partition_manifest(scope, command)
            .await
        else {
            return Ok(true);
        };

        // Nothing names the new manifest, its filters or the rebuilt archives unless the switch
        // went through after all.
        if let Ok(false) = self
            .catalog
            .object_is_referenced(scope, &new_pointer.object.key)
            .await
        {
            let orphans = rebuilt_keys.into_iter().chain([
                new_pointer.object.key.clone(),
                ObjectKey::from_string(bloom_key(&new_pointer.object.key)),
            ]);

            for key in orphans {
                let _ = self.catalog.enqueue_orphan(scope, &key, not_before).await;
            }
        }

        Err(error)
    }
}

/// What `segments` need rebuilt, at most `limit`: the segments indexed before `index_changed_at`
/// (a segment without the time was indexed before there was one), and the ready Tantivy indexes of
/// an older format in the rest.
fn stale_indexes(
    segments: &[DataSegment],
    index_changed_at: Option<i64>,
    limit: usize,
) -> Vec<Stale> {
    segments
        .iter()
        .enumerate()
        .filter_map(|(segment_index, segment)| {
            let indexed_before = index_changed_at.is_some_and(|changed_at| {
                segment.column_stats.indexed_at_micros.unwrap_or(i64::MIN) < changed_at
            });

            if indexed_before {
                return Some(Stale::Settings {
                    segment: segment_index,
                });
            }

            let archive = ready_archive(segment)?;

            (segment.auxiliaries[archive].format_version < tantivy_index::FORMAT_VERSION).then_some(
                Stale::Format {
                    segment: segment_index,
                    archive,
                },
            )
        })
        .take(limit)
        .collect()
}

/// Where among `segment`'s auxiliaries its ready Tantivy archive of its primary file is.
fn ready_archive(segment: &DataSegment) -> Option<usize> {
    segment.auxiliaries.iter().position(|artifact| {
        artifact.role == ArtifactRole::Index
            && artifact.state == ArtifactState::Ready
            && artifact.artifact_type.as_str() == type_id::builtin::ARTIFACT_TANTIVY
            && artifact.source_artifact_id.as_ref() == Some(&segment.primary.id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::storage::{
            Artifact, ArtifactId, ArtifactTypeId, ColumnStats, ObjectChecksum, Partition,
            PhysicalDatasetId, SegmentId, SegmentState, StoredObject,
        },
        shared::{
            ids::Id,
            time::{TimeRange, TimestampMicros},
        },
    };

    fn artifact(
        id: &str,
        role: ArtifactRole,
        artifact_type: &'static str,
        format_version: u32,
        source: Option<&ArtifactId>,
    ) -> Artifact {
        Artifact {
            id: ArtifactId::from_string(id),
            role,
            artifact_type: ArtifactTypeId::builtin(artifact_type),
            format_version,
            object: StoredObject {
                key: ObjectKey::from_string(format!("objects/{id}")),
                size_bytes: 1,
                checksum: ObjectChecksum::from_string("b3:test"),
                etag: None,
            },
            source_artifact_id: source.cloned(),
            source_checksum: None,
            schema_fingerprint: None,
            state: ArtifactState::Ready,
            failure_reason: None,
        }
    }

    fn segment(id: &str, index_format: u32) -> DataSegment {
        let primary = artifact(
            &format!("{id}-data"),
            ArtifactRole::PrimaryData,
            type_id::builtin::ARTIFACT_PARQUET,
            1,
            None,
        );

        let index = artifact(
            &format!("{id}-index"),
            ArtifactRole::Index,
            type_id::builtin::ARTIFACT_TANTIVY,
            index_format,
            Some(&primary.id),
        );

        DataSegment {
            id: SegmentId::from_string(id),
            organization_id: Id::from_string("org"),
            dataset_id: PhysicalDatasetId::from_string("dataset"),
            partition: Partition {
                start_micros: 0,
                end_micros: 3_600_000_000,
                shard: 0,
            },
            time_range: TimeRange::new(TimestampMicros(1), TimestampMicros(2)),
            sequence_range: None,
            row_count: 1,
            schema_fingerprint: None,
            column_stats: ColumnStats::default(),
            flush_id: None,
            output_ordinal: 0,
            primary,
            auxiliaries: vec![index],
            state: SegmentState::Sealed,
            visible_from_version: 1,
            retired_at_version: None,
            created_at_micros: 0,
        }
    }

    #[test]
    fn only_ready_tantivy_indexes_of_an_older_format_are_stale() {
        let segments = vec![
            segment("a", 1),
            segment("b", tantivy_index::FORMAT_VERSION),
            segment("c", 2),
            segment("d", 1),
        ];

        let format = |segment| Stale::Format {
            segment,
            archive: 0,
        };

        assert_eq!(
            stale_indexes(&segments, None, 10),
            vec![format(0), format(2), format(3)]
        );
        assert_eq!(
            stale_indexes(&segments, None, 2),
            vec![format(0), format(2)]
        );

        let mut failed = segments[0].clone();
        failed.auxiliaries[0].state = ArtifactState::Failed;
        assert!(stale_indexes(&[failed], None, 10).is_empty());
    }

    /// 早于数据流最近一次改索引建索引的段整段重建，没记下建索引时刻的也算早；之后建的只看格式。
    #[test]
    fn segments_indexed_before_the_indexes_changed_are_stale_whole() {
        let mut segments = vec![
            segment("before", tantivy_index::FORMAT_VERSION),
            segment("after", tantivy_index::FORMAT_VERSION),
            segment("unknown", tantivy_index::FORMAT_VERSION),
            segment("old-format", 1),
        ];

        segments[0].column_stats.indexed_at_micros = Some(10);
        segments[1].column_stats.indexed_at_micros = Some(30);
        segments[3].column_stats.indexed_at_micros = Some(30);

        assert_eq!(
            stale_indexes(&segments, Some(20), 10),
            vec![
                Stale::Settings { segment: 0 },
                Stale::Settings { segment: 2 },
                Stale::Format {
                    segment: 3,
                    archive: 0
                },
            ]
        );
        assert_eq!(stale_indexes(&segments[1..2], Some(20), 10), vec![]);
    }
}
