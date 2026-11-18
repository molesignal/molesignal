// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A Segment written from a sorted stream of batches.
//!
//! A compaction merges files whose total size can reach many gigabytes. Holding the merged rows,
//! the encoded Parquet file or the Tantivy archive in memory would make a merge need memory in
//! proportion to its output. Here the Parquet file is encoded one row group at a time and
//! uploaded in parts while the batches arrive; the Tantivy archive is built on local disk and
//! uploaded from there.

use arrow::{array::RecordBatch, compute::concat_batches, datatypes::SchemaRef};
use domain::{
    storage::{
        Artifact, ArtifactId, ArtifactRole, ArtifactState, ArtifactTypeId, DataSegment,
        IndexTypeId, Partition, PhysicalDataset, SegmentId, SegmentState, StoredObject, type_id,
    },
    stream::{Clustering, StreamDefinition},
};
use futures::{Stream, StreamExt};
use kernel::{
    Error, Result,
    time::{TimeRange, TimestampMicros},
};
use parquet::{arrow::AsyncArrowWriter, errors::ParquetError};

use super::{
    ParquetWriter, add_batch_docs,
    catalog::{pending_object, schema_fingerprint, truncate_failure},
    column_stats::ColumnStatsBuilder,
    index_objects::SegmentObjects,
    metadata::timestamp_range,
    ngram_columns,
    upload::ArtifactUpload,
    upload_tantivy_archive, writer_properties,
};
use crate::infra::{
    search::tantivy_index::{self, TantivyArchiveBuilder},
    storage::{
        filters::{BLOCK_ROWS, SegmentFilters},
        layout::StorageLayout,
        minute_counts::MinuteCountsBuilder,
        parquet::stored_text::{stored_batch, stored_schema},
    },
};

/// A row group ends once its encoded columns reach this size, which bounds what the Parquet
/// writer buffers before it uploads.
const ROW_GROUP_BYTES: usize = 64 * 1024 * 1024;

impl ParquetWriter {
    /// Writes one Segment of `partition` from `batches`, which arrive in the storage order and
    /// aligned to `schema`, clustered by `clustering` when it is given. Like
    /// [`Self::write_compaction_catalog`], the Segment becomes visible only through
    /// `FileCatalog::replace_segments`; a write that fails leaves no object behind.
    pub async fn write_sorted_segment(
        &self,
        stream: &StreamDefinition,
        dataset: &PhysicalDataset,
        partition: Partition,
        schema: SchemaRef,
        clustering: Option<&Clustering>,
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

        // A stream with block filters is written a whole block at a time, and its row groups end
        // here after a block: the writer would end one wherever its bytes ran out, and the pages
        // of the next would then start inside blocks.
        let mut whole_blocks =
            (!ngram_columns(stream, &schema).is_empty()).then(|| WholeBlocks::new(schema.clone()));

        let properties = writer_properties(stream, &schema)
            .set_max_row_group_bytes(whole_blocks.is_none().then_some(ROW_GROUP_BYTES))
            .build();

        let upload = ArtifactUpload::new(self.object_store.clone(), primary_key);

        // Text columns are stored as byte arrays (see `stored_text`); every batch is converted on
        // its way into the writer.
        let mut parquet =
            AsyncArrowWriter::try_new(upload, stored_schema(&schema), Some(properties))
                .map_err(|error| Error::internal(format!("AsyncArrowWriter::try_new: {error}")))?;

        let mut index = if wants_tantivy {
            IndexBuild::start(stream)
        } else {
            IndexBuild::Skipped
        };

        let mut stats = SegmentStats::new(stream);
        let mut filters = SegmentFilters::new(stream);
        let mut minute_counts = MinuteCountsBuilder::new(stream, clustering);

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

                if let Some(minute_counts) = minute_counts.as_mut() {
                    minute_counts.add(&batch);
                }

                let Some(blocks) = whole_blocks.as_mut() else {
                    parquet
                        .write(&stored_batch(&batch)?)
                        .await
                        .map_err(write_error)?;
                    continue;
                };

                for whole in blocks.push(&batch)? {
                    parquet
                        .write(&stored_batch(&whole)?)
                        .await
                        .map_err(write_error)?;

                    if parquet.in_progress_size() >= ROW_GROUP_BYTES {
                        parquet.flush().await.map_err(write_error)?;
                    }
                }
            }

            if stats.rows == 0 {
                return Err(Error::invalid("a Segment needs at least one row"));
            }

