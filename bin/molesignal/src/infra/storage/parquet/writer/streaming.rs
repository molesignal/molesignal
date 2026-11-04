// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A Segment written from a sorted stream of batches.
//!
//! A compaction merges files whose total size can reach many gigabytes. Holding the merged rows,
//! the encoded Parquet file or the Tantivy archive in memory would make a merge need memory in
//! proportion to its output. Here the Parquet file is encoded one row group at a time and
//! uploaded in parts while the batches arrive; the Tantivy archive is built on local disk and
//! uploaded from there.

use arrow::{array::RecordBatch, datatypes::SchemaRef};
use futures::{Stream, StreamExt};
use parquet::arrow::AsyncArrowWriter;

use super::{
    ParquetWriter, add_batch_docs,
    catalog::{pending_object, schema_fingerprint, truncate_failure},
    column_stats::ColumnStatsBuilder,
    filter_artifact::publish_filters,
    metadata::timestamp_range,
    upload::ArtifactUpload,
    upload_tantivy_archive, writer_properties,
};
use crate::{
    domain::{
        storage::{
            Artifact, ArtifactId, ArtifactRole, ArtifactState, ArtifactTypeId, DataSegment,
            IndexTypeId, Partition, PhysicalDataset, SegmentId, SegmentState, StoredObject,
            type_id,
        },
        stream::StreamDefinition,
    },
    infra::{
        search::tantivy_index::{self, TantivyArchiveBuilder},
        storage::{filters::SegmentFilters, layout::StorageLayout},
    },
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

/// A row group ends once its encoded columns reach this size, which bounds what the Parquet
/// writer buffers before it uploads.
const ROW_GROUP_BYTES: usize = 64 * 1024 * 1024;

impl ParquetWriter {
    /// Writes one Segment of `partition` from `batches`, which arrive in the storage order and
    /// aligned to `schema`. Like [`Self::write_compaction_catalog`], the Segment becomes visible
    /// only through `FileCatalog::replace_segments`; a write that fails leaves no object behind.
    pub async fn write_sorted_segment(
        &self,
        stream: &StreamDefinition,
        dataset: &PhysicalDataset,
        partition: Partition,
        schema: SchemaRef,
        batches: impl Stream<Item = Result<RecordBatch>> + Send,
    ) -> Result<DataSegment> {
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

        let properties = writer_properties(stream, &schema)
            .set_max_row_group_bytes(Some(ROW_GROUP_BYTES))
            .build();

        let upload = ArtifactUpload::new(self.object_store.clone(), primary_key);

        let mut parquet = AsyncArrowWriter::try_new(upload, schema.clone(), Some(properties))
            .map_err(|error| Error::internal(format!("AsyncArrowWriter::try_new: {error}")))?;

        let mut index = if wants_tantivy {
            IndexBuild::start(stream)
        } else {
            IndexBuild::Skipped
        };

        let mut stats = SegmentStats::new(stream);
        let mut filters = SegmentFilters::new(stream);

        let written = async {
            let mut batches = std::pin::pin!(batches);

            while let Some(batch) = batches.next().await {
                let batch = batch?;

                if batch.num_rows() == 0 {
                    continue;
                }

                stats.add(stream, &batch, &partition)?;
                index.add(stream, &batch);

                if let Some(filters) = filters.as_mut() {
                    filters.add(&batch);
                }

                parquet
                    .write(&batch)
                    .await
                    .map_err(|error| Error::internal(format!("parquet write: {error}")))?;
            }

            if stats.rows == 0 {
                return Err(Error::invalid("a Segment needs at least one row"));
            }

            parquet
                .finish()
                .await
                .map_err(|error| Error::internal(format!("parquet finish: {error}")))?;

            Ok(())
        }
        .await;

        let mut upload = parquet.into_inner();

        if let Err(error) = written {
            upload.abort().await;
            return Err(error);
        }

        let primary_object = upload.finish().await?;
        let fingerprint = schema_fingerprint(schema.as_ref());

        let primary = Artifact {
            id: primary_id.clone(),
            role: ArtifactRole::PrimaryData,
            artifact_type: parquet_type,
            format_version: 1,
            object: primary_object,
            source_artifact_id: None,
            source_checksum: None,
            schema_fingerprint: Some(fingerprint),
            state: ArtifactState::Ready,
            failure_reason: None,
        };

        let mut auxiliaries = index
            .publish(self, stream, dataset, &partition, &segment_id, &primary)
            .await
            .into_iter()
            .collect::<Vec<_>>();

        if let Some(filters) = filters {
            auxiliaries.extend(
                publish_filters(
                    self.object_store.as_ref(),
                    stream,
                    dataset,
                    &partition,
                    &segment_id,
                    &primary,
                    filters,
                )
                .await,
            );
        }

        Ok(DataSegment {
            id: segment_id,
            organization_id: dataset.organization_id.clone(),
            dataset_id: dataset.id.clone(),
            partition,
            time_range: TimeRange::new(TimestampMicros(stats.start), TimestampMicros(stats.end)),
            sequence_range: None,
            row_count: stats.rows,
            schema_fingerprint: Some(fingerprint),
            column_stats: stats.columns.finish(),
            flush_id: None,
            output_ordinal: 0,
            primary,
            auxiliaries,
            state: SegmentState::Active,
            visible_from_version: 0,
            retired_at_version: None,
            created_at_micros: TimestampMicros::now().0,
        })
    }
}

/// Row count, time bounds and column statistics of the batches written so far.
struct SegmentStats {
    rows: u64,
    start: i64,
    end: i64,
    columns: ColumnStatsBuilder,
}

impl SegmentStats {
    fn new(stream: &StreamDefinition) -> Self {
        Self {
            rows: 0,
            start: i64::MAX,
            end: i64::MIN,
            columns: ColumnStatsBuilder::new(stream),
        }
    }

    fn add(
        &mut self,
        stream: &StreamDefinition,
        batch: &RecordBatch,
        partition: &Partition,
    ) -> Result<()> {
        let (start, end) = timestamp_range(batch)?;

        if start < partition.start_micros || end >= partition.end_micros {
            return Err(Error::internal(
                "a row lies outside the partition its Segment is written to",
            ));
        }

        self.rows += batch.num_rows() as u64;
        self.start = self.start.min(start);
        self.end = self.end.max(end);
        self.columns.add(stream, batch);
        Ok(())
    }
}

/// The Tantivy archive of the Segment while its rows arrive. Indexing failures do not fail the
/// Segment: its index Artifact is recorded as failed, as the batch writer records it.
enum IndexBuild {
    Skipped,
    /// The stream has no indexed text column yet; the first batch decides.
    Waiting(TantivyArchiveBuilder),
    Building(TantivyArchiveBuilder),
    Failed(String),
}

impl IndexBuild {
    fn start(stream: &StreamDefinition) -> Self {
        match TantivyArchiveBuilder::try_new(stream) {
            Ok(Some(builder)) => Self::Waiting(builder),
            Ok(None) => Self::Skipped,
            Err(error) => Self::Failed(truncate_failure(&format!("tantivy builder: {error}"))),
        }
    }

    fn add(&mut self, stream: &StreamDefinition, batch: &RecordBatch) {
        let state = std::mem::replace(self, Self::Skipped);

        *self = match state {
            Self::Waiting(mut builder) | Self::Building(mut builder) => {
                match add_batch_docs(&mut builder, stream, batch) {
                    Ok(true) => Self::Building(builder),
                    Ok(false) => Self::Skipped,
                    Err(error) => Self::Failed(truncate_failure(&error.to_string())),
                }
            }
            other => other,
        };
    }

    /// Uploads the archive and returns its Artifact, if the Segment has an index.
    async fn publish(
        self,
        writer: &ParquetWriter,
        stream: &StreamDefinition,
        dataset: &PhysicalDataset,
        partition: &Partition,
        segment_id: &SegmentId,
        primary: &Artifact,
    ) -> Option<Artifact> {
        let artifact_id = ArtifactId::generate();
        let artifact_type = ArtifactTypeId::builtin(type_id::builtin::ARTIFACT_TANTIVY);

        let key = StorageLayout::artifact_key(
            &dataset.organization_id,
            stream.stream_type,
            &dataset.id,
            partition,
            segment_id,
            &artifact_id,
            &artifact_type,
        );

        let outcome: std::result::Result<StoredObject, String> = match self {
            Self::Skipped | Self::Waiting(_) => return None,
            Self::Failed(reason) => Err(reason),
            Self::Building(builder) => {
                upload_tantivy_archive(writer.object_store.clone(), builder, key.clone())
                    .await
                    .map_err(|error| truncate_failure(&error.to_string()))
            }
        };

        let (object, state, failure_reason) = match outcome {
            Ok(object) => (object, ArtifactState::Ready, None),
            Err(reason) => (pending_object(key), ArtifactState::Failed, Some(reason)),
        };

        Some(Artifact {
            id: artifact_id,
            role: ArtifactRole::Index,
            artifact_type,
            format_version: tantivy_index::FORMAT_VERSION,
            object,
            source_artifact_id: Some(primary.id.clone()),
            source_checksum: Some(primary.object.checksum.clone()),
            schema_fingerprint: primary.schema_fingerprint,
            state,
            failure_reason,
        })
    }
}
