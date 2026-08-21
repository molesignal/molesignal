// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 统一 FileCatalog（PG 实现）集成测试：flush 幂等、多写入节点、epoch 栅栏、
//! compaction 替换冲突、索引重建槽位、tombstone 与延迟 GC 入队。
//!
//! 默认跳过（testcontainers 需要 docker）；`MS_RUN_IT=1` 启用：
//! ```bash
//! MS_RUN_IT=1 cargo test -p molesignal --test infra_it_file_catalog -- --nocapture
//! ```

use molesignal::{
    config::MetaStoreSettings,
    domain::{
        iam::{Organization, OrganizationRepository},
        storage::{
            Artifact, ArtifactId, ArtifactRole, ArtifactState, ArtifactTypeId, ArtifactUpdate,
            CatalogInvariant, ColumnStats, CommitFlush, DataSegment, DatasetSelection,
            DatasetTypeId, FileCatalog, FlushProvenance, ObjectChecksum, OrganizationScope,
            Partition, PartitionManifestPointer, PhysicalDatasetId, PublishDatasetTransform,
            PublishPartitionManifest, ReplaceSegments, SegmentId, SegmentState, SequenceRange,
            StoredObject, TombstoneSegments, UpdateArtifact, WalSequence, WriterEpoch,
            WriterNodeId, builtin_registry, type_id,
        },
        stream::{Schema, StreamDefinition, StreamRepository, StreamType},
    },
    infra::{
        persistence::{
            MetaStore,
            repositories::{
                file_catalog::PgFileCatalog, organizations::PgOrganizationRepository,
                streams::PgStreamRepository,
            },
        },
        storage::layout::StorageLayout,
    },
    shared::{
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres as PgImage;

fn skip_unless_enabled() -> bool {
    std::env::var("MS_RUN_IT").ok().as_deref() != Some("1")
}

const HOUR_MICROS: i64 = 3_600_000_000;

struct Harness {
    catalog: PgFileCatalog,
    pool: sqlx::PgPool,
    scope: OrganizationScope,
    stream_id: Id,
    dataset_id: PhysicalDatasetId,
    // 容器随 Harness 一起存活。
    _pg: testcontainers::ContainerAsync<PgImage>,
}

async fn harness() -> Harness {
    let pg = PgImage::default().start().await.expect("start pg");
    let port = pg.get_host_port_ipv4(5432).await.expect("pg port");
    let host = pg.get_host().await.expect("pg host");
    let dsn = format!("postgres://postgres:postgres@{host}:{port}/postgres");
    let store = MetaStore::connect(&MetaStoreSettings {
        backend: "postgres".into(),
        dsn,
        min_connections: 1,
        max_connections: 5,
    })
    .await
    .expect("connect + migrate");

    let org_id = Id::new();
    PgOrganizationRepository::new(store.pool.clone())
        .create(Organization {
            id: org_id.clone(),
            name: "Cat".into(),
            slug: "cat".into(),
            system: false,
            disabled: false,
            created_at: TimestampMicros::now(),
        })
        .await
        .expect("create org");

    let now = TimestampMicros::now();
    let stream = PgStreamRepository::new(store.pool.clone())
        .create(StreamDefinition {
            id: Id::new(),
            org_id: org_id.clone(),
            name: "app_logs".into(),
            stream_type: StreamType::LOGS,
            schema: Schema { fields: vec![] },
            retention: None,
            created_at: now,
            updated_at: now,
        })
        .await
        .expect("create stream");

    let catalog = PgFileCatalog::new(store.pool.clone());
    let scope = OrganizationScope::new(org_id);

    let registry = builtin_registry();
    let specs = registry
        .eager_dataset_specs(&StreamType::LOGS)
        .expect("eager specs");
    assert_eq!(specs.len(), 1, "logs eagerly provisions records only");

    let first = catalog
        .ensure_datasets(&scope, &stream.id, &specs)
        .await
        .expect("ensure datasets");
    let second = catalog
        .ensure_datasets(&scope, &stream.id, &specs)
        .await
        .expect("ensure datasets twice");
    assert_eq!(first[0].id, second[0].id, "ensure_datasets is idempotent");
    assert_eq!(
        first[0].dataset_type.as_str(),
        type_id::builtin::DATASET_LOG_RECORDS
    );

    Harness {
        catalog,
        pool: store.pool.clone(),
        scope,
        stream_id: stream.id,
        dataset_id: first[0].id.clone(),
        _pg: pg,
    }
}

fn ready_parquet(h: &Harness, partition: &Partition, segment_id: &SegmentId) -> Artifact {
    ready_parquet_for(h, &h.dataset_id, partition, segment_id)
}

fn ready_parquet_for(
    h: &Harness,
    dataset_id: &PhysicalDatasetId,
    partition: &Partition,
    segment_id: &SegmentId,
) -> Artifact {
    let id = ArtifactId::generate();
    let artifact_type = ArtifactTypeId::builtin(type_id::builtin::ARTIFACT_PARQUET);
    let key = StorageLayout::artifact_key(
        &h.scope.organization_id,
        dataset_id,
        partition,
        segment_id,
        &id,
        &artifact_type,
    );
    Artifact {
        id,
        role: ArtifactRole::PrimaryData,
        artifact_type,
        format_version: 1,
        object: StoredObject {
            key,
            size_bytes: 1024,
            checksum: ObjectChecksum::from_string("b3:deadbeef"),
            etag: None,
        },
        source_artifact_id: None,
        source_checksum: None,
        schema_fingerprint: None,
        state: ArtifactState::Ready,
        failure_reason: None,
    }
}

fn pending_tantivy(
    h: &Harness,
    partition: &Partition,
    segment_id: &SegmentId,
    source: &ArtifactId,
) -> Artifact {
    let id = ArtifactId::generate();
    let artifact_type = ArtifactTypeId::builtin(type_id::builtin::ARTIFACT_TANTIVY);
    let key = StorageLayout::artifact_key(
        &h.scope.organization_id,
        &h.dataset_id,
        partition,
        segment_id,
        &id,
        &artifact_type,
    );
    Artifact {
        id,
        role: ArtifactRole::Index,
        artifact_type,
        format_version: 1,
        object: StoredObject {
            key,
            size_bytes: 0,
            checksum: ObjectChecksum::from_string(""),
            etag: None,
        },
        source_artifact_id: Some(source.clone()),
        source_checksum: Some(ObjectChecksum::from_string("b3:deadbeef")),
        schema_fingerprint: None,
        state: ArtifactState::Pending,
        failure_reason: None,
    }
}

fn segment(h: &Harness, provenance: &FlushProvenance, hour: i64, with_index: bool) -> DataSegment {
    let partition = Partition {
        start_micros: hour * HOUR_MICROS,
        end_micros: (hour + 1) * HOUR_MICROS,
        shard: 0,
    };
    let id = SegmentId::generate();
    let primary = ready_parquet(h, &partition, &id);
    let auxiliaries = if with_index {
        vec![pending_tantivy(h, &partition, &id, &primary.id)]
    } else {
        vec![]
    };
    DataSegment {
        id,
        organization_id: h.scope.organization_id.clone(),
        dataset_id: h.dataset_id.clone(),
        partition,
        time_range: TimeRange::new(
            TimestampMicros(hour * HOUR_MICROS + 1),
            TimestampMicros(hour * HOUR_MICROS + 1000),
        ),
        sequence_range: Some(provenance.sequence),
        row_count: 100,
        schema_fingerprint: None,
        column_stats: ColumnStats::default(),
        flush_id: Some(provenance.flush_id.clone()),
        output_ordinal: 0,
        primary,
        auxiliaries,
        state: SegmentState::Active,
        visible_from_version: 0,
        retired_at_version: None,
        created_at_micros: TimestampMicros::now().0,
    }
}

fn provenance(node: &str, epoch: u64, start: u64, end: u64) -> FlushProvenance {
    FlushProvenance::derive(
        WriterNodeId::new(node),
        WriterEpoch(epoch),
        SequenceRange::new(WalSequence(start), WalSequence(end)),
    )
}

fn whole_day() -> TimeRange {
    TimeRange::new(TimestampMicros(0), TimestampMicros(24 * HOUR_MICROS))
}

async fn snapshot_segments(h: &Harness) -> Vec<DataSegment> {
    let snapshot = h
        .catalog
        .snapshot(
            &h.scope,
            DatasetSelection {
                dataset_ids: vec![h.dataset_id.clone()],
                time_range: whole_day(),
                partition_shard: None,
            },
        )
        .await
        .expect("snapshot");
    snapshot
        .datasets
        .into_iter()
        .next()
        .expect("dataset")
        .segments
}

async fn gc_queue_len(h: &Harness) -> i64 {
    sqlx::query_scalar::<i64>("SELECT COUNT(*) FROM object_gc_queue WHERE org_id = $1")
        .bind(h.scope.organization_id.as_str())
        .fetch_one(&h.pool)
        .await
        .expect("count gc queue")
}

#[tokio::test]
async fn flush_publish_replace_and_tombstone_lifecycle() {
    if skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }
    let h = harness().await;

    // flush 发布 + 幂等重试。
    let p1 = provenance("node-a", 1, 1, 10);
    let seg1 = segment(&h, &p1, 1, true);
    let commit = CommitFlush {
        dataset_id: h.dataset_id.clone(),
        provenance: p1.clone(),
        segments: vec![seg1.clone()],
    };
    let first = h
        .catalog
        .commit_flush(&h.scope, commit.clone())
        .await
        .expect("first commit");
    assert!(!first.already_committed);
    assert_eq!(first.catalog_version, 1);

    let retry = h
        .catalog
        .commit_flush(&h.scope, commit)
        .await
        .expect("retry commit");
    assert!(retry.already_committed, "same flush_id must be idempotent");
    assert_eq!(retry.catalog_version, 1);
    assert_eq!(snapshot_segments(&h).await.len(), 1, "no duplicate segment");

    // 快照带 checkpoint；pending 索引对查询可见（用于降级判断）。
    let snap = h
        .catalog
        .snapshot(
            &h.scope,
            DatasetSelection {
                dataset_ids: vec![h.dataset_id.clone()],
                time_range: whole_day(),
                partition_shard: None,
            },
        )
        .await
        .expect("snapshot");
    let ds = &snap.datasets[0];
    assert_eq!(
        ds.committed_sequence(&WriterNodeId::new("node-a"), WriterEpoch(1)),
        WalSequence(10)
    );
    assert_eq!(ds.segments[0].auxiliaries.len(), 1);
    assert_eq!(ds.segments[0].auxiliaries[0].state, ArtifactState::Pending);

    // 另一个写入节点：相同 epoch 与序号区间不得撞车（flush_id 含 node）。
    let p2 = provenance("node-b", 1, 1, 10);
    let seg2 = segment(&h, &p2, 2, false);
    let other = h
        .catalog
        .commit_flush(
            &h.scope,
            CommitFlush {
                dataset_id: h.dataset_id.clone(),
                provenance: p2,
                segments: vec![seg2.clone()],
            },
        )
        .await
        .expect("other node commit");
    assert!(
        !other.already_committed,
        "different node is a distinct flush"
    );
    assert_eq!(other.catalog_version, 2);

    // 索引构建完成：Pending → Ready。
    let index_id = seg1.auxiliaries[0].id.clone();
    let mut ready_object = seg1.auxiliaries[0].object.clone();
    ready_object.size_bytes = 512;
    ready_object.checksum = ObjectChecksum::from_string("b3:index");
    h.catalog
        .update_artifact(
            &h.scope,
            UpdateArtifact {
                dataset_id: h.dataset_id.clone(),
                segment_id: seg1.id.clone(),
                artifact_id: index_id.clone(),
                update: ArtifactUpdate::MarkReady {
                    object: ready_object,
                },
            },
        )
        .await
        .expect("index ready");

    // 索引重建替换：旧行 tombstone + 入 GC，新行占据同一槽位。
    let rebuilt = {
        let mut a = pending_tantivy(&h, &seg1.partition, &seg1.id, &seg1.primary.id);
        a.state = ArtifactState::Ready;
        a.object.size_bytes = 640;
        a.object.checksum = ObjectChecksum::from_string("b3:index2");
        a
    };
    h.catalog
        .update_artifact(
            &h.scope,
            UpdateArtifact {
                dataset_id: h.dataset_id.clone(),
                segment_id: seg1.id.clone(),
                artifact_id: index_id,
                update: ArtifactUpdate::Replace {
                    replacement: rebuilt.clone(),
                    gc_not_before_micros: TimestampMicros::now().0 + HOUR_MICROS,
                },
            },
        )
        .await
        .expect("index rebuild swap");
    assert_eq!(gc_queue_len(&h).await, 1, "old index object queued for gc");
    let live_index: Vec<_> = snapshot_segments(&h)
        .await
        .into_iter()
        .find(|s| s.id == seg1.id)
        .expect("segment")
        .auxiliaries;
    assert_eq!(
        live_index.len(),
        1,
        "tombstoned index excluded from snapshot"
    );
    assert_eq!(live_index[0].id, rebuilt.id);
    assert_eq!(live_index[0].state, ArtifactState::Ready);

    // compaction 替换：全有全无 + 版本推进 + 旧对象入 GC。
    let merged = {
        let p = provenance("compactor", 1, 0, 0);
        let mut s = segment(&h, &p, 1, false);
        s.flush_id = None;
        s.sequence_range = None;
        s.time_range = TimeRange::new(
            TimestampMicros(HOUR_MICROS),
            TimestampMicros(3 * HOUR_MICROS),
        );
        s
    };
    let new_version = h
        .catalog
        .replace_segments(
            &h.scope,
            ReplaceSegments {
                dataset_id: h.dataset_id.clone(),
                replaced: vec![seg1.id.clone(), seg2.id.clone()],
                replacements: vec![merged.clone()],
                gc_not_before_micros: TimestampMicros::now().0 + HOUR_MICROS,
            },
        )
        .await
        .expect("replace");
    assert_eq!(new_version, 3);
    let segments = snapshot_segments(&h).await;
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].id, merged.id);

    // 并发合并败方：同一批源已被替换 → Conflict。
    let conflict = h
        .catalog
        .replace_segments(
            &h.scope,
            ReplaceSegments {
                dataset_id: h.dataset_id.clone(),
                replaced: vec![seg1.id.clone()],
                replacements: vec![],
                gc_not_before_micros: 0,
            },
        )
        .await;
    assert!(conflict.is_err(), "losing compactor must get a conflict");

    // retention tombstone：幂等 + GC 入队。
    let after_tombstone = h
        .catalog
        .tombstone_segments(
            &h.scope,
            TombstoneSegments {
                dataset_id: h.dataset_id.clone(),
                segment_ids: vec![merged.id.clone()],
                gc_not_before_micros: TimestampMicros::now().0 + HOUR_MICROS,
            },
        )
        .await
        .expect("tombstone");
    assert_eq!(after_tombstone, 4);
    assert!(snapshot_segments(&h).await.is_empty());
    let repeat = h
        .catalog
        .tombstone_segments(
            &h.scope,
            TombstoneSegments {
                dataset_id: h.dataset_id.clone(),
                segment_ids: vec![merged.id],
                gc_not_before_micros: 0,
            },
        )
        .await
        .expect("tombstone repeat");
    assert_eq!(repeat, 4, "idempotent tombstone must not bump version");
}

