// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::Arc;

use arrow::{
    array::{Array, RecordBatch, StringArray, UInt32Array},
    datatypes::{DataType, Field, Schema},
};
use bytes::Bytes;
use parquet::{
    arrow::{ArrowWriter, arrow_reader::ParquetRecordBatchReaderBuilder},
    basic::Compression,
    file::properties::WriterProperties,
};

use crate::{
    domain::storage::PartitionManifest,
    shared::{Error, Result},
};

const SCHEMA_VERSION: u32 = 1;

pub(crate) fn encode(manifest: &PartitionManifest) -> Result<Bytes> {
    manifest.validate()?;
    let schema = Arc::new(Schema::new(vec![
        Field::new("schema_version", DataType::UInt32, false),
        Field::new("manifest_json", DataType::Utf8, false),
    ]));
    let json = serde_json::to_string(manifest)
        .map_err(|error| Error::internal(format!("serialize partition manifest: {error}")))?;
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(UInt32Array::from(vec![SCHEMA_VERSION])),
            Arc::new(StringArray::from(vec![json])),
        ],
    )
    .map_err(|error| Error::internal(format!("build partition manifest batch: {error}")))?;
    let properties = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .build();
    let mut output = Vec::with_capacity(64 * 1024);
    let mut writer = ArrowWriter::try_new(&mut output, schema, Some(properties))
        .map_err(|error| Error::internal(format!("open manifest parquet writer: {error}")))?;
    writer
        .write(&batch)
        .map_err(|error| Error::internal(format!("write manifest parquet: {error}")))?;
    writer
        .close()
        .map_err(|error| Error::internal(format!("close manifest parquet: {error}")))?;
    Ok(Bytes::from(output))
}

pub(super) fn decode(bytes: Bytes) -> Result<PartitionManifest> {
    let builder = ParquetRecordBatchReaderBuilder::try_new(bytes)
        .map_err(|error| Error::internal(format!("open manifest parquet: {error}")))?;
    let mut reader = builder
        .with_batch_size(1)
        .build()
        .map_err(|error| Error::internal(format!("build manifest parquet reader: {error}")))?;
    let batch = reader
        .next()
        .transpose()
        .map_err(|error| Error::internal(format!("read manifest parquet: {error}")))?
        .ok_or_else(|| Error::internal("partition manifest parquet is empty"))?;
    let has_extra_batch = reader
        .next()
        .transpose()
        .map_err(|error| Error::internal(format!("read manifest parquet: {error}")))?
        .is_some();
    if batch.num_rows() != 1 || has_extra_batch {
        return Err(Error::internal(
            "partition manifest parquet must contain exactly one row",
        ));
    }
    let version = batch
        .column_by_name("schema_version")
        .and_then(|array| array.as_any().downcast_ref::<UInt32Array>())
        .filter(|array| !array.is_null(0))
        .map(|array| array.value(0))
        .ok_or_else(|| Error::internal("manifest schema_version column is invalid"))?;
    if version != SCHEMA_VERSION {
        return Err(Error::internal(format!(
            "unsupported partition manifest schema version {version}"
        )));
    }
    let json = batch
        .column_by_name("manifest_json")
        .and_then(|array| array.as_any().downcast_ref::<StringArray>())
        .filter(|array| !array.is_null(0))
        .map(|array| array.value(0))
        .ok_or_else(|| Error::internal("manifest_json column is invalid"))?;
    let manifest: PartitionManifest = serde_json::from_str(json)
        .map_err(|error| Error::internal(format!("decode partition manifest: {error}")))?;
    manifest.validate()?;
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::storage::{
            Artifact, ArtifactId, ArtifactRole, ArtifactState, ArtifactTypeId, ColumnStats,
            DataSegment, ObjectChecksum, ObjectKey, Partition, PhysicalDatasetId, SegmentId,
            SegmentState, StoredObject, type_id,
        },
        shared::{
            ids::Id,
            time::{TimeRange, TimestampMicros},
        },
    };

    #[test]
    fn parquet_round_trip() {
        let partition = Partition {
            start_micros: 0,
            end_micros: 3_600_000_000,
            shard: 0,
        };
        let dataset_id = PhysicalDatasetId::from_string("dataset-a");
        let manifest = PartitionManifest {
            organization_id: Id::from_string("org-a"),
            dataset_id: dataset_id.clone(),
            partition,
            generation: 1,
            segments: vec![DataSegment {
                id: SegmentId::from_string("segment-a"),
                organization_id: Id::from_string("org-a"),
                dataset_id,
                partition,
                time_range: TimeRange::new(TimestampMicros(1), TimestampMicros(2)),
                sequence_range: None,
                row_count: 1,
                schema_fingerprint: None,
                column_stats: ColumnStats::default(),
                flush_id: None,
                output_ordinal: 0,
                primary: Artifact {
                    id: ArtifactId::from_string("artifact-a"),
                    role: ArtifactRole::PrimaryData,
                    artifact_type: ArtifactTypeId::builtin(type_id::builtin::ARTIFACT_PARQUET),
                    format_version: 1,
                    object: StoredObject {
                        key: ObjectKey::from_string("v1/artifacts/a"),
                        size_bytes: 1,
                        checksum: ObjectChecksum::from_string("b3:00"),
                        etag: None,
                    },
                    source_artifact_id: None,
                    source_checksum: None,
                    schema_fingerprint: None,
                    state: ArtifactState::Ready,
                    failure_reason: None,
                },
                auxiliaries: Vec::new(),
                state: SegmentState::Sealed,
                visible_from_version: 1,
                retired_at_version: None,
                created_at_micros: 0,
            }],
        };
        let encoded = encode(&manifest).unwrap();
        assert_eq!(decode(encoded).unwrap(), manifest);
    }
}
