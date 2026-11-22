// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 统一 FileCatalog（PG 实现）集成测试：flush 幂等、多写入节点、epoch 栅栏、
//! compaction 替换冲突、索引重建槽位、tombstone 与延迟 GC 入队。
//!
//! 默认跳过（testcontainers 需要 docker）；`MS_RUN_IT=1` 启用：
//! ```bash
//! MS_RUN_IT=1 cargo test -p molesignal --test infra_it_file_catalog -- --nocapture
//! ```

mod common;

use std::time::Duration;

use domain::{
    iam::{Organization, OrganizationRepository},
    storage::{
        Artifact, ArtifactId, ArtifactRole, ArtifactState, ArtifactTypeId, ArtifactUpdate,
        CatalogInvariant, ColumnStats, CommitFlush, DataSegment, DatasetSelection, DatasetTypeId,
        FileCatalog, FlushProvenance, ObjectChecksum, OrganizationScope, OrphanedStreams,
        Partition, PartitionManifestPointer, PhysicalDatasetId, PublishDatasetTransform,
        PublishPartitionManifest, ReplaceSegments, SegmentId, SegmentState, SequenceRange,
        StoredObject, TombstoneSegments, UpdateArtifact, WalSequence, WalStreamRow,
        WalWriterRegistry, WriterEpoch, WriterNodeId, builtin_registry, is_stale_writer_epoch,
        type_id,
    },
    stream::{Schema, StreamDefinition, StreamRepository, StreamType},
};
use kernel::{
    ids::Id,
    time::{TimeRange, TimestampMicros},
};
use molesignal::infra::storage::layout::StorageLayout;
use postgres::{
    MetaStore,
    repositories::{
        file_catalog::PgFileCatalog, organizations::PgOrganizationRepository,
        streams::PgStreamRepository, wal_writers::PgWalWriterRegistry,
    },
};
use settings::MetaDatabaseSettings;
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
    _pg: Option<testcontainers::ContainerAsync<PgImage>>,
}