#[tokio::test]
async fn stale_writer_epoch_is_fenced() {
    if skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }
    let h = harness().await;

    let newer = provenance("node-a", 2, 1, 5);
    h.catalog
        .commit_flush(
            &h.scope,
            CommitFlush {
                dataset_id: h.dataset_id.clone(),
                provenance: newer.clone(),
                segments: vec![segment(&h, &newer, 1, false)],
            },
        )
        .await
        .expect("epoch 2 commit");

    let stale = provenance("node-a", 1, 1, 5);
    let rejected = h
        .catalog
        .commit_flush(
            &h.scope,
            CommitFlush {
                dataset_id: h.dataset_id.clone(),
                provenance: stale.clone(),
                segments: vec![segment(&h, &stale, 2, false)],
            },
        )
        .await;
    assert!(
        rejected.is_err(),
        "zombie writer from older epoch is fenced"
    );

    // 其他节点不受该栅栏影响。
    let other_node = provenance("node-b", 1, 1, 5);
    h.catalog
        .commit_flush(
            &h.scope,
            CommitFlush {
                dataset_id: h.dataset_id.clone(),
                provenance: other_node.clone(),
                segments: vec![segment(&h, &other_node, 3, false)],
            },
        )
        .await
        .expect("independent node epoch space");
}

