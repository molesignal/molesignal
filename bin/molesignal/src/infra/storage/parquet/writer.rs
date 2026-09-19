// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Arrow batch encoding and Catalog-native immutable Artifact publication.
//!
//! Object keys are supplied exclusively by [`crate::infra::storage::layout::StorageLayout`]
//! through the `catalog` implementation. This module deliberately has no API that accepts a
//! logical stream name and invents a physical path from it.

use std::{collections::HashMap, sync::Arc};

use arrow::array::{Array, RecordBatch, StringArray};
use bytes::Bytes;
use object_store::ObjectStore;
use parquet::{
    arrow::ArrowWriter, basic::Compression, file::properties::WriterProperties,
    schema::types::ColumnPath,
};

use crate::{
    domain::stream::{StreamDefinition, StreamIndexType},
    infra::search::tantivy_index::TantivyArchiveBuilder,
    shared::{Error, Result},
};

mod catalog;
mod metadata;

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
    let schema = batch.schema();
    let mut indexed_arrays: Vec<(String, &StringArray)> = Vec::new();
    for field in &stream.schema.fields {
        if !matches!(
            field.effective_index_type(),
            StreamIndexType::Exact | StreamIndexType::FullText
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
        return Ok(None);
    }
    for row in 0..batch.num_rows() {
        let mut values: HashMap<&str, &str> = HashMap::new();
        for (name, array) in &indexed_arrays {
            if !array.is_null(row) {
                values.insert(name.as_str(), array.value(row));
            }
        }
        builder
            .add_doc(&values)
            .map_err(|error| Error::internal(format!("tantivy add_doc: {error}")))?;
    }
    let bytes = builder
        .commit_and_archive()
        .map_err(|error| Error::internal(format!("tantivy commit_and_archive: {error}")))?;
    Ok(Some(bytes))
}

pub(super) fn encode_parquet(stream: &StreamDefinition, batch: &RecordBatch) -> Result<Bytes> {
    let mut properties = WriterProperties::builder().set_compression(Compression::SNAPPY);
    for field in stream.schema.fields.iter().filter(|field| {
        matches!(
            field.effective_index_type(),
            StreamIndexType::Exact | StreamIndexType::Bloom
        ) && !field.encrypted
    }) {
        if batch.schema().index_of(&field.name).is_ok() {
            properties = properties
                .set_column_bloom_filter_enabled(ColumnPath::from(field.name.clone()), true)
                .set_column_bloom_filter_fpp(ColumnPath::from(field.name.clone()), 0.01);
        }
    }
    let properties = properties.build();
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
