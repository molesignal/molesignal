// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! DatasetResolver：写入路径的 (stream, dataset type) → PhysicalDataset 解析。
//!
//! WAL、Buffer 与对象布局只认 [`PhysicalDatasetId`]；本层负责在首次写入时经
//! [`FileCatalog::ensure_datasets`] 幂等建集，并按 (stream id, dataset type) 缓存结果。
//! 类型合法性由 [`StreamTypeRegistry`] 把关——未注册组合明确报错，绝不回退默认。

use std::sync::Arc;

use dashmap::DashMap;

use crate::{
    domain::{
        storage::{
            DatasetTypeId, FileCatalog, OrganizationScope, PhysicalDataset, StreamTypeId,
            StreamTypeRegistry, WalCodecId,
        },
        stream::StreamDefinition,
    },
    infra::intake::wal_pool::WalStreamIdentity,
    shared::{Result, ids::Id},
};

/// 解析结果：dataset 行 + WAL 编码。
#[derive(Debug, Clone)]
pub struct ResolvedDataset {
    pub dataset: PhysicalDataset,
    pub wal_codec: WalCodecId,
}

impl ResolvedDataset {
    pub fn wal_identity(&self) -> WalStreamIdentity {
        WalStreamIdentity {
            organization_id: self.dataset.organization_id.clone(),
            dataset_id: self.dataset.id.clone(),
            dataset_type: self.dataset.dataset_type.clone(),
            wal_codec: self.wal_codec.clone(),
        }
    }
}

pub struct DatasetResolver {
    catalog: Arc<dyn FileCatalog>,
    registry: Arc<StreamTypeRegistry>,
    cache: DashMap<(Id, DatasetTypeId), Arc<ResolvedDataset>>,
}

impl DatasetResolver {
    pub fn new(catalog: Arc<dyn FileCatalog>, registry: Arc<StreamTypeRegistry>) -> Self {
        Self {
            catalog,
            registry,
            cache: DashMap::new(),
        }
    }

    pub fn registry(&self) -> &Arc<StreamTypeRegistry> {
        &self.registry
    }

    pub fn primary_dataset_type(&self, stream_type: StreamTypeId) -> Result<DatasetTypeId> {
        self.registry.primary_dataset_type(&stream_type)
    }

