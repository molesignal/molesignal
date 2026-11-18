// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Counts with numeric comparisons, through the engine. The numbers of a `range` field are in the
//! index archive, so `count(*)` with `>=`, `>`, `<` or `<=` on it is answered from the archives of
//! the files inside the window, with nothing scanned. On a field without the index the same count
//! is scanned, with the same answer.

use std::{collections::HashMap, sync::Arc};

use arrow::{
    array::{ArrayRef, Int64Array, RecordBatch, TimestampMicrosecondArray},
    datatypes::{DataType, Field, Schema as ArrowSchema, TimeUnit},
};
use async_trait::async_trait;
use domain::{
    query::{QueryEngine, QueryLanguage, QueryRequest, StreamHint},
    storage::{
        ArtifactState, DatasetState, PhysicalDataset, PhysicalDatasetId, QueryFile,
        QueryFileSource, builtin_registry, primary_dataset_type, type_id,
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
use settings::CacheLayerSettings;

use super::DataFusionEngine;
use crate::infra::{
    caching::IndexHandleCache, query::tantivy_pruner::TantivyPruner,
    search::tantivy_index::IndexHandle, storage::parquet::writer::ParquetWriter,
};

const HOUR: i64 = 3_600_000_000;

/// The durations of the rows of two files, one an hour; `value` repeats each duration on a field
/// without the index.
const FILES: [&[i64]; 2] = [&[50, 150, 999, 1000, 2500], &[10, 1000, 1001, 400]];

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

fn definition() -> StreamDefinition {
    let field = |name: &str, index_type| {
        let mut field = FieldDef {
            name: name.into(),
            data_type: FieldType::Int64,
            nullable: true,
            index_type: StreamIndexType::None,
            encrypted: false,
        };
        field.index_type = index_type;
        field
    };

    StreamDefinition {
        id: Id::new(),
        org_id: Id::from_string("org-a"),
        name: "logs".into(),
        stream_type: StreamType::LOGS,
        schema: Schema {
            fields: vec![
                field("duration", StreamIndexType::Range),
                field("value", StreamIndexType::Skip),
            ],
        },
        retention: None,
        created_at: TimestampMicros::now(),
        updated_at: TimestampMicros::now(),
    }
}

fn batch(hour: i64, durations: &[i64]) -> RecordBatch {
    let schema = ArrowSchema::new(vec![
        Field::new(
            "_timestamp",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        ),
        Field::new("duration", DataType::Int64, true),
        Field::new("value", DataType::Int64, true),
    ]);

    let times: Vec<i64> = (0..durations.len() as i64)
        .map(|row| hour * HOUR + row * 1_000)
        .collect();

    let columns: Vec<ArrayRef> = vec![
        Arc::new(TimestampMicrosecondArray::from(times).with_timezone("UTC")),
        Arc::new(Int64Array::from(durations.to_vec())),
        Arc::new(Int64Array::from(durations.to_vec())),
    ];

    RecordBatch::try_new(Arc::new(schema), columns).unwrap()
}

/// Writes one file of `durations` in `hour`; returns it with the key of its index archive.
async fn write(
    writer: &ParquetWriter,
    stream: &StreamDefinition,
    hour: i64,
    durations: &[i64],
) -> (QueryFile, String) {
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
        .write_compaction_catalog(stream, &dataset, batch(hour, durations))
        .await
        .expect("write the fixture");

    assert_eq!(segments.len(), 1, "a fixture file holds one hour");

    let segment = segments.remove(0);

    let index = segment
        .auxiliaries
        .iter()
        .find(|artifact| {
            artifact.artifact_type.as_str() == type_id::builtin::ARTIFACT_TANTIVY
                && artifact.state == ArtifactState::Ready
        })
        .expect("a range field gets an index archive")
        .object
        .key
        .0
        .clone();

    let file = QueryFile {
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
        clustering: segment.column_stats.clustering,
    };

    (file, index)
}

struct World {
    engine: DataFusionEngine,
    _directory: tempfile::TempDir,
}

async fn world() -> World {
    let directory = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(directory.path()).unwrap());

    let writer = ParquetWriter::new(store.clone());
    let stream = definition();
    let mut files = Vec::new();
    let mut index_keys = HashMap::new();

    for (hour, durations) in FILES.into_iter().enumerate() {
        let (file, index) = write(&writer, &stream, hour as i64 + 1, durations).await;
        index_keys.insert(file.object_key.clone(), index);
        files.push(file);
    }

    let handles: Arc<IndexHandleCache<Arc<IndexHandle>>> =
        Arc::new(IndexHandleCache::new(CacheLayerSettings::new(100, 60)));

    let engine = DataFusionEngine::new(
        Arc::new(Files(files)) as Arc<dyn QueryFileSource>,
        store.clone(),
    )
    .with_streams(Arc::new(Streams(stream)))
    .with_tantivy_pruner(Arc::new(TantivyPruner::new(handles, store)))
    .with_tantivy_index_keys(index_keys);

    World {
        engine,
        _directory: directory,
    }
}

/// `(count, rows scanned)` of `SELECT count(*)` under `condition`.
async fn count(world: &World, condition: &str) -> (Value, u64) {
    let sql = format!("SELECT count(*) AS n FROM logs WHERE {condition}");

    let request = QueryRequest {
        org_id: Id::from_string("org-a"),
        language: QueryLanguage::Sql,
        statement: sql.clone(),
        time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX)),
        stream: Some(StreamHint {
            name: "logs".into(),
            stream_type: StreamType::LOGS,
        }),
        limit: None,
        step: None,
        federation_clusters: Vec::new(),
    };

    let result = world
        .engine
        .execute(request)
        .await
        .unwrap_or_else(|error| panic!("{sql}: {error}"));

    (result.rows[0][0].clone(), result.scanned_rows)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_count_with_numeric_bounds_on_a_range_field_reads_only_the_index() {
    let world = world().await;

    assert_eq!(count(&world, "duration >= 1000").await, (json!(4), 0));
    assert_eq!(
        count(&world, "duration >= 100 AND 1000 > duration").await,
        (json!(3), 0)
    );
    assert_eq!(count(&world, "duration > 2500.5").await, (json!(0), 0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_count_with_numeric_bounds_on_another_field_is_scanned_to_the_same_answer() {
    let world = world().await;

    let (count_value, scanned) = count(&world, "value >= 1000").await;

    assert_eq!(count_value, json!(4));
    assert!(scanned > 0, "a field without the index is scanned");
}