#[tokio::test]
async fn manifest_generation_switches_atomically_and_can_retire_partition() {
    if skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }
    let h = harness().await;
    let provenance = provenance("node-a", 1, 1, 10);
    let segment = segment(&h, &provenance, 1, false);
    h.catalog
        .commit_flush(
            &h.scope,
            CommitFlush {
                dataset_id: h.dataset_id.clone(),
                provenance,
                segments: vec![segment.clone()],
            },
        )
        .await
        .expect("seed active segment");

    let pointer = PartitionManifestPointer {
        organization_id: h.scope.organization_id.clone(),
        dataset_id: h.dataset_id.clone(),
        partition: segment.partition,
        generation: 1,
        object: StoredObject {
            key: StorageLayout::manifest_key(
                &h.scope.organization_id,
                &h.dataset_id,
                &segment.partition,
                1,
            ),
            size_bytes: 512,
            checksum: ObjectChecksum::from_string("b3:manifest-1"),
            etag: Some("etag-1".into()),
        },
        segment_count: 1,
        created_at_micros: TimestampMicros::now().0,
    };
    let sealed_version = h
        .catalog
        .publish_partition_manifest(
            &h.scope,
            PublishPartitionManifest {
                dataset_id: h.dataset_id.clone(),
                partition: segment.partition,
                expected_generation: None,
                new_manifest: Some(pointer.clone()),
                seal_segment_ids: vec![segment.id.clone()],
                tombstone_segment_ids: Vec::new(),
                gc_not_before_micros: TimestampMicros::now().0 + HOUR_MICROS,
            },
        )
        .await
        .expect("publish first manifest generation");
    assert_eq!(sealed_version, 2);

    let snapshot = h
        .catalog
        .snapshot(
            &h.scope,
            DatasetSelection {
                dataset_ids: vec![h.dataset_id.clone()],
                time_range: whole_day(),
                partition_shard: None,
            },
        )
        .await
        .expect("snapshot sealed partition");
    assert!(snapshot.datasets[0].segments.is_empty());
    assert_eq!(snapshot.datasets[0].manifests, vec![pointer]);

    let retired_version = h
        .catalog
        .publish_partition_manifest(
            &h.scope,
            PublishPartitionManifest {
                dataset_id: h.dataset_id.clone(),
                partition: segment.partition,
                expected_generation: Some(1),
                new_manifest: None,
                seal_segment_ids: Vec::new(),
                tombstone_segment_ids: vec![segment.id],
                gc_not_before_micros: TimestampMicros::now().0 + HOUR_MICROS,
            },
        )
        .await
        .expect("retire manifest partition");
    assert_eq!(retired_version, 3);
    let snapshot = h
        .catalog
        .snapshot(
            &h.scope,
            DatasetSelection {
                dataset_ids: vec![h.dataset_id.clone()],
                time_range: whole_day(),
                partition_shard: None,
            },
        )
        .await
        .expect("snapshot retired partition");
    assert!(snapshot.datasets[0].segments.is_empty());
    assert!(snapshot.datasets[0].manifests.is_empty());
    assert_eq!(gc_queue_len(&h).await, 2, "manifest and primary queued");
}

