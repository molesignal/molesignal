// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A field widened from Int64 to Float64, through the engine: the files written while it was
//! Int64 are read as Float64 beside the files written after, in scans, filters and aggregates.

use std::sync::Arc;

use arrow::{
    array::{ArrayRef, Float64Array, Int64Array, RecordBatch, TimestampMicrosecondArray},
    datatypes::{DataType, Field, Schema as ArrowSchema, TimeUnit},
};
use async_trait::async_trait;
use domain::{
    query::{QueryEngine, QueryLanguage, QueryRequest, QueryResult, StreamHint},
    storage::{
        DatasetState, PhysicalDataset, PhysicalDatasetId, QueryFile, QueryFileSource,
        builtin_registry, primary_dataset_type,
    },
    stream::{
        FieldDef, FieldType, Schema, StreamDefinition, StreamIndexType, StreamRepository,
        StreamType,
    },
};
use kernel::{
    Error, Result,
    ids::Id,
    time::{TimeRange, TimestampMicros},
};
use object_store::{ObjectStore, local::LocalFileSystem};
use serde_json::{Value, json};

use super::DataFusionEngine;
use crate::infra::storage::parquet::writer::ParquetWriter;

const HOUR: i64 = 3_600_000_000;

struct Files(Vec<QueryFile>);

#[async_trait]
impl QueryFileSource for Files {
    async fn find(&self, _: &Id, _: &str, _: StreamType, _: TimeRange) -> Result<Vec<QueryFile>> {
        Ok(self.0.clone())
    }
}

struct Streams(StreamDefinition);

#[async_trait]
impl StreamRepository for Streams {
    async fn create(&self, definition: StreamDefinition) -> Result<StreamDefinition> {
        Ok(definition)
    }

    async fn update_schema(&self, _: &Id, _: Schema) -> Result<()> {
        Ok(())
    }

    async fn get(&self, _: &Id, name: &str, _: StreamType) -> Result<StreamDefinition> {
        (name == self.0.name)
            .then(|| self.0.clone())
            .ok_or_else(|| Error::not_found("stream"))
    }

    async fn list(&self, _: &Id) -> Result<Vec<StreamDefinition>> {
        Ok(vec![self.0.clone()])
    }

    async fn delete(&self, _: &Id) -> Result<()> {
        Ok(())
    }
}

fn definition(id: &Id, value_type: FieldType) -> StreamDefinition {
    StreamDefinition {
        id: id.clone(),
        org_id: Id::from_string("org-a"),
        name: "host".into(),
        stream_type: StreamType::LOGS,
        schema: Schema {
            fields: vec![FieldDef {
                name: "value".into(),
                data_type: value_type,
                nullable: true,
                index_type: StreamIndexType::None,
                encrypted: false,
            }],
        },
        retention: None,
        created_at: TimestampMicros::now(),
        updated_at: TimestampMicros::now(),
    }
}

fn batch(times: &[i64], values: ArrayRef) -> RecordBatch {
    let schema = ArrowSchema::new(vec![
        Field::new(
            "_timestamp",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        ),
        Field::new("value", values.data_type().clone(), true),
    ]);

    let columns: Vec<ArrayRef> = vec![
        Arc::new(TimestampMicrosecondArray::from(times.to_vec()).with_timezone("UTC")),
        values,
    ];

    RecordBatch::try_new(Arc::new(schema), columns).unwrap()
}

