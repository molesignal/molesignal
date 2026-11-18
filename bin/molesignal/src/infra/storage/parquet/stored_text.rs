// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Text columns are stored as plain byte arrays, without Parquet's UTF-8 annotation.
//!
//! A reader validates every value of a column annotated as UTF-8, on every read: a profile of the
//! benchmark's text queries put 6.5% of their CPU there, for text intake had already validated.
//! Without the annotation the reader trusts the bytes. Reads still get strings: DataFusion's scan
//! decodes a byte-array column as `Utf8` when the table says `Utf8`, and [`read_schema`] asks the
//! same of the other readers. Statistics, Bloom filters and dictionaries of byte arrays are those
//! of the same text.
//!
//! Only data files are stored this way; partition manifests keep their annotations.

use std::sync::Arc;

use arrow::{
    array::{ArrayRef, AsArray, BinaryArray, LargeBinaryArray, RecordBatch},
    datatypes::{DataType, Field, Schema, SchemaRef},
};
use kernel::{Error, Result};

/// The schema a data file is written with: text types as byte-array types.
pub(crate) fn stored_schema(schema: &Schema) -> SchemaRef {
    let fields: Vec<Field> = schema
        .fields()
        .iter()
        .map(|field| match stored_type(field.data_type()) {
            Some(data_type) => field.as_ref().clone().with_data_type(data_type),
            None => field.as_ref().clone(),
        })
        .collect();

    Arc::new(Schema::new_with_metadata(fields, schema.metadata().clone()))
}

/// `batch` as it is written: its text columns as byte arrays over the same buffers.
pub(crate) fn stored_batch(batch: &RecordBatch) -> Result<RecordBatch> {
    if !batch
        .schema()
        .fields()
        .iter()
        .any(|field| stored_type(field.data_type()).is_some())
    {
        return Ok(batch.clone());
    }

    let columns: Vec<ArrayRef> = batch.columns().iter().map(stored_column).collect();

    RecordBatch::try_new(stored_schema(batch.schema().as_ref()), columns)
        .map_err(|error| Error::internal(format!("stored text columns: {error}")))
}

/// The schema to read a data file with, given the one its footer holds: byte-array types as text
/// types. `None` when the file has no byte-array column.
pub(crate) fn read_schema(file_schema: &Schema) -> Option<SchemaRef> {
    let mut changed = false;

    let fields: Vec<Field> = file_schema
        .fields()
        .iter()
        .map(|field| {
            let data_type = match field.data_type() {
                DataType::Binary => DataType::Utf8,
                DataType::LargeBinary => DataType::LargeUtf8,
                DataType::BinaryView => DataType::Utf8View,
                _ => return field.as_ref().clone(),
            };

            changed = true;
            field.as_ref().clone().with_data_type(data_type)
        })
        .collect();

    changed.then(|| {
        Arc::new(Schema::new_with_metadata(
            fields,
            file_schema.metadata().clone(),
        ))
    })
}

fn stored_type(data_type: &DataType) -> Option<DataType> {
    match data_type {
        DataType::Utf8 => Some(DataType::Binary),
        DataType::LargeUtf8 => Some(DataType::LargeBinary),
        DataType::Utf8View => Some(DataType::BinaryView),
        _ => None,
    }
}

fn stored_column(column: &ArrayRef) -> ArrayRef {
    match column.data_type() {
        DataType::Utf8 => Arc::new(BinaryArray::from(column.as_string::<i32>().clone())),
        DataType::LargeUtf8 => Arc::new(LargeBinaryArray::from(column.as_string::<i64>().clone())),
        DataType::Utf8View => Arc::new(column.as_string_view().clone().to_binary_view()),
        _ => column.clone(),
    }
}

#[cfg(test)]
mod tests {
    use arrow::array::{Int64Array, StringArray};
    use bytes::Bytes;
    use parquet::{
        arrow::{
            ArrowWriter,
            arrow_reader::{ArrowReaderOptions, ParquetRecordBatchReaderBuilder},
        },
        basic::{ConvertedType, LogicalType},
    };

    use super::*;

    fn batch() -> RecordBatch {
        RecordBatch::try_new(
            Arc::new(Schema::new(vec![
                Field::new("message", DataType::Utf8, true),
                Field::new("n", DataType::Int64, false),
            ])),
            vec![
                Arc::new(StringArray::from(vec![Some("调用完成"), None])),
                Arc::new(Int64Array::from(vec![1, 2])),
            ],
        )
        .unwrap()
    }

    fn written(batch: &RecordBatch) -> Bytes {
        let stored = stored_batch(batch).unwrap();
        let mut bytes = Vec::new();
        let mut writer = ArrowWriter::try_new(&mut bytes, stored.schema(), None).unwrap();

        writer.write(&stored).unwrap();
        writer.close().unwrap();
        Bytes::from(bytes)
    }

    #[test]
    fn text_is_written_without_the_utf8_annotation_and_read_back_as_text() {
        let bytes = written(&batch());
        let builder = ParquetRecordBatchReaderBuilder::try_new(bytes.clone()).unwrap();
        let column = builder.parquet_schema().column(0);

        assert_eq!(column.converted_type(), ConvertedType::NONE);
        assert_ne!(column.logical_type_ref(), Some(&LogicalType::String));

        let schema = read_schema(builder.schema()).expect("a byte-array column");
        let reader = ParquetRecordBatchReaderBuilder::try_new_with_options(
            bytes,
            ArrowReaderOptions::new().with_schema(schema),
        )
        .unwrap()
        .build()
        .unwrap();

        let read: Vec<RecordBatch> = reader.map(Result::unwrap).collect();

        assert_eq!(read, [batch()]);
    }

    #[test]
    fn a_batch_without_text_is_stored_as_it_is() {
        let numbers = batch().project(&[1]).unwrap();

        assert_eq!(stored_batch(&numbers).unwrap(), numbers);
        assert!(read_schema(numbers.schema().as_ref()).is_none());
    }
}