async fn harness() -> Harness {
    let (dsn, pg) = common::test_database().await;
    let store = MetaStore::connect(&MetaDatabaseSettings {
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
        domain::storage::StreamTypeId::LOGS,
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
        domain::storage::StreamTypeId::LOGS,
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

/// Opens the WAL stream `commit_flush` checks: writer `epoch` of `node`, publishing itself.
async fn open_stream(h: &Harness, node: &str, epoch: u64) {
    sqlx::query(
        "INSERT INTO wal_writers (writer_epoch, writer_node_id, started_at, lease_expires_at)
         VALUES ($1, $2, now(), now() + interval '1 hour') ON CONFLICT DO NOTHING",
    )
    .bind(epoch as i64)
    .bind(node)
    .execute(&h.pool)
    .await
    .expect("insert wal writer");

    PgWalWriterRegistry::new(h.pool.clone())
        .open_stream(&WalStreamRow {
            organization_id: h.scope.organization_id.clone(),
            dataset_id: h.dataset_id.clone(),
            dataset_type: DatasetTypeId::builtin(type_id::builtin::DATASET_LOG_RECORDS),
            writer_node_id: WriterNodeId::new(node),
            writer_epoch: WriterEpoch(epoch),
            publisher_epoch: WriterEpoch(epoch),
        })
        .await
        .expect("open wal stream");
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
    let mut p1 = provenance("node-a", 1, 1, 10);
    open_stream(&h, "node-a", 1).await;
    p1.collected_bytes = Some(12345);
    let seg1 = segment(&h, &p1, 1, true);
    let commit = CommitFlush {
        dataset_id: h.dataset_id.clone(),
        publisher_epoch: p1.writer_epoch,
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

    let usage = h
        .catalog
        .intake_usage(&h.scope)
        .await
        .expect("intake usage");
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].collected_bytes, Some(12345));
    assert!(usage[0].complete);

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
    open_stream(&h, "node-b", 1).await;
    let seg2 = segment(&h, &p2, 2, false);
    let other = h
        .catalog
        .commit_flush(
            &h.scope,
            CommitFlush {
                dataset_id: h.dataset_id.clone(),
                publisher_epoch: p2.writer_epoch,
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
async fn only_the_current_unfenced_publisher_commits_and_a_claim_moves_it() {
    if skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }
    let h = harness().await;
    let registry = PgWalWriterRegistry::new(h.pool.clone());
    let lease = Duration::from_secs(60);

    let a = registry
        .register(&WriterNodeId::new("node-a"), lease)
        .await
        .expect("register a");
    let b = registry
        .register(&WriterNodeId::new("node-b"), lease)
        .await
        .expect("register b");
    assert!(b > a, "epochs grow across the cluster");

    let row = WalStreamRow {
        organization_id: h.scope.organization_id.clone(),
        dataset_id: h.dataset_id.clone(),
        dataset_type: DatasetTypeId::builtin(type_id::builtin::DATASET_LOG_RECORDS),
        writer_node_id: WriterNodeId::new("node-a"),
        writer_epoch: a,
        publisher_epoch: a,
    };
    registry.open_stream(&row).await.expect("open stream");
    registry
        .open_stream(&row)
        .await
        .expect("open is idempotent");

    let commit = |provenance: &FlushProvenance, publisher: WriterEpoch, hour: i64| CommitFlush {
        dataset_id: h.dataset_id.clone(),
        provenance: provenance.clone(),
        publisher_epoch: publisher,
        segments: vec![segment(&h, provenance, hour, false)],
    };

    // The writer publishes its own stream; nobody else may while it holds it.
    let first = provenance("node-a", a.0, 1, 5);
    h.catalog
        .commit_flush(&h.scope, commit(&first, a, 1))
        .await
        .expect("writer publishes");
    let early = provenance("node-a", a.0, 6, 9);
    assert!(
        is_stale_writer_epoch(
            &h.catalog
                .commit_flush(&h.scope, commit(&early, b, 2))
                .await
                .unwrap_err()
        ),
        "a node that did not claim the stream is refused"
    );
    assert!(
        !registry.claim_stream(&row, b).await.expect("claim"),
        "a live writer's stream cannot be claimed"
    );

    // The lease lapses: the writer is fenced, renewing fails, its stream is work for b.
    sqlx::query("UPDATE wal_writers SET lease_expires_at = now() - interval '1 second' WHERE writer_epoch = $1")
        .bind(a.0 as i64)
        .execute(&h.pool)
        .await
        .expect("expire lease");
    assert_eq!(registry.fence_expired().await.expect("fence"), 1);
    assert!(
        !registry.renew(a, lease).await.expect("renew"),
        "a fenced writer stays fenced"
    );
    let orphaned = registry
        .orphaned_streams(b, &datasets(), 10)
        .await
        .expect("orphaned");
    assert_eq!(orphaned, vec![row.clone()]);

    // A trace candidate log of the same writer is asked for apart from the datasets.
    let candidate_log = WalStreamRow {
        dataset_id: PhysicalDatasetId::from(Id("trace-candidates".into())),
        dataset_type: molesignal::app::trace::durable::candidate_log_type(),
        ..row.clone()
    };
    registry
        .open_stream(&candidate_log)
        .await
        .expect("open candidate log");
    assert_eq!(
        registry
            .orphaned_streams(b, &datasets(), 10)
            .await
            .expect("datasets"),
        vec![row.clone()],
        "a dataset round never sees candidate logs"
    );
    assert_eq!(
        registry
            .orphaned_streams(
                b,
                &OrphanedStreams::Only(molesignal::app::trace::durable::candidate_log_type()),
                10,
            )
            .await
            .expect("candidate logs"),
        vec![candidate_log.clone()],
        "a trace round sees only candidate logs"
    );
    assert!(
        registry
            .close_stream(
                &candidate_log.organization_id,
                &candidate_log.dataset_id,
                a,
                a
            )
            .await
            .expect("close candidate log"),
        "its publisher closes it"
    );
    assert!(
        is_stale_writer_epoch(
            &h.catalog
                .commit_flush(&h.scope, commit(&early, a, 2))
                .await
                .unwrap_err()
        ),
        "a fenced writer is refused"
    );

    // b claims and publishes under a's identity; a retried range is a no-op.
    assert!(registry.claim_stream(&row, b).await.expect("claim"));
    assert!(
        registry.claim_stream(&row, b).await.expect("claim again"),
        "a claim is resumable"
    );
    let taken = provenance("node-a", a.0, 6, 9);
    assert!(
        !h.catalog
            .commit_flush(&h.scope, commit(&taken, b, 3))
            .await
            .expect("claimant publishes")
            .already_committed
    );
    assert!(
        h.catalog
            .commit_flush(&h.scope, commit(&taken, b, 3))
            .await
            .expect("retry")
            .already_committed
    );
    assert_eq!(
        registry
            .orphaned_streams(b, &datasets(), 10)
            .await
            .expect("resumable")[0]
            .publisher_epoch,
        b,
        "an unfinished claim stays b's work"
    );

    // b dies too: c takes the claim over from it.
    let c = registry
        .register(&WriterNodeId::new("node-c"), lease)
        .await
        .expect("register c");
    registry.release(b).await.expect("release b");
    assert!(registry.claim_stream(&row, c).await.expect("claim from b"));
    assert!(
        is_stale_writer_epoch(
            &h.catalog
                .commit_flush(&h.scope, commit(&provenance("node-a", a.0, 10, 12), b, 4))
                .await
                .unwrap_err()
        ),
        "the superseded claimant is refused"
    );

    // Finished: the stream closes only for its publisher, then the fenced writers go.
    assert!(
        !registry
            .close_stream(&row.organization_id, &row.dataset_id, a, b)
            .await
            .unwrap()
    );
    assert!(
        registry
            .close_stream(&row.organization_id, &row.dataset_id, a, c)
            .await
            .unwrap()
    );
    assert_eq!(registry.forget_finished_writers().await.expect("forget"), 2);
    assert!(registry.renew(c, lease).await.expect("c lives on"));
    assert!(
        is_stale_writer_epoch(
            &h.catalog
                .commit_flush(&h.scope, commit(&provenance("node-a", a.0, 13, 14), c, 5))
                .await
                .unwrap_err()
        ),
        "a closed stream takes no more publications"
    );
}

#[tokio::test]
async fn manifest_generation_switches_atomically_and_can_retire_partition() {
    if skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }
    let h = harness().await;
    let provenance = provenance("node-a", 1, 1, 10);
    open_stream(&h, "node-a", 1).await;
    let segment = segment(&h, &provenance, 1, false);
    h.catalog
        .commit_flush(
            &h.scope,
            CommitFlush {
                dataset_id: h.dataset_id.clone(),
                publisher_epoch: provenance.writer_epoch,
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
                domain::storage::StreamTypeId::LOGS,
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
                replace_artifacts: Vec::new(),
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
                replace_artifacts: Vec::new(),
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

    open_stream(&h, "node-a", 1).await;
    let input = segment(&h, &provenance, 1, false);
    h.catalog
        .commit_flush(
            &h.scope,
            CommitFlush {
                dataset_id: h.dataset_id.clone(),
                publisher_epoch: provenance.writer_epoch,
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
                domain::storage::StreamTypeId::LOGS,
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
                replace_artifacts: Vec::new(),
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
    open_stream(&h, "node-a", 1).await;
    h.catalog
        .commit_flush(
            &h.scope,
            CommitFlush {
                dataset_id: h.dataset_id.clone(),
                publisher_epoch: provenance.writer_epoch,
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

/// What a dataset takeover round asks for: every orphaned stream but trace candidate logs.
fn datasets() -> OrphanedStreams {
    OrphanedStreams::Except(molesignal::app::trace::durable::candidate_log_type())
}