async fn write(writer: &ParquetWriter, stream: &StreamDefinition, batch: RecordBatch) -> QueryFile {
    let dataset_type = primary_dataset_type(stream.stream_type).unwrap();

    let spec = builtin_registry()
        .dataset_type(&stream.stream_type, &dataset_type)
        .expect("built-in raw dataset descriptor")
        .to_spec();

    let dataset = PhysicalDataset {
        id: PhysicalDatasetId::from_string(format!("fixture-{}", stream.id)),
        organization_id: stream.org_id.clone(),
        logical_stream_id: stream.id.clone(),
        dataset_type: dataset_type.clone(),
        dataset_type_version: spec.dataset_type_version,
        partition_policy: spec.partition_policy,
        storage_policy: spec.storage_policy,
        index_policy: spec.index_policy,
        catalog_version: 0,
        state: DatasetState::Active,
        created_at_micros: 0,
        updated_at_micros: 0,
    };

    let mut segments = writer
        .write_compaction_catalog(stream, &dataset, batch)
        .await
        .expect("write the fixture");

    let segment = segments.remove(0);

    QueryFile {
        id: segment.id.0,
        org_id: stream.org_id.clone(),
        stream: stream.name.clone(),
        stream_type: stream.stream_type,
        dataset_type,
        object_key: segment.primary.object.key.0,
        checksum: Some(segment.primary.object.checksum),
        etag: segment.primary.object.etag,
        time_range: segment.time_range,
        rows: segment.row_count,
        size_bytes: segment.primary.object.size_bytes,
        min_values: segment.column_stats.min_values,
        max_values: segment.column_stats.max_values,
        value_sets: segment.column_stats.value_sets,
        address_ranges: segment.column_stats.address_ranges,
        null_counts: segment.column_stats.null_counts,
        indexed_at_micros: segment.column_stats.indexed_at_micros,
        shard: None,
        clustering: Default::default(),
    }
}

struct World {
    engine: DataFusionEngine,
    _directory: tempfile::TempDir,
}

/// An hour of Int64 values written before the widening, and an hour of Float64 values after.
async fn world() -> World {
    let directory = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(directory.path()).unwrap());

    let writer = ParquetWriter::new(store.clone());
    let id = Id::new();
    let before = definition(&id, FieldType::Int64);
    let after = definition(&id, FieldType::Float64);

    let files = vec![
        write(
            &writer,
            &before,
            batch(
                &[HOUR + 10, HOUR + 20],
                Arc::new(Int64Array::from(vec![1, 2])),
            ),
        )
        .await,
        write(
            &writer,
            &after,
            batch(
                &[2 * HOUR + 10, 2 * HOUR + 20],
                Arc::new(Float64Array::from(vec![0.5, 2.5])),
            ),
        )
        .await,
    ];

    let engine = DataFusionEngine::new(Arc::new(Files(files)) as Arc<dyn QueryFileSource>, store)
        .with_streams(Arc::new(Streams(after)));

    World {
        engine,
        _directory: directory,
    }
}

async fn run(world: &World, sql: &str) -> QueryResult {
    let request = QueryRequest {
        org_id: Id::from_string("org-a"),
        language: QueryLanguage::Sql,
        statement: sql.into(),
        time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX)),
        stream: Some(StreamHint {
            name: "host".into(),
            stream_type: StreamType::LOGS,
        }),
        limit: None,
        step: None,
        federation_clusters: Vec::new(),
    };

    world
        .engine
        .execute(request)
        .await
        .unwrap_or_else(|error| panic!("{sql}: {error}"))
}

fn column(result: &QueryResult) -> Vec<Value> {
    result.rows.iter().map(|row| row[0].clone()).collect()
}

#[tokio::test]
async fn files_from_before_a_widening_read_as_float() {
    let world = world().await;

    let values = run(&world, "SELECT value FROM host ORDER BY value").await;
    assert_eq!(
        column(&values),
        [json!(0.5), json!(1.0), json!(2.0), json!(2.5)]
    );

    let filtered = run(
        &world,
        "SELECT value FROM host WHERE value > 1.5 ORDER BY value",
    )
    .await;
    assert_eq!(column(&filtered), [json!(2.0), json!(2.5)]);

    let total = run(&world, "SELECT sum(value) AS total FROM host").await;
    assert_eq!(column(&total), [json!(6.0)]);
}
