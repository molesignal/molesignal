// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Arrow batch encoding and Catalog-native immutable Artifact publication.
//!
//! Object keys are supplied exclusively by [`crate::infra::storage::layout::StorageLayout`]
//! through the `catalog` implementation. This module deliberately has no API that accepts a
//! logical stream name and invents a physical path from it.

use std::{collections::HashMap, sync::Arc};

use arrow::{
    array::{Array, RecordBatch, StringArray, TimestampMicrosecondArray},
    datatypes::Schema as ArrowSchema,
};
use bytes::Bytes;
use domain::stream::{FieldDef, FieldType, StreamDefinition, StreamIndexType};
use kernel::{Error, Result};
use object_store::ObjectStore;
use parquet::{
    arrow::ArrowWriter,
    basic::{Compression, ZstdLevel},
    file::properties::{
        DEFAULT_DATA_PAGE_ROW_COUNT_LIMIT, WriterProperties, WriterPropertiesBuilder,
    },
    schema::types::ColumnPath,
};
use query_language::Number;

use crate::infra::{
    search::tantivy_index::{NumberColumns, TantivyArchiveBuilder},
    storage::{arrow_schema::TS_COL, filters::BLOCK_ROWS},
};

mod catalog;
pub(crate) mod column_stats;
mod index_objects;
mod metadata;
mod streaming;
pub(crate) mod upload;

/// zstd level of every Parquet file written: about as fast to write as snappy, a smaller file
/// to read from object storage, and decoding speed hardly depends on the level.
const ZSTD_LEVEL: i32 = 1;

/// Most bytes of a page of an `ngram` column. Its pages end after [`BLOCK_ROWS`] rows; this only
/// keeps the writer from splitting a block of long values into pages that cross blocks.
const NGRAM_PAGE_BYTES: usize = 64 * 1024 * 1024;

/// The codec of every Parquet file written: data files and partition manifests.
pub(crate) fn compression() -> Compression {
    Compression::ZSTD(ZstdLevel::try_new(ZSTD_LEVEL).expect("a valid zstd level"))
}

pub struct ParquetWriter {
    pub(super) object_store: Arc<dyn ObjectStore>,
}

impl ParquetWriter {
    pub fn new(object_store: Arc<dyn ObjectStore>) -> Self {
        Self { object_store }
    }
}

pub(crate) fn build_tantivy_bytes_for_batch(
    stream: &StreamDefinition,
    batch: &RecordBatch,
) -> Result<Option<Vec<u8>>> {
    let mut builder = match TantivyArchiveBuilder::try_new(stream)
        .map_err(|error| Error::internal(format!("tantivy builder: {error}")))?
    {
        Some(builder) => builder,
        None => return Ok(None),
    };

    if !add_batch_docs(&mut builder, stream, batch)? {
        return Ok(None);
    }

    let bytes = builder
        .commit_and_archive()
        .map_err(|error| Error::internal(format!("tantivy commit_and_archive: {error}")))?;

    Ok(Some(bytes))
}

/// Adds one Tantivy document per row of `batch`: the row's indexed text values, its numbers of
/// the `range` fields and its `_timestamp`. `false` when the batch has none of the indexed
/// columns, so there is nothing to index.
pub(crate) fn add_batch_docs(
    builder: &mut TantivyArchiveBuilder,
    stream: &StreamDefinition,
    batch: &RecordBatch,
) -> Result<bool> {
    let schema = batch.schema();
    let mut indexed_arrays: Vec<(String, &StringArray)> = Vec::new();

    for field in &stream.schema.fields {
        if !matches!(
            field.index_type,
            StreamIndexType::Exact | StreamIndexType::FullText | StreamIndexType::KeyValue
        ) {
            continue;
        }

        if let Ok(index) = schema.index_of(&field.name)
            && let Some(array) = batch.column(index).as_any().downcast_ref::<StringArray>()
        {
            indexed_arrays.push((field.name.clone(), array));
        }
    }

    let numbers = NumberColumns::of(stream, batch);

    if indexed_arrays.is_empty() && numbers.is_empty() {
        return Ok(false);
    }

    let timestamps = schema.index_of(TS_COL).ok().and_then(|index| {
        batch
            .column(index)
            .as_any()
            .downcast_ref::<TimestampMicrosecondArray>()
    });

    // One map of each kind for every row: cleared, not reallocated.
    let mut values: HashMap<&str, &str> = HashMap::with_capacity(indexed_arrays.len());
    let mut row_numbers: HashMap<&str, Number> = HashMap::new();

    for row in 0..batch.num_rows() {
        values.clear();

        for (name, array) in &indexed_arrays {
            if !array.is_null(row) {
                values.insert(name.as_str(), array.value(row));
            }
        }

        numbers.read(row, &mut row_numbers);

        let timestamp = timestamps
            .filter(|timestamps| !timestamps.is_null(row))
            .map(|timestamps| timestamps.value(row));

        builder
            .add_doc_with_numbers(timestamp, &values, &row_numbers)
            .map_err(|error| Error::internal(format!("tantivy add_doc: {error}")))?;
    }

    Ok(true)
}

