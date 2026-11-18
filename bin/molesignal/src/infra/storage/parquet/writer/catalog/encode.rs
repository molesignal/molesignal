// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The CPU half of writing a segment: sorting its rows, encoding the Parquet file, building the
//! index archive, the filters, the column statistics and the minute counts. It runs on a blocking thread, so a flush
//! or a compaction does not hold an async worker while it works; the uploads that follow stay on
//! the runtime.
//!
//! Once the rows are sorted, the Parquet file, the index archive and the filters with the column
//! statistics and the minute counts each read them on their own and are made at the same time. One stream's flushes run
//! one at a time: with the stages taken one after another, a busy stream would be flushed only as
//! fast as one thread encodes.

use std::thread::{self, ScopedJoinHandle};

use arrow::array::RecordBatch;
use bytes::Bytes;
use domain::{
    storage::{
        ArtifactId, ArtifactTypeId, ColumnStats, IndexTypeId, ObjectChecksum, ObjectKey, Partition,
        PhysicalDataset, SchemaFingerprint, SegmentId, type_id,
    },
    stream::{Clustering, StreamDefinition},
};
use kernel::{
    Error, Result,
    time::{TimeRange, TimestampMicros},
};

use super::{
    super::{
        build_tantivy_bytes_for_batch, column_stats::ColumnStatsBuilder, encode_parquet,
        metadata::timestamp_range,
    },
    Piece, checksum, schema_fingerprint, truncate_failure,
};
use crate::infra::storage::{
    filters::SegmentFilters,
    layout::StorageLayout,
    minute_counts::{MinuteCounts, MinuteCountsBuilder},
    parquet::clustering::{applied, sort_for_storage_clustered},
    write_metrics::{EncodeStep, timed},
};

/// The index archive of a segment, or why it could not be built.
pub(super) enum IndexBuild {
    Ready {
        artifact_id: ArtifactId,
        artifact_type: ArtifactTypeId,
        key: ObjectKey,
        bytes: Bytes,
        checksum: ObjectChecksum,
    },
    Failed {
        artifact_id: ArtifactId,
        artifact_type: ArtifactTypeId,
        key: ObjectKey,
        reason: String,
    },
}

/// One segment's rows turned into what is uploaded and recorded for it.
pub(super) struct EncodedPiece {
    pub(super) partition: Partition,
    pub(super) time_range: TimeRange,
    pub(super) segment_id: SegmentId,
    pub(super) primary_id: ArtifactId,
    pub(super) primary_key: ObjectKey,
    pub(super) parquet: Bytes,
    pub(super) parquet_checksum: ObjectChecksum,
    pub(super) schema_fingerprint: SchemaFingerprint,
    pub(super) index: Option<IndexBuild>,
    pub(super) filters: Option<SegmentFilters>,
    pub(super) minute_counts: Option<MinuteCounts>,
    pub(super) column_stats: ColumnStats,
    pub(super) row_count: u64,
}

/// Encodes `piece` on a thread of its own, below request handling in priority (see
/// [`background`](crate::infra::storage::background)).
pub(super) async fn encode_piece(
    stream: &StreamDefinition,
    dataset: &PhysicalDataset,
    piece: Piece,
) -> Result<EncodedPiece> {
    let stream = stream.clone();
    let dataset = dataset.clone();

    crate::infra::storage::background::run("segment-encode", move || {
        encode(&stream, &dataset, piece)
    })
    .await?
}

