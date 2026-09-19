// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{collections::HashMap, sync::Arc};

use object_store::{ObjectStore, ObjectStoreExt, PutMode, PutOptions, PutPayload, path::Path};

use super::{PartitionManifestReader, encode_manifest};
use crate::{
    config::{CatalogSettings, GarbageCollectionSettings},
    domain::{
        storage::{
            ArtifactState, DatasetSelection, DatasetState, FileCatalog, ObjectChecksum,
            OrganizationScope, Partition, PartitionManifest, PartitionManifestPointer,
            PhysicalDataset, PublishPartitionManifest, SegmentState, StoredObject,
        },
        stream::StreamDefinition,
    },
    infra::storage::layout::StorageLayout,
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

struct PartitionWork {
    pointer: Option<PartitionManifestPointer>,
    base: Vec<crate::domain::storage::DataSegment>,
    overlay: Vec<crate::domain::storage::DataSegment>,
}

pub struct PartitionManifestManager {
    catalog: Arc<dyn FileCatalog>,
    reader: Arc<PartitionManifestReader>,
    object_store: Arc<dyn ObjectStore>,
    settings: CatalogSettings,
    gc_grace_micros: i64,
    default_retention_days: u32,
}

impl PartitionManifestManager {
    pub fn new(
        catalog: Arc<dyn FileCatalog>,
        reader: Arc<PartitionManifestReader>,
        object_store: Arc<dyn ObjectStore>,
        settings: CatalogSettings,
        gc: &GarbageCollectionSettings,
        default_retention_days: u32,
    ) -> Self {
        Self {
            catalog,
            reader,
            object_store,
            settings,
            gc_grace_micros: i64::from(gc.grace_period_secs) * 1_000_000,
            default_retention_days: default_retention_days.max(1),
        }
    }

    pub fn settings(&self) -> &CatalogSettings {
        &self.settings
    }

    /// Seal cold partitions, fold late overlays into the next generation, and apply retention.
    pub async fn maintain_stream(&self, stream: &StreamDefinition) -> Result<usize> {
        let scope = OrganizationScope::new(stream.org_id.clone());
        let datasets = self
            .catalog
            .list_datasets(&scope, &stream.id)
            .await?
            .into_iter()
            .filter(|dataset| dataset.state == DatasetState::Active)
            .collect::<Vec<_>>();
        let mut switched = 0;
        for dataset in datasets {
            switched += self.maintain_dataset(stream, &scope, &dataset).await?;
        }
        Ok(switched)
    }

    async fn maintain_dataset(
        &self,
        stream: &StreamDefinition,
        scope: &OrganizationScope,
        dataset: &PhysicalDataset,
    ) -> Result<usize> {
        let snapshot = self
            .catalog
            .snapshot(
                scope,
                DatasetSelection {
                    dataset_ids: vec![dataset.id.clone()],
                    time_range: TimeRange::new(
                        TimestampMicros(i64::MIN),
                        TimestampMicros(i64::MAX),
                    ),
                    partition_shard: None,
                },
            )
            .await?;
        let dataset_snapshot = snapshot
            .dataset(&dataset.id)
            .ok_or_else(|| Error::internal(format!("catalog omitted dataset {}", dataset.id)))?;
        let work = self.partition_work(dataset_snapshot).await?;
        let now = TimestampMicros::now().0;
        let seal_after_hours = dataset
            .storage_policy
            .seal_after_hours
            .unwrap_or(self.settings.seal_after_hours);
        let seal_cutoff = (seal_after_hours > 0).then(|| {
            now.saturating_sub(i64::from(seal_after_hours).saturating_mul(60 * 60 * 1_000_000))
        });
        let retention_days = dataset
            .storage_policy
            .retention_days
            .unwrap_or_else(|| stream.effective_retention_days(self.default_retention_days));
        let retention_cutoff =
            now.saturating_sub(i64::from(retention_days).saturating_mul(24 * 60 * 60 * 1_000_000));
        let mut switched = 0;
        let mut partitions = work.into_iter().collect::<Vec<_>>();
        partitions.sort_by_key(|(partition, _)| {
            (
                partition.end_micros,
                partition.start_micros,
                partition.shard,
            )
        });
        for (_, partition) in partitions
            .iter()
            .take(self.settings.batch_size.max(1) as usize)
        {
            if self
                .publish_partition(scope, dataset, partition, seal_cutoff, retention_cutoff)
                .await?
            {
                switched += 1;
            }
        }
        Ok(switched)
    }

    async fn partition_work(
        &self,
        snapshot: &crate::domain::storage::DatasetSnapshot,
    ) -> Result<HashMap<Partition, PartitionWork>> {
        let mut work = HashMap::new();
        for pointer in &snapshot.manifests {
            let manifest = self.reader.load(pointer).await?;
            work.insert(
                pointer.partition,
                PartitionWork {
                    pointer: Some(pointer.clone()),
                    base: manifest.segments.clone(),
                    overlay: Vec::new(),
                },
            );
        }
        for segment in &snapshot.segments {
            work.entry(segment.partition)
                .or_insert_with(|| PartitionWork {
                    pointer: None,
                    base: Vec::new(),
                    overlay: Vec::new(),
                })
                .overlay
                .push(segment.clone());
        }
        Ok(work)
    }

    async fn publish_partition(
        &self,
        scope: &OrganizationScope,
        dataset: &PhysicalDataset,
        work: &PartitionWork,
        seal_cutoff: Option<i64>,
        retention_cutoff: i64,
    ) -> Result<bool> {
        let is_cold = work.pointer.is_some()
            || seal_cutoff.is_some_and(|cutoff| {
                work.overlay
                    .first()
                    .is_some_and(|segment| segment.partition.end_micros <= cutoff)
            });
        if !is_cold {
            return Ok(false);
        }
        let expired_base = work
            .base
            .iter()
            .filter(|segment| segment.time_range.end.0 <= retention_cutoff)
            .map(|segment| segment.id.clone())
            .collect::<Vec<_>>();
        let expired_overlay = work
            .overlay
            .iter()
            .filter(|segment| segment.time_range.end.0 <= retention_cutoff)
            .map(|segment| segment.id.clone())
            .collect::<Vec<_>>();
        let sealable = work
            .overlay
            .iter()
            .filter(|segment| {
                segment.time_range.end.0 > retention_cutoff
                    && segment.primary.state == ArtifactState::Ready
                    && segment
                        .auxiliaries
                        .iter()
                        .all(|artifact| artifact.state == ArtifactState::Ready)
            })
            .cloned()
            .collect::<Vec<_>>();
        let changed =
            !sealable.is_empty() || !expired_base.is_empty() || !expired_overlay.is_empty();
        if !changed {
            return Ok(false);
        }

        let mut segments = work
            .base
            .iter()
            .filter(|segment| segment.time_range.end.0 > retention_cutoff)
            .cloned()
            .collect::<Vec<_>>();
        segments.extend(sealable.iter().cloned());
        segments
            .sort_by_key(|segment| (segment.time_range.start.0, segment.id.as_str().to_owned()));
        segments.dedup_by(|left, right| left.id == right.id);
        let generation = match &work.pointer {
            Some(pointer) => pointer
                .generation
                .checked_add(1)
                .ok_or_else(|| Error::internal("partition manifest generation overflow"))?,
            None => 1,
        };
        let new_pointer = if segments.is_empty() {
            None
        } else {
            Some(
                self.write_manifest(scope, dataset, work_partition(work), generation, segments)
                    .await?,
            )
        };
        let mut tombstones = expired_base;
        tombstones.extend(expired_overlay);
        let command = PublishPartitionManifest {
            dataset_id: dataset.id.clone(),
            partition: work_partition(work),
            expected_generation: work.pointer.as_ref().map(|pointer| pointer.generation),
            new_manifest: new_pointer.clone(),
            seal_segment_ids: sealable.into_iter().map(|segment| segment.id).collect(),
            tombstone_segment_ids: tombstones,
            gc_not_before_micros: TimestampMicros::now()
                .0
                .saturating_add(self.gc_grace_micros),
        };
        match self
            .catalog
            .publish_partition_manifest(scope, command)
            .await
        {
            Ok(_) => Ok(true),
            Err(error) => {
                if let Some(pointer) = new_pointer {
                    match self
                        .catalog
                        .object_is_referenced(scope, &pointer.object.key)
                        .await
                    {
                        Ok(false) => {
                            let _ = self
                                .catalog
                                .enqueue_orphan(
                                    scope,
                                    &pointer.object.key,
                                    TimestampMicros::now()
                                        .0
                                        .saturating_add(self.gc_grace_micros),
                                )
                                .await;
                        }
                        Ok(true) => {}
                        Err(check_error) => tracing::warn!(
                            object_key = %pointer.object.key,
                            %check_error,
                            "manifest publication failed and orphan status could not be checked"
                        ),
                    }
                }
                Err(error)
            }
        }
    }

    async fn write_manifest(
        &self,
        scope: &OrganizationScope,
        dataset: &PhysicalDataset,
        partition: Partition,
        generation: u64,
        segments: Vec<crate::domain::storage::DataSegment>,
    ) -> Result<PartitionManifestPointer> {
        let segments = segments
            .into_iter()
            .map(|mut segment| {
                segment.state = SegmentState::Sealed;
                segment
            })
            .collect();
        let manifest = PartitionManifest {
            organization_id: scope.organization_id.clone(),
            dataset_id: dataset.id.clone(),
            partition,
            generation,
            segments,
        };
        let bytes = encode_manifest(&manifest)?;
        let checksum = ObjectChecksum::from_string(format!("b3:{}", blake3::hash(&bytes).to_hex()));
        let key = StorageLayout::manifest_key(
            &scope.organization_id,
            &dataset.id,
            &partition,
            generation,
        );
        let put = self
            .object_store
            .put_opts(
                &Path::from(key.as_str()),
                PutPayload::from(bytes.clone()),
                PutOptions {
                    mode: PutMode::Create,
                    ..PutOptions::default()
                },
            )
            .await;
        let etag = match put {
            Ok(put) => put.e_tag,
            Err(object_store::Error::AlreadyExists { .. }) => {
                let path = Path::from(key.as_str());
                let existing = self
                    .object_store
                    .get(&path)
                    .await
                    .map_err(|error| {
                        Error::internal(format!("read existing partition manifest: {error}"))
                    })?
                    .bytes()
                    .await
                    .map_err(|error| {
                        Error::internal(format!("collect existing partition manifest: {error}"))
                    })?;
                if existing.len() != bytes.len() || blake3::hash(&existing) != blake3::hash(&bytes)
                {
                    return Err(Error::conflict(format!(
                        "manifest generation key {key} already contains different bytes"
                    )));
                }
                self.object_store
                    .head(&path)
                    .await
                    .map_err(|error| {
                        Error::internal(format!("head existing partition manifest: {error}"))
                    })?
                    .e_tag
            }
            Err(error) => {
                return Err(Error::internal(format!(
                    "upload partition manifest: {error}"
                )));
            }
        };
        Ok(PartitionManifestPointer {
            organization_id: scope.organization_id.clone(),
            dataset_id: dataset.id.clone(),
            partition,
            generation,
            object: StoredObject {
                key,
                size_bytes: bytes.len() as u64,
                checksum,
                etag,
            },
            segment_count: manifest.segments.len() as u32,
            created_at_micros: TimestampMicros::now().0,
        })
    }
}

fn work_partition(work: &PartitionWork) -> Partition {
    work.pointer
        .as_ref()
        .map(|pointer| pointer.partition)
        .or_else(|| work.overlay.first().map(|segment| segment.partition))
        .or_else(|| work.base.first().map(|segment| segment.partition))
        .expect("partition work is never empty")
}