/// Commits a Tantivy archive into a local file off the async workers, then uploads the file
/// to `key` in parts. Neither the archive nor its upload is held in memory.
pub(crate) async fn upload_tantivy_archive(
    store: Arc<dyn ObjectStore>,
    builder: TantivyArchiveBuilder,
    key: domain::storage::ObjectKey,
) -> Result<domain::storage::StoredObject> {
    let file = tempfile::Builder::new()
        .prefix("molesignal-index-")
        .tempfile()
        .map_err(|error| Error::internal(format!("index archive file: {error}")))?;

    let path = file.path().to_path_buf();

    tokio::task::spawn_blocking(move || builder.commit_and_archive_to(&path))
        .await
        .map_err(|error| Error::internal(format!("index archive join: {error}")))?
        .map_err(|error| Error::internal(format!("tantivy commit_and_archive: {error}")))?;

    upload::ArtifactUpload::file(store, key, file.path()).await
}

/// Writer properties every Parquet file of `stream` is encoded with.
///
/// The columns of `ngram` fields get block filters, one for every [`BLOCK_ROWS`] rows (see
/// [`filters`](crate::infra::storage::filters)). A file with such a column has pages of
/// [`BLOCK_ROWS`] rows in every column, so a scan skips each block the filters rule out without
/// decompressing it, as long as the file is written a whole block at a time.
pub(super) fn writer_properties(
    stream: &StreamDefinition,
    schema: &ArrowSchema,
) -> WriterPropertiesBuilder {
    let mut properties = WriterProperties::builder().set_compression(compression());

    for field in stream
        .schema
        .fields
        .iter()
        .filter(|field| parquet_bloom(field))
    {
        if schema.index_of(&field.name).is_ok() {
            properties = properties
                .set_column_bloom_filter_enabled(ColumnPath::from(field.name.clone()), true)
                .set_column_bloom_filter_fpp(ColumnPath::from(field.name.clone()), 0.01);
        }
    }

    let ngram = ngram_columns(stream, schema);

    if !ngram.is_empty() {
        properties = properties
            .set_write_batch_size(BLOCK_ROWS)
            .set_data_page_row_count_limit(BLOCK_ROWS);
    }

    for column in ngram {
        properties =
            properties.set_column_data_page_size_limit(ColumnPath::from(column), NGRAM_PAGE_BYTES);
    }

    properties
}

/// Whether the column of `field` gets a Parquet Bloom filter: a `bloom` field's, and an `exact`
/// field's that is not text. The index archive holds the values of `exact` text fields (see
/// [`add_batch_docs`]): it skips the files without a value and reads only the pages of its rows,
/// so no query would read their Bloom filters, which cost about 5 bytes a row for an identifier.
fn parquet_bloom(field: &FieldDef) -> bool {
    !field.encrypted
        && match field.index_type {
            StreamIndexType::Bloom => true,
            StreamIndexType::Exact => field.data_type != FieldType::Utf8,
            _ => false,
        }
}