    /// 解析（必要时建集）。dataset 身份不可变，缓存不需要失效。
    pub async fn resolve(
        &self,
        stream: &StreamDefinition,
        dataset_type: DatasetTypeId,
    ) -> Result<Arc<ResolvedDataset>> {
        let cache_key = (stream.id.clone(), dataset_type.clone());
        if let Some(hit) = self.cache.get(&cache_key) {
            return Ok(hit.clone());
        }

        let descriptor = self
            .registry
            .dataset_type(&stream.stream_type, &dataset_type)?;
        let spec = descriptor.to_spec();
        let wal_codec = spec.wal_codec.clone();

        let scope = OrganizationScope::new(stream.org_id.clone());
        let datasets = self
            .catalog
            .ensure_datasets(&scope, &stream.id, std::slice::from_ref(&spec))
            .await?;
        let dataset = datasets.into_iter().next().ok_or_else(|| {
            crate::shared::Error::internal(format!(
                "file catalog omitted ensured dataset `{dataset_type}` for stream {}",
                stream.id
            ))
        })?;
        let resolved = Arc::new(ResolvedDataset { dataset, wal_codec });
        self.cache.insert(cache_key, resolved.clone());
        Ok(resolved)
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::{
        collections::HashSet,
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
    };

    use async_trait::async_trait;
    use dashmap::DashMap;

    use super::DatasetResolver;
    use crate::{
        domain::storage::{
            ArtifactState, CatalogSnapshot, CommitFlush, DataSegment, DatasetSelection,
            DatasetSnapshot, DatasetState, DatasetTransformResult, FileCatalog, FlushCommitResult,
            OrganizationScope, PhysicalDataset, PhysicalDatasetId, PhysicalDatasetSpec,
            PublishDatasetTransform, ReplaceSegments, SegmentState, TombstoneSegments,
            UpdateArtifact, WalCheckpointView, WalSequence, WriterEpoch, WriterNodeId,
            builtin_registry,
        },
        shared::{Error, Result, ids::Id, time::TimestampMicros},
    };

    /// 单测用内存 FileCatalog，覆盖数据集解析、快照和 flush 原子提交语义。
    #[derive(Default)]
    pub(crate) struct StubFileCatalog {
        datasets: DashMap<(String, String, String), PhysicalDataset>,
        segments: DashMap<(String, String), Vec<DataSegment>>,
        checkpoints: DashMap<(String, String, String, u64), WalSequence>,
        commits: DashMap<(String, String, String), u64>,
        fail_next_commit: AtomicBool,
        fail_next_replace: AtomicBool,
        committed_rows: Mutex<Vec<u64>>,
        snapshot_calls: AtomicUsize,
    }

    impl StubFileCatalog {
        pub(crate) fn fail_next_commit(&self) {
            self.fail_next_commit.store(true, Ordering::SeqCst);
        }

        pub(crate) fn fail_next_replace(&self) {
            self.fail_next_replace.store(true, Ordering::SeqCst);
        }

        pub(crate) fn committed_rows(&self) -> Vec<u64> {
            self.committed_rows.lock().unwrap().clone()
        }

        pub(crate) fn snapshot_calls(&self) -> usize {
            self.snapshot_calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl FileCatalog for StubFileCatalog {
        async fn ensure_datasets(
            &self,
            scope: &OrganizationScope,
            logical_stream_id: &Id,
            specs: &[PhysicalDatasetSpec],
        ) -> Result<Vec<PhysicalDataset>> {
            let now = TimestampMicros::now().0;
            Ok(specs
                .iter()
                .map(|spec| {
                    let key = (
                        scope.organization_id.as_str().to_owned(),
                        logical_stream_id.as_str().to_owned(),
                        spec.dataset_type.as_str().to_owned(),
                    );
                    self.datasets
                        .entry(key)
                        .or_insert_with(|| PhysicalDataset {
                            id: PhysicalDatasetId::generate(),
                            organization_id: scope.organization_id.clone(),
                            logical_stream_id: logical_stream_id.clone(),
                            dataset_type: spec.dataset_type.clone(),
                            dataset_type_version: spec.dataset_type_version,
                            partition_policy: spec.partition_policy.clone(),
                            storage_policy: spec.storage_policy.clone(),
                            index_policy: spec.index_policy.clone(),
                            catalog_version: 0,
                            state: DatasetState::Active,
                            created_at_micros: now,
                            updated_at_micros: now,
                        })
                        .clone()
                })
                .collect())
        }

        async fn list_datasets(
            &self,
            scope: &OrganizationScope,
            logical_stream_id: &Id,
        ) -> Result<Vec<PhysicalDataset>> {
            Ok(self
                .datasets
                .iter()
                .filter(|entry| {
                    entry.key().0 == scope.organization_id.as_str()
                        && entry.key().1 == logical_stream_id.as_str()
                })
                .map(|entry| entry.value().clone())
                .collect())
        }

        async fn snapshot(
            &self,
            scope: &OrganizationScope,
            selection: DatasetSelection,
        ) -> Result<CatalogSnapshot> {
            self.snapshot_calls.fetch_add(1, Ordering::SeqCst);
            let mut snapshots = Vec::with_capacity(selection.dataset_ids.len());
            for dataset_id in selection.dataset_ids {
                let dataset = self
                    .datasets
                    .iter()
                    .find(|entry| {
                        entry.value().organization_id == scope.organization_id
                            && entry.value().id == dataset_id
                    })
                    .map(|entry| entry.value().clone())
                    .ok_or_else(|| Error::not_found(format!("physical dataset {dataset_id}")))?;
                let segments = self
                    .segments
                    .get(&(
                        scope.organization_id.as_str().to_owned(),
                        dataset_id.as_str().to_owned(),
                    ))
                    .map(|rows| {
                        rows.iter()
                            .filter(|segment| {
                                segment.state == SegmentState::Active
                                    && segment.time_range.start.0 <= selection.time_range.end.0
                                    && segment.time_range.end.0 >= selection.time_range.start.0
                            })
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default();
                let wal_checkpoints = self
                    .checkpoints
                    .iter()
                    .filter(|entry| {
                        entry.key().0 == scope.organization_id.as_str()
                            && entry.key().1 == dataset_id.as_str()
                    })
                    .map(|entry| WalCheckpointView {
                        writer_node_id: WriterNodeId::new(entry.key().2.clone()),
                        writer_epoch: WriterEpoch(entry.key().3),
                        committed_sequence: *entry.value(),
                    })
                    .collect();
                snapshots.push(DatasetSnapshot {
                    dataset_id,
                    catalog_version: dataset.catalog_version,
                    wal_checkpoints,
                    segments,
                    manifests: Vec::new(),
                });
            }
            Ok(CatalogSnapshot {
                datasets: snapshots,
            })
        }

        async fn commit_flush(
            &self,
            scope: &OrganizationScope,
            command: CommitFlush,
        ) -> Result<FlushCommitResult> {
            if self.fail_next_commit.swap(false, Ordering::SeqCst) {
                return Err(Error::internal("injected FileCatalog commit failure"));
            }
            let commit_key = (
                scope.organization_id.as_str().to_owned(),
                command.dataset_id.as_str().to_owned(),
                command.provenance.flush_id.as_str().to_owned(),
            );
            if let Some(version) = self.commits.get(&commit_key) {
                return Ok(FlushCommitResult {
                    catalog_version: *version,
                    already_committed: true,
                });
            }
            let dataset_key = self
                .datasets
                .iter()
                .find(|entry| {
                    entry.value().organization_id == scope.organization_id
                        && entry.value().id == command.dataset_id
                })
                .map(|entry| entry.key().clone())
                .ok_or_else(|| {
                    Error::not_found(format!("physical dataset {}", command.dataset_id))
                })?;
            let version = {
                let mut dataset = self
                    .datasets
                    .get_mut(&dataset_key)
                    .expect("key found above");
                dataset.catalog_version += 1;
                dataset.catalog_version
            };
            let committed_rows = command
                .segments
                .iter()
                .map(|segment| segment.row_count)
                .collect::<Vec<_>>();
            self.segments
                .entry((
                    scope.organization_id.as_str().to_owned(),
                    command.dataset_id.as_str().to_owned(),
                ))
                .or_default()
                .extend(command.segments);
            let checkpoint_key = (
                scope.organization_id.as_str().to_owned(),
                command.dataset_id.as_str().to_owned(),
                command.provenance.writer_node_id.as_str().to_owned(),
                command.provenance.writer_epoch.0,
            );
            self.checkpoints
                .entry(checkpoint_key)
                .and_modify(|current| *current = (*current).max(command.provenance.sequence.end))
                .or_insert(command.provenance.sequence.end);
            self.committed_rows.lock().unwrap().extend(committed_rows);
            self.commits.insert(commit_key, version);
            Ok(FlushCommitResult {
                catalog_version: version,
                already_committed: false,
            })
        }

        async fn replace_segments(
            &self,
            scope: &OrganizationScope,
            command: ReplaceSegments,
        ) -> Result<u64> {
            if self.fail_next_replace.swap(false, Ordering::SeqCst) {
                return Err(Error::conflict("injected FileCatalog replacement conflict"));
            }
            if command.replaced.is_empty() {
                return Err(Error::invalid("replace_segments requires replaced ids"));
            }
            let dataset_key = self
                .datasets
                .iter()
                .find(|entry| {
                    entry.value().organization_id == scope.organization_id
                        && entry.value().id == command.dataset_id
                })
                .map(|entry| entry.key().clone())
                .ok_or_else(|| {
                    Error::not_found(format!("physical dataset {}", command.dataset_id))
                })?;
            for replacement in &command.replacements {
                replacement.validate()?;
                if replacement.organization_id != scope.organization_id
                    || replacement.dataset_id != command.dataset_id
                    || replacement.primary.state != ArtifactState::Ready
                {
                    return Err(Error::invalid(format!(
                        "replacement segment {} does not match dataset scope",
                        replacement.id
                    )));
                }
            }

            let segment_key = (
                scope.organization_id.as_str().to_owned(),
                command.dataset_id.as_str().to_owned(),
            );
            let replaced = command.replaced.into_iter().collect::<HashSet<_>>();
            let mut segments = self.segments.entry(segment_key).or_default();
            if replaced.iter().any(|id| {
                !segments
                    .iter()
                    .any(|segment| segment.id == *id && segment.state == SegmentState::Active)
            }) {
                return Err(Error::conflict(
                    "some replaced segments are no longer active",
                ));
            }
            let version = {
                let mut dataset = self
                    .datasets
                    .get_mut(&dataset_key)
                    .expect("dataset found above");
                dataset.catalog_version += 1;
                dataset.catalog_version
            };
            for segment in segments.iter_mut() {
                if replaced.contains(&segment.id) {
                    segment.state = SegmentState::Replaced;
                    segment.retired_at_version = Some(version);
                    segment.primary.state = ArtifactState::Tombstoned;
                    for artifact in &mut segment.auxiliaries {
                        artifact.state = ArtifactState::Tombstoned;
                    }
                }
            }
            segments.extend(command.replacements.into_iter().map(|mut segment| {
                segment.visible_from_version = version;
                segment
            }));
            Ok(version)
        }

        async fn publish_dataset_transform(
            &self,
            scope: &OrganizationScope,
            command: PublishDatasetTransform,
        ) -> Result<DatasetTransformResult> {
            if self.fail_next_replace.swap(false, Ordering::SeqCst) {
                return Err(Error::conflict(
                    "injected FileCatalog dataset transform conflict",
                ));
            }
            if command.input_dataset_id == command.output_dataset_id
                || command.input_segment_ids.is_empty()
                || command.output_segments.is_empty()
            {
                return Err(Error::invalid("invalid cross-dataset transform"));
            }
            let input_dataset_key = self
                .datasets
                .iter()
                .find(|entry| {
                    entry.value().organization_id == scope.organization_id
                        && entry.value().id == command.input_dataset_id
                })
                .map(|entry| entry.key().clone())
                .ok_or_else(|| {
                    Error::not_found(format!("physical dataset {}", command.input_dataset_id))
                })?;
            let output_dataset_key = self
                .datasets
                .iter()
                .find(|entry| {
                    entry.value().organization_id == scope.organization_id
                        && entry.value().id == command.output_dataset_id
                })
                .map(|entry| entry.key().clone())
                .ok_or_else(|| {
                    Error::not_found(format!("physical dataset {}", command.output_dataset_id))
                })?;
            if input_dataset_key.1 != output_dataset_key.1 {
                return Err(Error::invalid(
                    "transform datasets must belong to the same logical stream",
                ));
            }
            for output in &command.output_segments {
                output.validate()?;
                if output.organization_id != scope.organization_id
                    || output.dataset_id != command.output_dataset_id
                    || output.partition != command.input_partition
                    || output.primary.state != ArtifactState::Ready
                {
                    return Err(Error::invalid(
                        "transform output does not match target dataset",
                    ));
                }
            }

            let input_ids = command
                .input_segment_ids
                .iter()
                .cloned()
                .collect::<HashSet<_>>();
            if input_ids.len() != command.input_segment_ids.len() {
                return Err(Error::invalid("duplicate transform input segment"));
            }
            let input_segment_key = (
                scope.organization_id.as_str().to_owned(),
                command.input_dataset_id.as_str().to_owned(),
            );
            let mut inputs = self.segments.entry(input_segment_key).or_default();
            if input_ids.iter().any(|id| {
                !inputs.iter().any(|segment| {
                    segment.id == *id
                        && segment.partition == command.input_partition
                        && matches!(segment.state, SegmentState::Active | SegmentState::Sealed)
                })
            }) {
                return Err(Error::conflict(
                    "some transform inputs are no longer active or sealed",
                ));
            }
            let input_version = {
                let mut dataset = self
                    .datasets
                    .get_mut(&input_dataset_key)
                    .expect("input dataset found above");
                dataset.catalog_version += 1;
                dataset.catalog_version
            };
            for segment in inputs.iter_mut() {
                if input_ids.contains(&segment.id) {
                    segment.state = SegmentState::Replaced;
                    segment.retired_at_version = Some(input_version);
                    segment.primary.state = ArtifactState::Tombstoned;
                    for artifact in &mut segment.auxiliaries {
                        artifact.state = ArtifactState::Tombstoned;
                    }
                }
            }
            drop(inputs);

            let output_version = {
                let mut dataset = self
                    .datasets
                    .get_mut(&output_dataset_key)
                    .expect("output dataset found above");
                dataset.catalog_version += 1;
                dataset.catalog_version
            };
            self.segments
                .entry((
                    scope.organization_id.as_str().to_owned(),
                    command.output_dataset_id.as_str().to_owned(),
                ))
                .or_default()
                .extend(command.output_segments.into_iter().map(|mut segment| {
                    segment.visible_from_version = output_version;
                    segment
                }));
            Ok(DatasetTransformResult {
                input_catalog_version: input_version,
                output_catalog_version: output_version,
            })
        }

        async fn update_artifact(
            &self,
            _scope: &OrganizationScope,
            _command: UpdateArtifact,
        ) -> Result<()> {
            Err(Error::internal(
                "StubFileCatalog: update_artifact unsupported",
            ))
        }

        async fn tombstone_segments(
            &self,
            scope: &OrganizationScope,
            command: TombstoneSegments,
        ) -> Result<u64> {
            if command.segment_ids.is_empty() {
                return Err(Error::invalid("tombstone_segments requires segment ids"));
            }
            let dataset_key = self
                .datasets
                .iter()
                .find(|entry| {
                    entry.value().organization_id == scope.organization_id
                        && entry.value().id == command.dataset_id
                })
                .map(|entry| entry.key().clone())
                .ok_or_else(|| {
                    Error::not_found(format!("physical dataset {}", command.dataset_id))
                })?;
            let segment_ids = command.segment_ids.into_iter().collect::<HashSet<_>>();
            let segment_key = (
                scope.organization_id.as_str().to_owned(),
                command.dataset_id.as_str().to_owned(),
            );
            let mut segments = self.segments.entry(segment_key).or_default();
            if !segments.iter().any(|segment| {
                segment_ids.contains(&segment.id) && segment.state == SegmentState::Active
            }) {
                return Ok(self
                    .datasets
                    .get(&dataset_key)
                    .expect("dataset found above")
                    .catalog_version);
            }
            let version = {
                let mut dataset = self
                    .datasets
                    .get_mut(&dataset_key)
                    .expect("dataset found above");
                dataset.catalog_version += 1;
                dataset.catalog_version
            };
            for segment in segments.iter_mut() {
                if segment_ids.contains(&segment.id) && segment.state == SegmentState::Active {
                    segment.state = SegmentState::Tombstoned;
                    segment.retired_at_version = Some(version);
                    segment.primary.state = ArtifactState::Tombstoned;
                    for artifact in &mut segment.auxiliaries {
                        artifact.state = ArtifactState::Tombstoned;
                    }
                }
            }
            Ok(version)
        }

        async fn wal_checkpoints(
            &self,
            scope: &OrganizationScope,
            dataset_id: &PhysicalDatasetId,
        ) -> Result<Vec<WalCheckpointView>> {
            Ok(self
                .checkpoints
                .iter()
                .filter(|entry| {
                    entry.key().0 == scope.organization_id.as_str()
                        && entry.key().1 == dataset_id.as_str()
                })
                .map(|entry| WalCheckpointView {
                    writer_node_id: WriterNodeId::new(entry.key().2.clone()),
                    writer_epoch: WriterEpoch(entry.key().3),
                    committed_sequence: *entry.value(),
                })
                .collect())
        }
    }

    pub(crate) fn test_catalog_and_resolver() -> (Arc<StubFileCatalog>, Arc<DatasetResolver>) {
        let catalog = Arc::new(StubFileCatalog::default());
        let resolver = Arc::new(DatasetResolver::new(
            catalog.clone(),
            Arc::new(builtin_registry()),
        ));
        (catalog, resolver)
    }

    pub(crate) fn test_resolver() -> Arc<DatasetResolver> {
        test_catalog_and_resolver().1
    }
}

#[cfg(test)]
mod tests {
    use super::{test_support::test_resolver, *};
    use crate::{
        domain::{
            storage::{DatasetTypeId, primary_dataset_type, type_id::builtin},
            stream::{Schema, StreamType},
        },
        shared::time::TimestampMicros,
    };

    fn stream(stream_type: StreamType) -> StreamDefinition {
        StreamDefinition {
            id: Id::new(),
            org_id: Id::from_string("org-a"),
            name: "app".into(),
            stream_type,
            schema: Schema { fields: vec![] },
            retention: None,
            created_at: TimestampMicros::now(),
            updated_at: TimestampMicros::now(),
        }
    }

    #[tokio::test]
    async fn resolve_is_idempotent_and_cached() {
        let resolver = test_resolver();
        let s = stream(StreamType::LOGS);
        let dataset_type = primary_dataset_type(s.stream_type).unwrap();
        let first = resolver.resolve(&s, dataset_type.clone()).await.unwrap();
        let second = resolver.resolve(&s, dataset_type).await.unwrap();
        assert_eq!(first.dataset.id, second.dataset.id);
        assert_eq!(first.dataset.dataset_type.as_str(), "builtin.logs.records");
        assert_eq!(first.wal_codec.as_str(), "builtin.row_batch");

        let identity = first.wal_identity();
        assert_eq!(identity.dataset_id, first.dataset.id);
        assert_eq!(identity.organization_id, s.org_id);
    }

    #[tokio::test]
    async fn distinct_dataset_types_get_distinct_datasets() {
        let resolver = test_resolver();
        let s = stream(StreamType::METRICS);
        let raw = resolver
            .resolve(&s, primary_dataset_type(s.stream_type).unwrap())
            .await
            .unwrap();
        let rollup = resolver
            .resolve(&s, DatasetTypeId::builtin(builtin::DATASET_METRIC_ROLLUP))
            .await
            .unwrap();
        assert_ne!(raw.dataset.id, rollup.dataset.id);
        assert_eq!(
            rollup.dataset.dataset_type.as_str(),
            "builtin.metrics.rollup"
        );
    }

    #[tokio::test]
    async fn extend_streams_are_rejected_loudly() {
        let resolver = test_resolver();
        let s = stream(StreamType::EXTEND);
        assert!(
            resolver.primary_dataset_type(s.stream_type).is_err(),
            "extend streams have no physical datasets"
        );
    }
}
