// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Files whose rows are clustered, through the engine. Their rows are in time slices ordered by a
//! field, not in time order, so a query for the newest or the oldest rows must not take a file's
//! first rows for its newest: it gets the rows it gets from files in time order, and an equality
//! on the clustering field finds every row of the value.

use std::sync::Arc;

use arrow::{
    array::{ArrayRef, Int64Array, RecordBatch, StringArray, TimestampMicrosecondArray},
    datatypes::{DataType, Field, Schema as ArrowSchema, TimeUnit},
};
use async_trait::async_trait;
use domain::{
    query::{QueryEngine, QueryLanguage, QueryRequest, StreamHint},
    storage::{
        DatasetState, FlushProvenance, PhysicalDataset, PhysicalDatasetId, QueryFile,
        QueryFileSource, SequenceRange, WalSequence, WriterEpoch, WriterNodeId, builtin_registry,
        primary_dataset_type,
    },
    stream::{
        Clustering, FieldDef, FieldType, Schema, StreamDefinition, StreamIndexType,
        StreamRepository, StreamType,
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
const SECOND: i64 = 1_000_000;
const SERVICES: [&str; 3] = ["a", "b", "c"];

/// The rows of the fixture: one every three seconds over three minutes of one hour, the services
/// in turn, each row's `value` its own time.
fn rows() -> Vec<(i64, &'static str)> {
    (0..60)
        .map(|row| (HOUR + row * 3 * SECOND, SERVICES[row as usize % 3]))
        .collect()
}

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
                field("service", FieldType::Utf8),
                field("value", FieldType::Int64),
            ],
        },
        retention: None,
        created_at: TimestampMicros::now(),
        updated_at: TimestampMicros::now(),
    }
}

fn batch(rows: &[(i64, &str)]) -> RecordBatch {
    let schema = ArrowSchema::new(vec![
        Field::new(
            "_timestamp",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        ),
        Field::new("service", DataType::Utf8, true),
        Field::new("value", DataType::Int64, true),
    ]);

    let times: Vec<i64> = rows.iter().map(|(time, _)| *time).collect();

    let columns: Vec<ArrayRef> = vec![
        Arc::new(TimestampMicrosecondArray::from(times.clone()).with_timezone("UTC")),
        Arc::new(StringArray::from(
            rows.iter().map(|(_, service)| *service).collect::<Vec<_>>(),
        )),
        Arc::new(Int64Array::from(times)),
    ];

    RecordBatch::try_new(Arc::new(schema), columns).unwrap()
}

async fn write(
    writer: &ParquetWriter,
    stream: &StreamDefinition,
    ordinal: u64,
    rows: &[(i64, &str)],
) -> QueryFile {
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

    let provenance = FlushProvenance::derive(
        WriterNodeId::new("node-a"),
        WriterEpoch(1),
        SequenceRange::new(WalSequence(ordinal * 2 + 1), WalSequence(ordinal * 2 + 2)),
    );

    let clustering = Clustering::new(vec!["service".into()], 60 * SECOND);

    let mut segments = writer
        .flush_catalog_clustered(
            stream,
            &dataset,
            &provenance,
            batch(rows),
            None,
            Some(&clustering),
        )
        .await
        .expect("write the fixture");

    assert_eq!(segments.len(), 1, "a fixture file holds one hour");

    let segment = segments.remove(0);

    assert_eq!(
        segment.column_stats.clustering, clustering,
        "the file records the clustering its rows were written in"
    );

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
        clustering: segment.column_stats.clustering,
    }
}

struct World {
    engine: DataFusionEngine,
    _directory: tempfile::TempDir,
}

/// Two clustered files, the first and the second half of the rows.
async fn world() -> World {
    let directory = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(directory.path()).unwrap());

    let writer = ParquetWriter::new(store.clone());
    let stream = definition();
    let rows = rows();

    let files = vec![
        write(&writer, &stream, 0, &rows[..30]).await,
        write(&writer, &stream, 1, &rows[30..]).await,
    ];

    let engine = DataFusionEngine::new(Arc::new(Files(files)) as Arc<dyn QueryFileSource>, store)
        .with_streams(Arc::new(Streams(stream)));

    World {
        engine,
        _directory: directory,
    }
}

async fn values(world: &World, sql: &str) -> Vec<Value> {
    let request = QueryRequest {
        org_id: Id::from_string("org-a"),
        language: QueryLanguage::Sql,
        statement: sql.into(),
        time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX)),
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
        .rows
        .into_iter()
        .map(|row| row[0].clone())
        .collect()
}

/// The times of the rows of `service` (all with `None`), newest first.
fn newest(service: Option<&str>) -> Vec<Value> {
    let mut times: Vec<i64> = rows()
        .into_iter()
        .filter(|(_, row_service)| service.is_none_or(|service| service == *row_service))
        .map(|(time, _)| time)
        .collect();

    times.sort_unstable_by(|left, right| right.cmp(left));
    times.into_iter().map(|time| json!(time)).collect()
}

#[tokio::test]
async fn the_newest_and_the_oldest_rows_of_clustered_files_are_the_newest_and_the_oldest() {
    let world = world().await;

    assert_eq!(
        values(
            &world,
            "SELECT value FROM logs ORDER BY _timestamp DESC LIMIT 5"
        )
        .await,
        newest(None)[..5]
    );

    let mut oldest = newest(None);
    oldest.reverse();

    assert_eq!(
        values(
            &world,
            "SELECT value FROM logs ORDER BY _timestamp ASC LIMIT 4"
        )
        .await,
        oldest[..4]
    );
}

#[tokio::test]
async fn an_equality_on_the_clustering_field_finds_every_row_of_its_value() {
    let world = world().await;

    assert_eq!(
        values(
            &world,
            "SELECT value FROM logs WHERE service = 'b' ORDER BY _timestamp DESC LIMIT 3"
        )
        .await,
        newest(Some("b"))[..3]
    );

    assert_eq!(
        values(
            &world,
            "SELECT count(*) FROM logs WHERE service = 'c' AND value >= 0"
        )
        .await,
        vec![json!(20)]
    );
}
