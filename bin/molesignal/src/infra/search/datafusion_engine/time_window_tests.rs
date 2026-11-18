// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Files that a time window cuts through, through the engine. A count bounded by `_timestamp`
//! takes the files wholly inside its window from their row counts, scans the files on its edges
//! and skips the rest; a scan reads the files wholly inside the request's time range without the
//! time filter and still filters the files on its edges.

use std::sync::Arc;

use arrow::{
    array::{ArrayRef, Int64Array, RecordBatch, StringArray, TimestampMicrosecondArray},
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

/// The rows of the three files, one file per hour: each row's `value` is its own time.
const FILES: [&[i64]; 3] = [
    &[HOUR + 10, HOUR + 20, HOUR + 30],
    &[2 * HOUR + 10, 2 * HOUR + 20],
    &[3 * HOUR + 10, 3 * HOUR + 20, 3 * HOUR + 30, 3 * HOUR + 40],
];

struct Files(Vec<QueryFile>);

#[async_trait]
impl QueryFileSource for Files {
    async fn find(
        &self,
        _: &Id,
        _: &str,
        _: StreamType,
        range: TimeRange,
    ) -> Result<Vec<QueryFile>> {
        Ok(self
            .0
            .iter()
            .filter(|file| {
                file.time_range.end.0 >= range.start.0 && file.time_range.start.0 <= range.end.0
            })
            .cloned()
            .collect())
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
    let field = |name: &str, data_type| FieldDef {
        name: name.into(),
        data_type,
        nullable: true,
        index_type: StreamIndexType::None,
        encrypted: false,
    };

    StreamDefinition {
        id: Id::new(),
        org_id: Id::from_string("org-a"),
        name: "logs".into(),
        stream_type: StreamType::LOGS,
        schema: Schema {
            fields: vec![
                field("level", FieldType::Utf8),
                field("value", FieldType::Int64),
            ],
        },
        retention: None,
        created_at: TimestampMicros::now(),
        updated_at: TimestampMicros::now(),
    }
}

fn batch(times: &[i64]) -> RecordBatch {
    let schema = ArrowSchema::new(vec![
        Field::new(
            "_timestamp",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        ),
        Field::new("level", DataType::Utf8, true),
        Field::new("value", DataType::Int64, true),
    ]);

    let columns: Vec<ArrayRef> = vec![
        Arc::new(TimestampMicrosecondArray::from(times.to_vec()).with_timezone("UTC")),
        Arc::new(StringArray::from(vec!["INFO"; times.len()])),
        Arc::new(Int64Array::from(times.to_vec())),
    ];

    RecordBatch::try_new(Arc::new(schema), columns).unwrap()
}

async fn write(writer: &ParquetWriter, stream: &StreamDefinition, times: &[i64]) -> QueryFile {
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
        .write_compaction_catalog(stream, &dataset, batch(times))
        .await
        .expect("write the fixture");

    assert_eq!(segments.len(), 1, "a fixture file holds one hour");

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

async fn world() -> World {
    let directory = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(directory.path()).unwrap());

    let writer = ParquetWriter::new(store.clone());
    let stream = definition();
    let mut files = Vec::new();

    for times in FILES {
        files.push(write(&writer, &stream, times).await);
    }

    let engine = DataFusionEngine::new(Arc::new(Files(files)) as Arc<dyn QueryFileSource>, store)
        .with_streams(Arc::new(Streams(stream)));

    World {
        engine,
        _directory: directory,
    }
}

async fn run(world: &World, sql: &str, range: TimeRange) -> QueryResult {
    let request = QueryRequest {
        org_id: Id::from_string("org-a"),
        language: QueryLanguage::Sql,
        statement: sql.into(),
        time_range: range,
        stream: Some(StreamHint {
            name: "logs".into(),
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

fn everything() -> TimeRange {
    TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX))
}

/// `(count, rows scanned)` of `SELECT count(*)` under `condition`.
async fn count(world: &World, condition: &str) -> (Value, u64) {
    let result = run(
        world,
        &format!("SELECT count(*) AS n FROM logs WHERE {condition}"),
        everything(),
    )
    .await;

    (result.rows[0][0].clone(), result.scanned_rows)
}

#[tokio::test]
async fn a_count_takes_whole_files_from_their_row_counts_and_scans_its_edges() {
    let world = world().await;

    // The first two files lie inside the window; of the third, two rows do.
    assert_eq!(
        count(
            &world,
            &format!("_timestamp >= {HOUR} AND _timestamp < {}", 3 * HOUR + 25),
        )
        .await,
        (json!(7), 4)
    );

    // `>` and `<=` bound the same rows as `>= n + 1` and `< n + 1`.
    assert_eq!(
        count(
            &world,
            &format!(
                "CAST(_timestamp AS BIGINT) > {} AND CAST(_timestamp AS BIGINT) <= {}",
                HOUR + 10,
                3 * HOUR + 20
            ),
        )
        .await,
        (json!(6), 7)
    );
}

#[tokio::test]
async fn a_count_skips_files_outside_its_window() {
    let world = world().await;

    assert_eq!(
        count(
            &world,
            &format!("_timestamp >= {} AND _timestamp < {}", 2 * HOUR, 3 * HOUR),
        )
        .await,
        (json!(2), 0)
    );

    assert_eq!(
        count(&world, &format!("_timestamp >= {}", 4 * HOUR)).await,
        (json!(0), 0)
    );
}

/// The first file straddles the start of the request's time range, the second lies inside it.
#[tokio::test]
async fn a_scan_filters_only_the_files_on_an_edge_of_the_time_range() {
    let world = world().await;

    let range = TimeRange::new(TimestampMicros(HOUR + 15), TimestampMicros(3 * HOUR));
    let result = run(&world, "SELECT value FROM logs ORDER BY value", range).await;

    let values = result
        .rows
        .iter()
        .map(|row| row[0].clone())
        .collect::<Vec<_>>();

    assert_eq!(
        values,
        [HOUR + 20, HOUR + 30, 2 * HOUR + 10, 2 * HOUR + 20].map(|value| json!(value))
    );
}
