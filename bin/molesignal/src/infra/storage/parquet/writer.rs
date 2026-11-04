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
use object_store::ObjectStore;
use parquet::{
    arrow::ArrowWriter,
    basic::{Compression, ZstdLevel},
    file::properties::{WriterProperties, WriterPropertiesBuilder},
    schema::types::ColumnPath,
};

use crate::{
    domain::stream::{StreamDefinition, StreamIndexType},
    infra::{search::tantivy_index::TantivyArchiveBuilder, storage::arrow_schema::TS_COL},
    shared::{Error, Result},
};

mod catalog;
pub(crate) mod column_stats;
mod filter_artifact;
mod metadata;
mod streaming;
pub(crate) mod upload;

/// zstd level of every Parquet file written: about as fast to write as snappy, a smaller file
/// to read from object storage, and decoding speed hardly depends on the level.
const ZSTD_LEVEL: i32 = 1;

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

/// Adds one Tantivy document per row of `batch`: the row's indexed text values and its
/// `_timestamp`. `false` when the batch has none of the indexed columns, so there is nothing to
/// index.
pub(crate) fn add_batch_docs(
    builder: &mut TantivyArchiveBuilder,
    stream: &StreamDefinition,
    batch: &RecordBatch,
) -> Result<bool> {
    let schema = batch.schema();
    let mut indexed_arrays: Vec<(String, &StringArray)> = Vec::new();

    for field in &stream.schema.fields {
        if !matches!(
            field.effective_index_type(),
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

    if indexed_arrays.is_empty() {
        return Ok(false);
    }

    let timestamps = schema.index_of(TS_COL).ok().and_then(|index| {
        batch
            .column(index)
            .as_any()
            .downcast_ref::<TimestampMicrosecondArray>()
    });

    for row in 0..batch.num_rows() {
        let mut values: HashMap<&str, &str> = HashMap::new();

        for (name, array) in &indexed_arrays {
            if !array.is_null(row) {
                values.insert(name.as_str(), array.value(row));
            }
        }

        let timestamp = timestamps
            .filter(|timestamps| !timestamps.is_null(row))
            .map(|timestamps| timestamps.value(row));

        builder
            .add_doc(timestamp, &values)
            .map_err(|error| Error::internal(format!("tantivy add_doc: {error}")))?;
    }

    Ok(true)
}

/// Commits a Tantivy archive into a local file off the async workers, then uploads the file
/// to `key` in parts. Neither the archive nor its upload is held in memory.
pub(crate) async fn upload_tantivy_archive(
    store: Arc<dyn ObjectStore>,
    builder: TantivyArchiveBuilder,
    key: crate::domain::storage::ObjectKey,
) -> Result<crate::domain::storage::StoredObject> {
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
pub(super) fn writer_properties(
    stream: &StreamDefinition,
    schema: &ArrowSchema,
) -> WriterPropertiesBuilder {
    let mut properties = WriterProperties::builder().set_compression(compression());

    for field in stream.schema.fields.iter().filter(|field| {
        matches!(
            field.effective_index_type(),
            StreamIndexType::Exact | StreamIndexType::Bloom
        ) && !field.encrypted
    }) {
        if schema.index_of(&field.name).is_ok() {
            properties = properties
                .set_column_bloom_filter_enabled(ColumnPath::from(field.name.clone()), true)
                .set_column_bloom_filter_fpp(ColumnPath::from(field.name.clone()), 0.01);
        }
    }

    properties
}

pub(super) fn encode_parquet(stream: &StreamDefinition, batch: &RecordBatch) -> Result<Bytes> {
    let properties = writer_properties(stream, batch.schema().as_ref()).build();
    let mut buffer = Vec::with_capacity(64 * 1024);

    {
        let mut writer = ArrowWriter::try_new(&mut buffer, batch.schema(), Some(properties))
            .map_err(|error| Error::internal(format!("ArrowWriter::try_new: {error}")))?;

        writer
            .write(batch)
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

    use super::*;
    use crate::{
        domain::stream::{FieldDef, FieldType, Schema, StreamIndexType, StreamType},
        infra::storage::arrow_schema::to_arrow,
        shared::{ids::Id, time::TimestampMicros},
    };

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
                        index_type: Some(StreamIndexType::FullText),
                        indexed: true,
                        encrypted: false,
                        exact: false,
                    },
                    FieldDef {
                        name: "latency_ms".into(),
                        data_type: FieldType::Int64,
                        nullable: true,
                        index_type: Some(StreamIndexType::Skip),
                        indexed: true,
                        encrypted: false,
                        exact: false,
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

    #[test]
    fn bloom_type_writes_a_parquet_bloom_without_falling_through_from_skip() {
        use parquet::file::metadata::ParquetMetaDataReader;

        for index_type in [StreamIndexType::Bloom, StreamIndexType::Exact] {
            let mut stream = sample_stream();
            stream.schema.fields[0].configure_index(true, index_type);
            let bytes = encode_parquet(&stream, &sample_batch(&stream)).expect("encode parquet");
            let metadata = ParquetMetaDataReader::new()
                .parse_and_finish(&bytes)
                .expect("read parquet metadata");
            let row_group = metadata.row_group(0);
            assert!(row_group.column(1).bloom_filter_offset().is_some());
            assert!(row_group.column(2).bloom_filter_offset().is_none());
        }
    }
}
