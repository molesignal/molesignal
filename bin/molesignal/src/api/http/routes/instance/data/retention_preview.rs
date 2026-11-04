// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 调短数据流默认保留期之前的清理预览：这次改动会多清理掉什么，按数据流给出行数、字节数和最早
//! 时间。
//!
//! 只看文件目录和封存分区清单里的元数据，不打开数据文件，所以是估算：清理在之后的某一轮才发生，
//! 那时又有文件过了截止时间，合并也可能改变文件的划分。RUM 回放和 profile 归档跟同一个设置走，
//! 但不在文件目录里，不计入。

use axum::{
    Extension, Json,
    extract::{Query, State},
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};

use super::validate_stream_retention_days;
use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{
        iam::{Organization, permission},
        storage::PhysicalDatasetId,
        stream::{StreamDefinition, StreamRepository, StreamType},
    },
    infra::query::catalog_source::{CatalogQuerySource, SegmentSize},
    shared::{
        Result,
        time::{TimeRange, TimestampMicros},
    },
};

const DAY_MICROS: i64 = 24 * 60 * 60 * 1_000_000;
/// 响应里最多列出多少个数据流，大的在前；合计不受它限制。
const MAX_LISTED_STREAMS: usize = 100;
/// 同时查多少个数据流的数据集。
const DATASET_LOOKUPS: usize = 4;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PreviewParams {
    retention_days: u32,
}

/// 被清理的量。一个 Segment 都没有时 `earliest_at_micros` 是 `None`。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Tally {
    rows: u64,
    stored_bytes: u64,
    earliest_at: Option<i64>,
}

impl Tally {
    fn add(&mut self, other: &Tally) {
        self.rows = self.rows.saturating_add(other.rows);
        self.stored_bytes = self.stored_bytes.saturating_add(other.stored_bytes);

        self.earliest_at = match (self.earliest_at, other.earliest_at) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (left, right) => left.or(right),
        };
    }

    fn add_segment(&mut self, segment: &SegmentSize) {
        self.add(&Tally {
            rows: segment.rows,
            stored_bytes: segment.stored_bytes,
            earliest_at: Some(segment.time_range.start.0),
        });
    }
}

/// 一个数据流会多清理掉的量。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct StreamImpact {
    org_id: String,
    org_name: String,
    id: String,
    name: String,
    stream_type: StreamType,
    #[serde(flatten)]
    tally: Tally,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StreamRetentionPreview {
    retention_days: u32,
    previous_retention_days: u32,
    /// 总是 `true`：见模块说明。展示时要说明这是估算。
    estimated: bool,
    generated_at: i64,
    #[serde(flatten)]
    total: Tally,
    /// 受影响的数据流有多少个；`streams` 只列其中最大的一些。
    stream_count: usize,
    streams: Vec<StreamImpact>,
    /// 读不出目录、没有算进来的数据流有多少个。不是 0 时实际影响可能更大。
    unavailable_streams: usize,
}

/// 把保留期从 `previous_days` 调短到 `days` 让默认保留期的数据流多清理的范围。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Shortening {
    previous_days: u32,
    days: u32,
    previous_cutoff: i64,
    cutoff: i64,
}

impl Shortening {
    /// 调长或不变时不会多清理什么，是 `None`。
    fn new(previous_days: u32, days: u32, now: TimestampMicros) -> Option<Self> {
        let cutoff = |days: u32| {
            now.0
                .saturating_sub(i64::from(days).saturating_mul(DAY_MICROS))
        };

        (days < previous_days).then(|| Self {
            previous_days,
            days,
            previous_cutoff: cutoff(previous_days),
            cutoff: cutoff(days),
        })
    }

    /// 这个设置管不管这个数据流：自己设了保留期的不受影响。
    fn moves(&self, stream: &StreamDefinition) -> bool {
        stream.effective_retention_days(self.previous_days)
            != stream.effective_retention_days(self.days)
    }

    /// 与 `Compactor::retention_sweep` 同一条规则：Segment 整个过了截止时间才清理。早就过了旧的
    /// 截止时间的本来也要清理，不算这次改动的影响。
    fn removes(&self, segment: &SegmentSize) -> bool {
        let end = segment.time_range.end.0;

        self.previous_cutoff < end && end <= self.cutoff
    }