            if let Some(rest) = whole_blocks
                .take()
                .map(WholeBlocks::finish)
                .transpose()?
                .flatten()
            {
                parquet
                    .write(&stored_batch(&rest)?)
                    .await
                    .map_err(write_error)?;
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

        let objects = SegmentObjects {
            store: self.object_store.as_ref(),
            stream,
            dataset,
            partition: &partition,
            segment_id: &segment_id,
            primary: &primary,
        };

        if let Some(filters) = filters {
            auxiliaries.extend(objects.publish_filters(filters).await);
        }

        if let Some(counts) = minute_counts.and_then(MinuteCountsBuilder::finish) {
            auxiliaries.extend(objects.publish_minute_counts(counts).await);
        }

        let mut column_stats = stats.columns.finish();
        column_stats.clustering = clustering.cloned().unwrap_or_default();

        Ok(DataSegment {
            id: segment_id,
            organization_id: dataset.organization_id.clone(),
            dataset_id: dataset.id.clone(),
            partition,
            time_range: TimeRange::new(TimestampMicros(stats.start), TimestampMicros(stats.end)),
            sequence_range: None,
            row_count: stats.rows,
            schema_fingerprint: Some(fingerprint),
            column_stats,
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

fn write_error(error: ParquetError) -> Error {
    Error::internal(format!("parquet write: {error}"))
}

/// Rows handed on a whole block of [`BLOCK_ROWS`] at a time: the Parquet writer ends a page every
/// [`BLOCK_ROWS`] rows of a write, so writes of whole blocks keep every page on one block.
struct WholeBlocks {
    schema: SchemaRef,
    /// The rows of a block not yet finished.
    pending: Vec<RecordBatch>,
    rows: usize,
}

impl WholeBlocks {
    fn new(schema: SchemaRef) -> Self {
        Self {
            schema,
            pending: Vec::new(),
            rows: 0,
        }
    }

    /// The rows held before and those of `batch`, in batches of whole blocks; the rows of a block
    /// they leave unfinished are held.
    fn push(&mut self, batch: &RecordBatch) -> Result<Vec<RecordBatch>> {
        let mut whole = Vec::new();
        let mut rest = batch.clone();

        if self.rows > 0 {
            let missing = (BLOCK_ROWS - self.rows).min(rest.num_rows());

            self.pending.push(rest.slice(0, missing));
            self.rows += missing;
            rest = rest.slice(missing, rest.num_rows() - missing);

            if self.rows < BLOCK_ROWS {
                return Ok(whole);
            }

            whole.push(self.take()?);
        }

        let blocks = rest.num_rows() / BLOCK_ROWS * BLOCK_ROWS;

        if blocks > 0 {
            whole.push(rest.slice(0, blocks));
        }

        if blocks < rest.num_rows() {
            self.rows = rest.num_rows() - blocks;
            self.pending.push(rest.slice(blocks, self.rows));
        }

        Ok(whole)
    }

    /// The rows of the last block, which may be short.
    fn finish(mut self) -> Result<Option<RecordBatch>> {
        if self.rows == 0 {
            return Ok(None);
        }

        self.take().map(Some)
    }

    fn take(&mut self) -> Result<RecordBatch> {
        self.rows = 0;

        concat_batches(&self.schema, &std::mem::take(&mut self.pending))
            .map_err(|error| Error::internal(format!("join rows into a block: {error}")))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::{
        array::{Array, Int64Array},
        datatypes::{DataType, Field, Schema},
    };

    use super::*;

    #[test]
    fn rows_are_handed_on_in_whole_blocks_and_in_order() {
        let schema = Arc::new(Schema::new(vec![Field::new("row", DataType::Int64, false)]));
        let mut blocks = WholeBlocks::new(schema.clone());
        let mut next = 0_i64;
        let mut written = Vec::new();

        for rows in [10, BLOCK_ROWS * 2 + 5, 7, BLOCK_ROWS - 22, 3_000, 1] {
            let values = Int64Array::from((next..next + rows as i64).collect::<Vec<_>>());
            let batch = RecordBatch::try_new(schema.clone(), vec![Arc::new(values)]).unwrap();

            next += rows as i64;

            for whole in blocks.push(&batch).unwrap() {
                assert_eq!(whole.num_rows() % BLOCK_ROWS, 0);
                written.push(whole);
            }
        }

        let last = blocks.finish().unwrap().expect("a short last block");

        assert!(last.num_rows() < BLOCK_ROWS);
        written.push(last);

        let rows = written
            .iter()
            .flat_map(|batch| {
                let values = batch
                    .column(0)
                    .as_any()
                    .downcast_ref::<Int64Array>()
                    .unwrap();
                values.values().to_vec()
            })
            .collect::<Vec<_>>();

        assert_eq!(rows, (0..next).collect::<Vec<_>>());
    }

    #[test]
    fn whole_blocks_leave_nothing_behind() {
        let schema = Arc::new(Schema::new(vec![Field::new("row", DataType::Int64, false)]));
        let mut blocks = WholeBlocks::new(schema.clone());
        let values = Int64Array::from(vec![0; BLOCK_ROWS * 3]);
        let batch = RecordBatch::try_new(schema, vec![Arc::new(values)]).unwrap();

        assert_eq!(blocks.push(&batch).unwrap().len(), 1);
        assert!(blocks.finish().unwrap().is_none());
    }
}