fn encode(
    stream: &StreamDefinition,
    dataset: &PhysicalDataset,
    piece: Piece,
) -> Result<EncodedPiece> {
    let Piece {
        batch,
        shard,
        sharding,
        clustering,
    } = piece;

    // A file records the clustering only where it applies, so that it never claims an order its
    // rows do not have.
    let clustering = applied(
        batch.schema().as_ref(),
        &dataset.dataset_type,
        clustering.as_ref(),
    )
    .cloned();

    let batch = timed(EncodeStep::Sort, || {
        sort_for_storage_clustered(batch, &dataset.dataset_type, clustering.as_ref())
    })?;
    let (start_micros, end_micros) = timestamp_range(&batch)?;
    let partition_start = dataset.partition_policy.bucket_start_micros(start_micros);

    let partition = Partition {
        start_micros: partition_start,
        end_micros: partition_start
            .checked_add(dataset.partition_policy.granularity.micros())
            .ok_or_else(|| Error::invalid("partition end overflows i64"))?,
        shard,
    };

    let segment_id = SegmentId::generate();
    let primary_id = ArtifactId::generate();
    let parquet_type = ArtifactTypeId::builtin(type_id::builtin::ARTIFACT_PARQUET);

    let primary_key = StorageLayout::artifact_key(
        &dataset.organization_id,
        stream.stream_type,
        &dataset.id,
        &partition,
        &segment_id,
        &primary_id,
        &parquet_type,
    );

    let wants_tantivy = dataset
        .index_policy
        .indexers
        .iter()
        .any(|index| index == &IndexTypeId::builtin(type_id::builtin::INDEX_TANTIVY));

    let (parquet, archive, (filters, mut column_stats, minute_counts)) = thread::scope(|scope| {
        let parquet = scope.spawn(|| timed(EncodeStep::Parquet, || encode_parquet(stream, &batch)));

        let archive = scope.spawn(|| {
            wants_tantivy
                .then(|| {
                    timed(EncodeStep::Index, || {
                        build_tantivy_bytes_for_batch(stream, &batch)
                    })
                })
                .and_then(Result::transpose)
        });

        let summary = timed(EncodeStep::Filters, || {
            summarize(stream, &batch, clustering.as_ref())
        });

        (joined(parquet), joined(archive), summary)
    });

    let parquet = parquet?;
    let parquet_checksum = checksum(&parquet);
    let schema_fingerprint = schema_fingerprint(batch.schema().as_ref());

    let index = archive.map(|built| {
        let artifact_id = ArtifactId::generate();
        let artifact_type = ArtifactTypeId::builtin(type_id::builtin::ARTIFACT_TANTIVY);

        let key = StorageLayout::artifact_key(
            &dataset.organization_id,
            stream.stream_type,
            &dataset.id,
            &partition,
            &segment_id,
            &artifact_id,
            &artifact_type,
        );

        match built {
            Ok(bytes) => IndexBuild::Ready {
                artifact_id,
                artifact_type,
                key,
                checksum: checksum(&bytes),
                bytes: Bytes::from(bytes),
            },
            Err(error) => IndexBuild::Failed {
                artifact_id,
                artifact_type,
                key,
                reason: truncate_failure(&error.to_string()),
            },
        }
    });

    column_stats.partitions = sharding.unwrap_or_default();
    column_stats.clustering = clustering.unwrap_or_default();

    Ok(EncodedPiece {
        partition,
        time_range: TimeRange::new(TimestampMicros(start_micros), TimestampMicros(end_micros)),
        segment_id,
        primary_id,
        primary_key,
        parquet,
        parquet_checksum,
        schema_fingerprint,
        index,
        filters,
        minute_counts,
        column_stats,
        row_count: batch.num_rows() as u64,
    })
}

/// The filters, the column statistics and the minute counts of a segment's sorted rows.
fn summarize(
    stream: &StreamDefinition,
    batch: &RecordBatch,
    clustering: Option<&Clustering>,
) -> (Option<SegmentFilters>, ColumnStats, Option<MinuteCounts>) {
    let filters = SegmentFilters::new(stream).map(|mut filters| {
        filters.add(batch);
        filters
    });

    let mut column_stats = ColumnStatsBuilder::new(stream);
    column_stats.add(stream, batch);

    let minute_counts = MinuteCountsBuilder::new(stream, clustering).and_then(|mut counts| {
        counts.add(batch);
        counts.finish()
    });

    (filters, column_stats.finish(), minute_counts)
}

