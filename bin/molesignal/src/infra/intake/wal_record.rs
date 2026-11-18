// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A WAL record: one accepted batch, written as an Arrow IPC stream. The schema's metadata names
//! the stream the batch belongs to and carries its usage measurement, so replay needs nothing else.
//!
//! The columns are those the batch has in the dataset buffer (see
//! [`row_columns`](super::row_columns)), so replay appends them as they are: no parsing, and
//! encrypted fields are already ciphertext.

use std::{collections::HashMap, io::Cursor, sync::Arc};

use anyhow::{Context, Result, anyhow};
use arrow::{
    array::RecordBatch,
    ipc::{reader::StreamReader, writer::StreamWriter},
};
use domain::stream::StreamType;
use kernel::ids::Id;

const ORG: &str = "molesignal.org_id";
const STREAM: &str = "molesignal.stream";
const STREAM_TYPE: &str = "molesignal.stream_type";
const COLLECTED_BYTES: &str = "molesignal.collected_bytes";

/// One accepted batch and what replay needs to put it back into its buffer.
#[derive(Debug, Clone)]
pub struct WalBatch {
    pub org_id: Id,
    pub stream: String,
    pub stream_type: StreamType,
    /// The batch's canonical bytes for usage metering; `None` for data that is not metered.
    pub collected_bytes: Option<u64>,
    pub columns: RecordBatch,
}

/// The record's bytes.
pub fn encode(batch: &WalBatch) -> Result<Vec<u8>> {
    let mut metadata = HashMap::from([
        (ORG.to_owned(), batch.org_id.as_str().to_owned()),
        (STREAM.to_owned(), batch.stream.clone()),
        (
            STREAM_TYPE.to_owned(),
            batch.stream_type.as_str().to_owned(),
        ),
    ]);

    if let Some(bytes) = batch.collected_bytes {
        metadata.insert(COLLECTED_BYTES.to_owned(), bytes.to_string());
    }

    let schema = Arc::new(
        batch
            .columns
            .schema()
            .as_ref()
            .clone()
            .with_metadata(metadata),
    );

    let columns = batch
        .columns
        .clone()
        .with_schema(schema.clone())
        .context("wal record schema")?;

    let mut writer = StreamWriter::try_new(Vec::new(), &schema).context("wal record writer")?;
    writer.write(&columns).context("wal record write")?;
    writer.finish().context("wal record finish")?;
    writer.into_inner().context("wal record bytes")
}

/// The batch a record holds.
pub fn decode(bytes: &[u8]) -> Result<WalBatch> {
    let mut reader =
        StreamReader::try_new(Cursor::new(bytes), None).context("wal record reader")?;
    let schema = reader.schema();
    let metadata = schema.metadata();
    let field = |key: &str| {
        metadata
            .get(key)
            .ok_or_else(|| anyhow!("wal record has no `{key}`"))
    };

    let org_id = Id::from_string(field(ORG)?.as_str());
    let stream = field(STREAM)?.clone();
    let stream_type = StreamType::new(field(STREAM_TYPE)?.as_str())
        .map_err(|error| anyhow!("wal record stream type: {error}"))?;

    let collected_bytes = metadata
        .get(COLLECTED_BYTES)
        .map(|bytes| bytes.parse::<u64>())
        .transpose()
        .context("wal record collected bytes")?;

    let columns = reader
        .next()
        .ok_or_else(|| anyhow!("wal record holds no batch"))?
        .context("wal record batch")?;

    if reader.next().is_some() {
        return Err(anyhow!("wal record holds more than one batch"));
    }

    Ok(WalBatch {
        org_id,
        stream,
        stream_type,
        collected_bytes,
        columns,
    })
}

#[cfg(test)]
mod tests {
    use arrow::{
        array::{Array, AsArray, Int64Array, StringArray},
        datatypes::{DataType, Field, Int64Type, Schema},
    };

    use super::*;

    fn batch(collected_bytes: Option<u64>) -> WalBatch {
        let schema = Schema::new(vec![
            Field::new("message", DataType::Utf8, true),
            Field::new("latency", DataType::Int64, true),
        ]);

        WalBatch {
            org_id: Id::from_string("org-a"),
            stream: "app".into(),
            stream_type: StreamType::LOGS,
            collected_bytes,
            columns: RecordBatch::try_new(
                Arc::new(schema),
                vec![
                    Arc::new(StringArray::from(vec![Some("a"), None])),
                    Arc::new(Int64Array::from(vec![Some(1), Some(2)])),
                ],
            )
            .unwrap(),
        }
    }

    #[test]
    fn a_record_gives_back_its_batch_and_stream() {
        for collected_bytes in [Some(1234), None] {
            let decoded = decode(&encode(&batch(collected_bytes)).unwrap()).unwrap();

            assert_eq!(decoded.org_id, Id::from_string("org-a"));
            assert_eq!(decoded.stream, "app");
            assert_eq!(decoded.stream_type, StreamType::LOGS);
            assert_eq!(decoded.collected_bytes, collected_bytes);
            assert_eq!(decoded.columns.num_rows(), 2);
            assert_eq!(decoded.columns.column(0).as_string::<i32>().value(0), "a");
            assert!(decoded.columns.column(0).is_null(1));
            assert_eq!(
                decoded
                    .columns
                    .column(1)
                    .as_primitive::<Int64Type>()
                    .value(1),
                2
            );
        }
    }

    #[test]
    fn bytes_that_are_not_a_record_are_an_error() {
        assert!(decode(b"not a record").is_err());
        assert!(decode(&[]).is_err());
    }
}