    fn range(&self) -> TimeRange {
        TimeRange::new(
            TimestampMicros(self.previous_cutoff),
            TimestampMicros(self.cutoff),
        )
    }
}

#[derive(Debug, Default)]
struct Estimate {
    streams: Vec<StreamImpact>,
    unavailable_streams: usize,
}

#[permission("sys.settings.retention.read")]
pub(super) async fn handle(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(params): Query<PreviewParams>,
) -> Result<Json<StreamRetentionPreview>> {
    validate_stream_retention_days(params.retention_days)?;

    let settings = state.iam.instance_settings.get().await?;
    let previous_days = settings.default_retention_days;
    let generated_at = TimestampMicros::now();

    let found = match Shortening::new(previous_days, params.retention_days, generated_at) {
        Some(shortening) => {
            let orgs = state.iam.service.orgs.list().await?;

            estimate(
                &state.storage.catalog_query,
                state.telemetry.streams.as_ref(),
                &orgs,
                shortening,
            )
            .await?
        }
        None => Estimate::default(),
    };

    Ok(Json(preview(
        params.retention_days,
        previous_days,
        generated_at,
        found,
    )))
}

fn preview(
    retention_days: u32,
    previous_retention_days: u32,
    generated_at: TimestampMicros,
    found: Estimate,
) -> StreamRetentionPreview {
    let Estimate {
        mut streams,
        unavailable_streams,
    } = found;

    let mut total = Tally::default();

    for stream in &streams {
        total.add(&stream.tally);
    }

    streams.sort_by(|left, right| {
        right
            .tally
            .stored_bytes
            .cmp(&left.tally.stored_bytes)
            .then(right.tally.rows.cmp(&left.tally.rows))
            .then_with(|| left.org_name.cmp(&right.org_name))
            .then_with(|| left.name.cmp(&right.name))
    });

    let stream_count = streams.len();
    streams.truncate(MAX_LISTED_STREAMS);

    StreamRetentionPreview {
        retention_days,
        previous_retention_days,
        estimated: true,
        generated_at: generated_at.0,
        total,
        stream_count,
        streams,
        unavailable_streams,
    }
}

async fn estimate(
    catalog: &CatalogQuerySource,
    streams: &dyn StreamRepository,
    orgs: &[Organization],
    shortening: Shortening,
) -> Result<Estimate> {
    let mut found = Estimate::default();

    // 和 compactor 走的范围一样：所有组织，包括平台组织和已停用的组织。
    for org in orgs {
        let moved = streams
            .list(&org.id)
            .await?
            .into_iter()
            .filter(|stream| shortening.moves(stream))
            .collect::<Vec<_>>();

        estimate_org(catalog, org, moved, shortening, &mut found).await;
    }

    Ok(found)
}