/// What the thread of `handle` made. Its panic goes on here, and from here to the blocking task.
fn joined<T>(handle: ScopedJoinHandle<'_, T>) -> T {
    match handle.join() {
        Ok(made) => made,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::{
        array::{StringArray, TimestampMicrosecondArray},
        datatypes::{DataType, Field, Schema as ArrowSchema, TimeUnit},
    };
    use domain::{
        storage::{
            DatasetState, IndexPolicy, PartitionPolicy, PhysicalDatasetId, StoragePolicy,
            primary_dataset_type,
        },
        stream::{FieldDef, FieldType, Schema, StreamIndexType, StreamType},
    };
    use kernel::ids::Id;
    use parquet::file::metadata::ParquetMetaDataReader;
    use serde_json::json;

    use super::*;

    fn stream() -> StreamDefinition {
        let field = |name: &str, index_type: StreamIndexType| {
            let mut field = FieldDef {
                name: name.into(),
                data_type: FieldType::Utf8,
                nullable: true,
                index_type: StreamIndexType::None,
                encrypted: false,
            };

            field.index_type = index_type;
            field
        };

        StreamDefinition {
            id: Id::new(),
            org_id: Id::from_string("org"),
            name: "logs".into(),
            stream_type: StreamType::LOGS,
            schema: Schema {
                fields: vec![
                    field("level", StreamIndexType::Exact),
                    field("message", StreamIndexType::Ngram),
                ],
            },
            retention: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    /// 三段并行编出的东西各归其位：Parquet 有全部的行，索引归档建成，过滤器与列统计都在。
    #[test]
    fn a_piece_gets_its_file_index_filters_and_statistics() {
        let stream = stream();
        let rows = 2_000_i64;

        let dataset = PhysicalDataset {
            id: PhysicalDatasetId::generate(),
            organization_id: stream.org_id.clone(),
            logical_stream_id: stream.id.clone(),
            dataset_type: primary_dataset_type(StreamType::LOGS).unwrap(),
            dataset_type_version: 1,
            partition_policy: PartitionPolicy::default(),
            storage_policy: StoragePolicy::default(),
            index_policy: IndexPolicy {
                indexers: vec![IndexTypeId::builtin(type_id::builtin::INDEX_TANTIVY)],
            },
            catalog_version: 0,
            state: DatasetState::Active,
            created_at_micros: 0,
            updated_at_micros: 0,
        };

        let schema = ArrowSchema::new(vec![
            Field::new(
                "_timestamp",
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
            Field::new("level", DataType::Utf8, true),
            Field::new("message", DataType::Utf8, true),
        ]);

        let batch = RecordBatch::try_new(
            Arc::new(schema),
            vec![
                Arc::new(
                    TimestampMicrosecondArray::from((0..rows).collect::<Vec<_>>())
                        .with_timezone("UTC"),
                ),
                Arc::new(StringArray::from(vec!["INFO"; rows as usize])),
                Arc::new(StringArray::from(
                    (0..rows)
                        .map(|row| format!("request {row}"))
                        .collect::<Vec<_>>(),
                )),
            ],
        )
        .unwrap();

        let piece = Piece {
            batch,
            shard: 0,
            sharding: None,
            clustering: None,
        };

        let encoded = encode(&stream, &dataset, piece).unwrap();

        let metadata = ParquetMetaDataReader::new()
            .parse_and_finish(&encoded.parquet)
            .unwrap();

        assert_eq!(metadata.file_metadata().num_rows(), rows);
        assert_eq!(encoded.row_count, rows as u64);
        assert!(matches!(encoded.index, Some(IndexBuild::Ready { .. })));
        assert!(encoded.filters.is_some());
        assert_eq!(encoded.column_stats.null_counts["level"], json!(0));
    }
}