/// Most rows a page of a file of `stream` holds, as [`writer_properties`] sets them: [`BLOCK_ROWS`]
/// when it has `ngram` columns, the Parquet default otherwise. A page may hold fewer, where its
/// bytes run out first.
pub(crate) fn page_rows(stream: &StreamDefinition, schema: &ArrowSchema) -> usize {
    match ngram_columns(stream, schema).is_empty() {
        true => DEFAULT_DATA_PAGE_ROW_COUNT_LIMIT,
        false => BLOCK_ROWS,
    }
}

/// The columns of `schema` that hold `ngram` fields of `stream`: they get block filters.
pub(super) fn ngram_columns(stream: &StreamDefinition, schema: &ArrowSchema) -> Vec<String> {
    stream
        .schema
        .fields
        .iter()
        .filter(|field| {
            field.data_type == FieldType::Utf8
                && field.index_type == StreamIndexType::Ngram
                && !field.encrypted
                && schema.index_of(&field.name).is_ok()
        })
        .map(|field| field.name.clone())
        .collect()
}

pub(super) fn encode_parquet(stream: &StreamDefinition, batch: &RecordBatch) -> Result<Bytes> {
    let properties = writer_properties(stream, batch.schema().as_ref()).build();
    let stored = super::stored_text::stored_batch(batch)?;
    let mut buffer = Vec::with_capacity(64 * 1024);

    {
        let mut writer = ArrowWriter::try_new(&mut buffer, stored.schema(), Some(properties))
            .map_err(|error| Error::internal(format!("ArrowWriter::try_new: {error}")))?;

        writer
            .write(&stored)
            .map_err(|error| Error::internal(format!("ArrowWriter::write: {error}")))?;

        writer
            .close()
            .map_err(|error| Error::internal(format!("ArrowWriter::close: {error}")))?;
    }

    Ok(Bytes::from(buffer))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::{
        array::{Int64Array, StringArray, TimestampMicrosecondArray},
        record_batch::RecordBatch,
    };
    use domain::stream::{FieldDef, FieldType, Schema, StreamIndexType, StreamType};
    use kernel::{ids::Id, time::TimestampMicros};

    use super::*;
    use crate::infra::storage::arrow_schema::to_arrow;

    fn sample_stream() -> StreamDefinition {
        StreamDefinition {
            id: Id::new(),
            org_id: Id::from_string("org-1"),
            name: "app".into(),
            stream_type: StreamType::LOGS,
            schema: Schema {
                fields: vec![
                    FieldDef {
                        name: "level".into(),
                        data_type: FieldType::Utf8,
                        nullable: false,
                        index_type: StreamIndexType::FullText,
                        encrypted: false,
                    },
                    FieldDef {
                        name: "latency_ms".into(),
                        data_type: FieldType::Int64,
                        nullable: true,
                        index_type: StreamIndexType::Skip,
                        encrypted: false,
                    },
                ],
            },
            retention: None,
            created_at: TimestampMicros::now(),
            updated_at: TimestampMicros::now(),
        }
    }

    fn sample_batch(stream: &StreamDefinition) -> RecordBatch {
        RecordBatch::try_new(
            to_arrow(&stream.schema),
            vec![
                Arc::new(
                    TimestampMicrosecondArray::from(vec![1_000_000, 2_000_000, 3_000_000])
                        .with_timezone("UTC"),
                ),
                Arc::new(StringArray::from(vec!["info", "warn", "error"])),
                Arc::new(Int64Array::from(vec![Some(10), Some(20), None])),
            ],
        )
        .unwrap()
    }

    /// A `bloom` field gets a Parquet Bloom filter, and so does an `exact` field that is not
    /// text; an `exact` text field is found by the index archive instead, and `skip` gets none.
    #[test]
    fn a_parquet_bloom_is_written_for_bloom_fields_and_exact_fields_that_are_not_text() {
        use parquet::file::metadata::ParquetMetaDataReader;

        for (text, number, blooms) in [
            (StreamIndexType::Bloom, StreamIndexType::Skip, [true, false]),
            (
                StreamIndexType::Exact,
                StreamIndexType::Skip,
                [false, false],
            ),
            (
                StreamIndexType::Exact,
                StreamIndexType::Exact,
                [false, true],
            ),
            (StreamIndexType::Skip, StreamIndexType::Bloom, [false, true]),
        ] {
            let mut stream = sample_stream();
            stream.schema.fields[0].index_type = text;
            stream.schema.fields[1].index_type = number;
            let bytes = encode_parquet(&stream, &sample_batch(&stream)).expect("encode parquet");
            let metadata = ParquetMetaDataReader::new()
                .parse_and_finish(&bytes)
                .expect("read parquet metadata");
            let row_group = metadata.row_group(0);

            let written =
                [1, 2].map(|column| row_group.column(column).bloom_filter_offset().is_some());

            assert_eq!(written, blooms, "{text:?} text, {number:?} number");
        }
    }

    /// A stream with an `ngram` field writes a page for every block; any other the Parquet default.
    #[test]
    fn a_page_holds_a_block_of_rows_where_the_stream_has_an_ngram_field() {
        let mut stream = sample_stream();
        let schema = to_arrow(&stream.schema);

        assert_eq!(
            page_rows(&stream, &schema),
            DEFAULT_DATA_PAGE_ROW_COUNT_LIMIT
        );

        stream.schema.fields[0].index_type = StreamIndexType::Ngram;

        assert_eq!(page_rows(&stream, &schema), BLOCK_ROWS);
    }

    /// The pages of every column of a stream with an `ngram` field start on blocks, the `ngram`
    /// column's too once its dictionary is full, however long its values.
    #[test]
    fn an_ngram_stream_writes_a_page_for_each_block() {
        use parquet::{
            arrow::arrow_reader::{ArrowReaderMetadata, ArrowReaderOptions},
            file::metadata::PageIndexPolicy,
        };

        let rows = BLOCK_ROWS * 4 + 100;

        let batch = |stream: &StreamDefinition| {
            let texts = (0..rows).map(|row| format!("{row:06} {}", "abcdefghij".repeat(120)));

            RecordBatch::try_new(
                to_arrow(&stream.schema),
                vec![
                    Arc::new(
                        TimestampMicrosecondArray::from((0..rows as i64).collect::<Vec<_>>())
                            .with_timezone("UTC"),
                    ),
                    Arc::new(StringArray::from(texts.collect::<Vec<_>>())),
                    Arc::new(Int64Array::from((0..rows as i64).collect::<Vec<_>>())),
                ],
            )
            .unwrap()
        };

        let pages = |stream: &StreamDefinition| {
            let bytes = encode_parquet(stream, &batch(stream)).expect("encode parquet");
            let options =
                ArrowReaderOptions::new().with_page_index_policy(PageIndexPolicy::Optional);
            let metadata = ArrowReaderMetadata::load(&bytes, options).expect("read metadata");

            metadata.metadata().offset_index().expect("an offset index")[0]
                .iter()
                .map(|column| {
                    column
                        .page_locations()
                        .iter()
                        .map(|page| page.first_row_index as usize)
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        };

        let mut stream = sample_stream();
        stream.schema.fields[0].index_type = StreamIndexType::Ngram;

        let starts = pages(&stream);
        let blocks = (0..rows.div_ceil(BLOCK_ROWS))
            .map(|block| block * BLOCK_ROWS)
            .collect::<Vec<_>>();

        assert_eq!((&starts[0], &starts[2]), (&blocks, &blocks));

        // The text column's dictionary fills up inside the first block: the page it ends there is
        // followed by one that runs to the end of the next block, and every later page starts on a
        // block.
        let unaligned = starts[1].iter().filter(|start| *start % BLOCK_ROWS != 0);

        assert!(unaligned.count() <= 1, "{starts:?}");
        assert!(
            blocks[2..].iter().all(|block| starts[1].contains(block)),
            "{starts:?}"
        );

        // Without an `ngram` field, pages keep their default size: the text pages end where their
        // bytes run out, and the numbers fit in one page.
        let starts = pages(&sample_stream());

        assert_ne!(starts[1], blocks);
        assert_eq!((starts[0].clone(), starts[2].clone()), (vec![0], vec![0]));
    }
}