async fn estimate_org(
    catalog: &CatalogQuerySource,
    org: &Organization,
    streams: Vec<StreamDefinition>,
    shortening: Shortening,
    found: &mut Estimate,
) {
    let looked_up = futures::stream::iter(streams)
        .map(|stream| async move {
            let datasets = catalog.active_datasets(&stream).await;

            (stream, datasets)
        })
        .buffered(DATASET_LOOKUPS)
        .collect::<Vec<_>>()
        .await;

    let mut selected: Vec<(StreamDefinition, Vec<PhysicalDatasetId>)> = Vec::new();

    for (stream, datasets) in looked_up {
        match datasets {
            // 数据集自己设了保留期的（比如预聚合）不跟默认保留期走。
            Ok(datasets) => selected.push((
                stream,
                datasets
                    .into_iter()
                    .filter(|dataset| dataset.storage_policy.retention_days.is_none())
                    .map(|dataset| dataset.id)
                    .collect(),
            )),
            Err(error) => {
                tracing::warn!(
                    org_id = %org.id.0,
                    stream = %stream.name,
                    %error,
                    "retention preview could not list the datasets of a stream"
                );

                found.unavailable_streams += 1;
            }
        }
    }

    let dataset_ids = selected
        .iter()
        .flat_map(|(_, dataset_ids)| dataset_ids.iter().cloned())
        .collect();

    let mut sizes = match catalog
        .segment_sizes(&org.id, dataset_ids, shortening.range())
        .await
    {
        Ok(sizes) => sizes,
        Err(error) => {
            tracing::warn!(org_id = %org.id.0, %error, "retention preview could not read the catalog");

            found.unavailable_streams += selected
                .iter()
                .filter(|(_, dataset_ids)| !dataset_ids.is_empty())
                .count();

            return;
        }
    };

    for (stream, dataset_ids) in selected {
        let mut tally = Tally::default();
        let mut readable = true;

        for dataset_id in dataset_ids {
            match sizes.remove(&dataset_id) {
                Some(Ok(segments)) => {
                    for segment in segments
                        .iter()
                        .filter(|segment| shortening.removes(segment))
                    {
                        tally.add_segment(segment);
                    }
                }
                Some(Err(error)) => {
                    tracing::warn!(
                        org_id = %org.id.0,
                        stream = %stream.name,
                        %error,
                        "retention preview could not read a sealed manifest"
                    );

                    readable = false;
                }
                None => {}
            }
        }

        if !readable {
            found.unavailable_streams += 1;
        } else if tally.earliest_at.is_some() {
            found.streams.push(StreamImpact {
                org_id: org.id.0.clone(),
                org_name: org.name.clone(),
                id: stream.id.0,
                name: stream.name,
                stream_type: stream.stream_type,
                tally,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, sync::Arc};

    use object_store::{ObjectStore, ObjectStoreExt, memory::InMemory, path::Path};
    use serde_json::json;

    use super::*;
    use crate::{
        bootstrap::test_support::FixedStreams,
        config::{CompactorSettings, ObjectCacheSettings},
        domain::{
            storage::{
                Artifact, ArtifactId, ArtifactRole, ArtifactState, ArtifactTypeId, ColumnStats,
                DataSegment, DatasetSelection, DatasetTypeId, FileCatalog, IndexPolicy,
                ObjectChecksum, ObjectKey, OrganizationScope, Partition, PartitionManifest,
                PartitionManifestPointer, PartitionPolicy, PhysicalDataset, PhysicalDatasetSpec,
                SegmentId, SegmentState, StoragePolicy, StoredObject, WalCodecId, type_id::builtin,
            },
            stream::{Retention, Schema},
        },
        infra::{
            intake::{BufferPool, dataset_resolver::test_support::StubFileCatalog},
            storage::{
                compactor::Compactor,
                manifest::{PartitionManifestReader, encode_manifest},
                object_reader::ObjectReader,
                parquet::writer::ParquetWriter,
            },
        },
        shared::ids::Id,
    };

    const NOW: i64 = 1_800_000_000_000_000;
    const DATA_BYTES: u64 = 100;
    const INDEX_BYTES: u64 = 20;
    const ROWS: u64 = 10;

    fn days_ago(days: i64) -> i64 {
        NOW - days * DAY_MICROS
    }

    fn org(id: &str) -> Organization {
        Organization {
            id: Id::from_string(id),
            name: format!("Org {id}"),
            slug: id.into(),
            system: false,
            disabled: false,
            created_at: TimestampMicros(0),
        }
    }

    fn stream(org_id: &str, name: &str, own_days: Option<u32>) -> StreamDefinition {
        StreamDefinition {
            id: Id::from_string(format!("{org_id}-{name}")),
            org_id: Id::from_string(org_id),
            name: name.into(),
            stream_type: StreamType::LOGS,
            schema: Schema { fields: Vec::new() },
            retention: own_days.map(|days| Retention { days }),
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    fn artifact(id: &str, role: ArtifactRole, source: Option<&Artifact>, size: u64) -> Artifact {
        Artifact {
            id: ArtifactId::from_string(id),
            role,
            artifact_type: ArtifactTypeId::builtin(if source.is_some() {
                builtin::ARTIFACT_TANTIVY
            } else {
                builtin::ARTIFACT_PARQUET
            }),
            format_version: 1,
            object: StoredObject {
                key: ObjectKey::from_string(format!("objects/{id}")),
                size_bytes: size,
                checksum: ObjectChecksum::from_string("test"),
                etag: None,
            },
            source_artifact_id: source.map(|artifact| artifact.id.clone()),
            source_checksum: source.map(|artifact| artifact.object.checksum.clone()),
            schema_fingerprint: None,
            state: ArtifactState::Ready,
            failure_reason: None,
        }
    }

    /// 一小时长的 Segment，结束在 `end`；一个数据文件加一个索引。
    fn segment(
        dataset: &PhysicalDataset,
        id: &str,
        end: i64,
        partition: Partition,
        state: SegmentState,
    ) -> DataSegment {
        let primary = artifact(id, ArtifactRole::PrimaryData, None, DATA_BYTES);
        let index_id = format!("{id}-index");
        let index = artifact(&index_id, ArtifactRole::Index, Some(&primary), INDEX_BYTES);

        DataSegment {
            id: SegmentId::from_string(id),
            organization_id: dataset.organization_id.clone(),
            dataset_id: dataset.id.clone(),
            partition,
            time_range: TimeRange::new(
                TimestampMicros(end - DAY_MICROS / 24),
                TimestampMicros(end),
            ),
            sequence_range: None,
            row_count: ROWS,
            schema_fingerprint: None,
            column_stats: ColumnStats::default(),
            flush_id: None,
            output_ordinal: 0,
            primary,
            auxiliaries: vec![index],
            state,
            visible_from_version: 1,
            retired_at_version: None,
            created_at_micros: 0,
        }
    }

    fn partition_of(end: i64) -> Partition {
        Partition {
            start_micros: end - DAY_MICROS,
            end_micros: end + DAY_MICROS,
            shard: 0,
        }
    }

    struct Fixture {
        catalog: Arc<StubFileCatalog>,
        source: CatalogQuerySource,
        reader: Arc<ObjectReader>,
        store: Arc<dyn ObjectStore>,
    }

    fn fixture() -> Fixture {
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());

        let reader =
            ObjectReader::build(store.clone(), "local", &ObjectCacheSettings::default()).unwrap();

        let catalog = Arc::new(StubFileCatalog::default());

        let source = CatalogQuerySource::new(
            catalog.clone(),
            Arc::new(BufferPool::new()),
            Arc::new(FixedStreams(Vec::new())),
            reader.clone(),
            Arc::new(PartitionManifestReader::new(reader.clone(), 1024 * 1024)),
        );

        Fixture {
            catalog,
            source,
            reader,
            store,
        }
    }

    impl Fixture {
        async fn dataset(
            &self,
            stream: &StreamDefinition,
            dataset_type: &'static str,
            own_days: Option<u32>,
        ) -> PhysicalDataset {
            self.catalog
                .ensure_datasets(
                    &OrganizationScope::new(stream.org_id.clone()),
                    &stream.id,
                    &[PhysicalDatasetSpec {
                        dataset_type: DatasetTypeId::builtin(dataset_type),
                        dataset_type_version: 1,
                        wal_codec: WalCodecId::builtin(builtin::WAL_CODEC_ROW_BATCH),
                        partition_policy: PartitionPolicy::default(),
                        storage_policy: StoragePolicy {
                            retention_days: own_days,
                            seal_after_hours: None,
                        },
                        index_policy: IndexPolicy::default(),
                    }],
                )
                .await
                .unwrap()
                .remove(0)
        }

        fn hot(&self, dataset: &PhysicalDataset, id: &str, end: i64) {
            self.catalog.seed_segment(segment(
                dataset,
                id,
                end,
                partition_of(end),
                SegmentState::Active,
            ));
        }

        /// 一个封存分区，里面一个结束在 `end` 的 Segment。
        async fn sealed(&self, dataset: &PhysicalDataset, id: &str, end: i64) {
            let partition = partition_of(end);
            let key = ObjectKey::from_string(format!("manifests/{id}"));

            let bytes = encode_manifest(&PartitionManifest {
                organization_id: dataset.organization_id.clone(),
                dataset_id: dataset.id.clone(),
                partition,
                generation: 1,
                segments: vec![segment(dataset, id, end, partition, SegmentState::Sealed)],
            })
            .unwrap();

            let size_bytes = bytes.len() as u64;

            self.store
                .put(&Path::from(key.as_str()), bytes.into())
                .await
                .unwrap();

            self.point_at_manifest(dataset, partition, key, size_bytes);
        }

        fn point_at_manifest(
            &self,
            dataset: &PhysicalDataset,
            partition: Partition,
            key: ObjectKey,
            size_bytes: u64,
        ) {
            self.catalog.seed_manifest(PartitionManifestPointer {
                organization_id: dataset.organization_id.clone(),
                dataset_id: dataset.id.clone(),
                partition,
                generation: 1,
                object: StoredObject {
                    key,
                    size_bytes,
                    checksum: ObjectChecksum::from_string("test"),
                    etag: None,
                },
                segment_count: 1,
                created_at_micros: 0,
            });
        }

        async fn estimate(
            &self,
            orgs: &[Organization],
            streams: Vec<StreamDefinition>,
            shortening: Shortening,
        ) -> Estimate {
            estimate(&self.source, &FixedStreams(streams), orgs, shortening)
                .await
                .unwrap()
        }
    }

    fn thirty_to_seven() -> Shortening {
        Shortening::new(30, 7, TimestampMicros(NOW)).unwrap()
    }

    #[test]
    fn only_a_shorter_retention_removes_more() {
        let now = TimestampMicros(NOW);

        assert_eq!(Shortening::new(30, 30, now), None);
        assert_eq!(Shortening::new(30, 90, now), None);

        let shortening = thirty_to_seven();

        assert_eq!(shortening.previous_cutoff, days_ago(30));
        assert_eq!(shortening.cutoff, days_ago(7));
    }

    #[test]
    fn counts_a_segment_by_its_end_and_leaves_what_was_going_anyway() {
        let shortening = thirty_to_seven();

        let ending = |end: i64| SegmentSize {
            time_range: TimeRange::new(TimestampMicros(end - 1), TimestampMicros(end)),
            rows: 1,
            stored_bytes: 1,
        };

        assert!(shortening.removes(&ending(shortening.cutoff)));
        assert!(shortening.removes(&ending(shortening.previous_cutoff + 1)));

        // 跨过新的截止时间的还留着；过了旧的截止时间的不改也要清理。
        assert!(!shortening.removes(&ending(shortening.cutoff + 1)));
        assert!(!shortening.removes(&ending(shortening.previous_cutoff)));
    }

    #[test]
    fn a_stream_with_a_retention_of_its_own_is_left_alone() {
        let shortening = thirty_to_seven();

        assert!(shortening.moves(&stream("a", "follows", None)));
        assert!(shortening.moves(&stream("a", "zero-is-unset", Some(0))));
        assert!(!shortening.moves(&stream("a", "own", Some(14))));
        assert!(!shortening.moves(&stream("a", "own-same-as-new", Some(7))));
    }

    #[test]
    fn totals_every_stream_and_lists_the_largest_first() {
        let impact = |name: &str, stored_bytes: u64, earliest: i64| StreamImpact {
            org_id: "a".into(),
            org_name: "Org a".into(),
            id: name.into(),
            name: name.into(),
            stream_type: StreamType::LOGS,
            tally: Tally {
                rows: 1,
                stored_bytes,
                earliest_at: Some(earliest),
            },
        };

        let mut streams = (0..MAX_LISTED_STREAMS)
            .map(|index| impact(&format!("small-{index:03}"), 1, 50))
            .collect::<Vec<_>>();

        streams.push(impact("largest", 9, 10));

        let shown = preview(
            7,
            30,
            TimestampMicros(NOW),
            Estimate {
                streams,
                unavailable_streams: 2,
            },
        );

        assert!(shown.estimated);
        assert_eq!(shown.stream_count, MAX_LISTED_STREAMS + 1);
        assert_eq!(shown.streams.len(), MAX_LISTED_STREAMS);
        assert_eq!(shown.streams[0].name, "largest");
        assert_eq!(shown.total.rows, MAX_LISTED_STREAMS as u64 + 1);
        assert_eq!(shown.total.stored_bytes, MAX_LISTED_STREAMS as u64 + 9);
        assert_eq!(shown.total.earliest_at, Some(10));
        assert_eq!(shown.unavailable_streams, 2);
    }

    #[test]
    fn says_it_is_an_estimate_and_flattens_the_figures() {
        let value = serde_json::to_value(preview(
            7,
            30,
            TimestampMicros(NOW),
            Estimate {
                streams: vec![StreamImpact {
                    org_id: "a".into(),
                    org_name: "Org a".into(),
                    id: "s".into(),
                    name: "app".into(),
                    stream_type: StreamType::LOGS,
                    tally: Tally {
                        rows: 3,
                        stored_bytes: 4,
                        earliest_at: Some(5),
                    },
                }],
                unavailable_streams: 0,
            },
        ))
        .unwrap();

        assert_eq!(
            value,
            json!({
                "retentionDays": 7,
                "previousRetentionDays": 30,
                "estimated": true,
                "generatedAt": NOW,
                "rows": 3,
                "storedBytes": 4,
                "earliestAt": 5,
                "streamCount": 1,
                "streams": [{
                    "orgId": "a",
                    "orgName": "Org a",
                    "id": "s",
                    "name": "app",
                    "streamType": "logs",
                    "rows": 3,
                    "storedBytes": 4,
                    "earliestAt": 5,
                }],
                "unavailableStreams": 0,
            }),
        );
    }

    #[test]
    fn nothing_found_has_no_earliest_time() {
        let value =
            serde_json::to_value(preview(90, 30, TimestampMicros(NOW), Estimate::default()))
                .unwrap();

        assert_eq!(value["rows"], 0);
        assert_eq!(value["storedBytes"], 0);
        assert_eq!(value["earliestAt"], serde_json::Value::Null);
        assert_eq!(value["streamCount"], 0);
        assert_eq!(value["streams"], json!([]));
    }

    #[test]
    fn days_outside_the_bounds_of_the_setting_are_refused() {
        assert!(validate_stream_retention_days(0).is_err());
        assert!(validate_stream_retention_days(1).is_ok());
        assert!(validate_stream_retention_days(3650).is_ok());
        assert!(validate_stream_retention_days(3651).is_err());
    }

    #[tokio::test]
    async fn adds_up_hot_and_sealed_segments_with_their_indexes_by_stream() {
        let fixture = fixture();
        let app = stream("a", "app", None);
        let quiet = stream("a", "quiet", None);
        let records = fixture
            .dataset(&app, builtin::DATASET_LOG_RECORDS, None)
            .await;
        let idle = fixture
            .dataset(&quiet, builtin::DATASET_LOG_RECORDS, None)
            .await;

        fixture.hot(&records, "hot-goes", days_ago(10));
        fixture.sealed(&records, "sealed-goes", days_ago(20)).await;
        fixture.hot(&records, "recent", days_ago(1));
        fixture.hot(&idle, "idle-recent", days_ago(1));

        let found = fixture
            .estimate(&[org("a")], vec![app, quiet], thirty_to_seven())
            .await;

        assert_eq!(found.unavailable_streams, 0);
        assert_eq!(found.streams.len(), 1);
        assert_eq!(found.streams[0].name, "app");
        assert_eq!(found.streams[0].org_name, "Org a");
        assert_eq!(found.streams[0].tally.rows, 2 * ROWS);
        assert_eq!(
            found.streams[0].tally.stored_bytes,
            2 * (DATA_BYTES + INDEX_BYTES),
        );
        assert_eq!(
            found.streams[0].tally.earliest_at,
            Some(days_ago(20) - DAY_MICROS / 24),
        );
    }

    #[tokio::test]
    async fn leaves_out_streams_and_datasets_with_a_retention_of_their_own() {
        let fixture = fixture();
        let own = stream("a", "own", Some(90));
        let follows = stream("a", "follows", None);
        let own_records = fixture
            .dataset(&own, builtin::DATASET_LOG_RECORDS, None)
            .await;
        let records = fixture
            .dataset(&follows, builtin::DATASET_METRIC_SAMPLES, None)
            .await;
        let rollup = fixture
            .dataset(&follows, builtin::DATASET_METRIC_ROLLUP, Some(365))
            .await;

        fixture.hot(&own_records, "own", days_ago(10));
        fixture.hot(&records, "follows", days_ago(10));
        fixture.hot(&rollup, "rollup", days_ago(10));

        let found = fixture
            .estimate(&[org("a")], vec![own, follows], thirty_to_seven())
            .await;

        assert_eq!(found.streams.len(), 1);
        assert_eq!(found.streams[0].name, "follows");
        assert_eq!(found.streams[0].tally.rows, ROWS);
    }

    #[tokio::test]
    async fn a_manifest_that_cannot_be_read_marks_its_stream_and_not_the_others() {
        let fixture = fixture();
        let broken = stream("a", "broken", None);
        let fine = stream("a", "fine", None);
        let broken_records = fixture
            .dataset(&broken, builtin::DATASET_LOG_RECORDS, None)
            .await;
        let fine_records = fixture
            .dataset(&fine, builtin::DATASET_LOG_RECORDS, None)
            .await;

        fixture.hot(&broken_records, "broken-hot", days_ago(10));
        fixture.point_at_manifest(
            &broken_records,
            partition_of(days_ago(20)),
            ObjectKey::from_string("manifests/missing"),
            1,
        );
        fixture.hot(&fine_records, "fine-hot", days_ago(10));

        let found = fixture
            .estimate(&[org("a")], vec![broken, fine], thirty_to_seven())
            .await;

        assert_eq!(found.unavailable_streams, 1);
        assert_eq!(found.streams.len(), 1);
        assert_eq!(found.streams[0].name, "fine");
    }

    #[tokio::test]
    async fn every_organization_is_counted_under_its_own_name() {
        let fixture = fixture();
        let first = stream("a", "app", None);
        let second = stream("b", "app", None);
        let first_records = fixture
            .dataset(&first, builtin::DATASET_LOG_RECORDS, None)
            .await;
        let second_records = fixture
            .dataset(&second, builtin::DATASET_LOG_RECORDS, None)
            .await;

        fixture.hot(&first_records, "a-goes", days_ago(10));
        fixture.hot(&second_records, "b-goes", days_ago(10));

        let found = fixture
            .estimate(
                &[org("a"), org("b")],
                vec![first, second],
                thirty_to_seven(),
            )
            .await;

        let names = found
            .streams
            .iter()
            .map(|stream| stream.org_name.as_str())
            .collect::<BTreeSet<_>>();

        assert_eq!(names, BTreeSet::from(["Org a", "Org b"]));
    }

    /// 预览说会多清理的，正是把设置改掉之后 compactor 真正清理掉的。
    #[tokio::test]
    async fn names_exactly_what_the_sweep_then_removes() {
        let fixture = fixture();
        let app = stream("a", "app", None);
        let records = fixture
            .dataset(&app, builtin::DATASET_LOG_RECORDS, None)
            .await;
        let rollup = fixture
            .dataset(&app, builtin::DATASET_METRIC_ROLLUP, Some(365))
            .await;
        let now = TimestampMicros::now().0;
        let ago = |days: i64| now - days * DAY_MICROS;

        for (id, end) in [
            ("already-expired", ago(40)),
            ("goes-a", ago(25)),
            ("goes-b", ago(8)),
            ("recent", ago(3)),
        ] {
            fixture.hot(&records, id, end);
        }

        fixture.hot(&rollup, "rollup-own-retention", ago(25));

        let found = fixture
            .estimate(
                &[org("a")],
                vec![app.clone()],
                Shortening::new(30, 7, TimestampMicros(now)).unwrap(),
            )
            .await;

        assert_eq!(found.streams[0].tally.rows, 2 * ROWS);

        let compactor = Compactor::new(
            fixture.catalog.clone(),
            fixture.reader.clone(),
            Arc::new(PartitionManifestReader::new(
                fixture.reader.clone(),
                1024 * 1024,
            )),
            Arc::new(ParquetWriter::new(fixture.store.clone())),
            fixture.store.clone(),
            CompactorSettings::default(),
            3600,
        );

        // 改之前这一轮只清理本来就过期的；改之后多清理的就是预览里的两个。
        assert_eq!(compactor.retention_sweep(&app, 30).await.unwrap(), 1);
        assert_eq!(compactor.retention_sweep(&app, 7).await.unwrap(), 2);

        let left = fixture
            .catalog
            .snapshot(
                &OrganizationScope::new(app.org_id.clone()),
                DatasetSelection {
                    dataset_ids: vec![records.id.clone(), rollup.id.clone()],
                    time_range: TimeRange::new(
                        TimestampMicros(i64::MIN),
                        TimestampMicros(i64::MAX),
                    ),
                    partition_shard: None,
                },
            )
            .await
            .unwrap()
            .datasets
            .iter()
            .flat_map(|dataset| &dataset.segments)
            .map(|segment| segment.id.as_str().to_owned())
            .collect::<BTreeSet<_>>();

        assert_eq!(
            left,
            BTreeSet::from(["recent".to_owned(), "rollup-own-retention".to_owned()]),
        );
    }
}