#[tokio::test]
async fn cross_dataset_transform_retires_sealed_base_and_publishes_output_atomically() {
    if skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }
    let h = harness().await;
    let output_type = DatasetTypeId::builtin(type_id::builtin::DATASET_RUM_SESSION_SUMMARY);
    let output_spec = builtin_registry()
        .dataset_type(&StreamType::LOGS, &output_type)
        .unwrap()
        .to_spec();
    let output_dataset = h
        .catalog
        .ensure_datasets(&h.scope, &h.stream_id, &[output_spec])
        .await
        .expect("ensure transform output dataset")
        .remove(0);

    let provenance = provenance("node-a", 1, 1, 10);
    let input = segment(&h, &provenance, 1, false);
    h.catalog
        .commit_flush(
            &h.scope,
            CommitFlush {
                dataset_id: h.dataset_id.clone(),
                provenance,
                segments: vec![input.clone()],
            },
        )
        .await
        .expect("seed transform input");
    let pointer = PartitionManifestPointer {
        organization_id: h.scope.organization_id.clone(),
        dataset_id: h.dataset_id.clone(),
        partition: input.partition,
        generation: 1,
        object: StoredObject {
            key: StorageLayout::manifest_key(
                &h.scope.organization_id,
                &h.dataset_id,
                &input.partition,
                1,
            ),
            size_bytes: 512,
            checksum: ObjectChecksum::from_string("b3:transform-manifest"),
            etag: None,
        },
        segment_count: 1,
        created_at_micros: TimestampMicros::now().0,
    };
    h.catalog
        .publish_partition_manifest(
            &h.scope,
            PublishPartitionManifest {
                dataset_id: h.dataset_id.clone(),
                partition: input.partition,
                expected_generation: None,
                new_manifest: Some(pointer.clone()),
                seal_segment_ids: vec![input.id.clone()],
                tombstone_segment_ids: Vec::new(),
                gc_not_before_micros: TimestampMicros::now().0 + HOUR_MICROS,
            },
        )
        .await
        .expect("seal transform input");

    let output_id = SegmentId::generate();
    let output = DataSegment {
        id: output_id.clone(),
        organization_id: h.scope.organization_id.clone(),
        dataset_id: output_dataset.id.clone(),
        partition: input.partition,
        time_range: input.time_range,
        sequence_range: None,
        row_count: 10,
        schema_fingerprint: None,
        column_stats: ColumnStats::default(),
        flush_id: None,
        output_ordinal: 0,
        primary: ready_parquet_for(&h, &output_dataset.id, &input.partition, &output_id),
        auxiliaries: Vec::new(),
        state: SegmentState::Active,
        visible_from_version: 0,
        retired_at_version: None,
        created_at_micros: TimestampMicros::now().0,
    };
    let published = h
        .catalog
        .publish_dataset_transform(
            &h.scope,
            PublishDatasetTransform {
                input_dataset_id: h.dataset_id.clone(),
                input_partition: input.partition,
                input_manifest: Some(pointer),
                input_segment_ids: vec![input.id],
                output_dataset_id: output_dataset.id.clone(),
                output_segments: vec![output],
                gc_not_before_micros: TimestampMicros::now().0 + HOUR_MICROS,
            },
        )
        .await
        .expect("publish cross-dataset transform");
    assert_eq!(published.input_catalog_version, 3);
    assert_eq!(published.output_catalog_version, 1);

    let snapshot = h
        .catalog
        .snapshot(
            &h.scope,
            DatasetSelection {
                dataset_ids: vec![h.dataset_id.clone(), output_dataset.id.clone()],
                time_range: whole_day(),
                partition_shard: None,
            },
        )
        .await
        .expect("snapshot transformed datasets");
    let input_snapshot = snapshot.dataset(&h.dataset_id).unwrap();
    assert!(input_snapshot.segments.is_empty());
    assert!(input_snapshot.manifests.is_empty());
    let output_snapshot = snapshot.dataset(&output_dataset.id).unwrap();
    assert_eq!(output_snapshot.segments.len(), 1);
    assert_eq!(output_snapshot.segments[0].id, output_id);
    assert_eq!(gc_queue_len(&h).await, 2, "raw primary and manifest queued");
}

#[tokio::test]
async fn reconciler_catalog_invariants_detect_checkpoint_drift() {
    if skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }
    let h = harness().await;
    let provenance = provenance("node-a", 1, 1, 10);
    h.catalog
        .commit_flush(
            &h.scope,
            CommitFlush {
                dataset_id: h.dataset_id.clone(),
                provenance: provenance.clone(),
                segments: vec![segment(&h, &provenance, 1, false)],
            },
        )
        .await
        .expect("seed committed flush");
    sqlx::query(
        "UPDATE wal_checkpoints SET committed_sequence = committed_sequence + 1 \
         WHERE org_id = $1 AND dataset_id = $2",
    )
    .bind(h.scope.organization_id.as_str())
    .bind(h.dataset_id.as_str())
    .execute(&h.pool)
    .await
    .expect("inject checkpoint drift");

    let issues = h
        .catalog
        .catalog_invariant_issues(&h.scope, TimestampMicros::now().0)
        .await
        .expect("scan catalog invariants");
    assert!(issues.iter().any(|issue| {
        issue.invariant == CatalogInvariant::WalCheckpointFlushMismatch && issue.occurrences == 1
    }));
}
