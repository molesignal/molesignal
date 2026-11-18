// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Counts answered from segments' minute counts, through the engine. The files are clustered by
//! `service` and `level`, with no index: a count, a count by service and bars of whole minutes
//! with equality conditions on those fields read nothing, also where the window cuts a file
//! between minutes. A window that cuts a minute, or a condition on another field, is scanned to
//! the same answer. A page of the newest or oldest rows is read from the minutes the counts point
//! to.

use std::{collections::HashMap, sync::Arc};

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
        primary_dataset_type, type_id,
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

const MINUTE: i64 = 60_000_000;
const SECOND: i64 = 1_000_000;

/// The first minute of the rows, an hour after the epoch.
const FIRST_MINUTE: i64 = 60;

/// A row: its time, service, level and host. Its `value` is its time.
type Row = (i64, &'static str, &'static str, &'static str);

/// One row a second over four minutes: the services in turn, every fourth row an error, the
/// hosts in turn.
fn rows() -> Vec<Row> {
    (0..240)
        .map(|row| {
            (
                FIRST_MINUTE * MINUTE + row * SECOND,
                ["a", "b", "c"][row as usize % 3],
                if row % 4 == 0 { "ERROR" } else { "INFO" },
                ["h0", "h1"][row as usize % 2],
            )
        })
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
                field("level", FieldType::Utf8),
                field("host", FieldType::Utf8),
                field("value", FieldType::Int64),
            ],
        },
        retention: None,
        created_at: TimestampMicros::now(),
        updated_at: TimestampMicros::now(),
    }
}

fn batch(rows: &[Row]) -> RecordBatch {
    let schema = ArrowSchema::new(vec![
        Field::new(
            "_timestamp",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        ),
        Field::new("service", DataType::Utf8, true),
        Field::new("level", DataType::Utf8, true),
        Field::new("host", DataType::Utf8, true),
        Field::new("value", DataType::Int64, true),
    ]);

    let text = |pick: fn(&Row) -> &'static str| -> ArrayRef {
        Arc::new(StringArray::from(rows.iter().map(pick).collect::<Vec<_>>()))
    };

    let columns: Vec<ArrayRef> = vec![
        Arc::new(
            TimestampMicrosecondArray::from(rows.iter().map(|row| row.0).collect::<Vec<_>>())
                .with_timezone("UTC"),
        ),
        text(|row| row.1),
        text(|row| row.2),
        text(|row| row.3),
        Arc::new(Int64Array::from(
            rows.iter().map(|row| row.0).collect::<Vec<_>>(),
        )),
    ];

    RecordBatch::try_new(Arc::new(schema), columns).unwrap()
}

/// Writes one clustered file of `rows`; returns it with the key of its minute counts.
async fn write(
    writer: &ParquetWriter,
    stream: &StreamDefinition,
    ordinal: u64,
    rows: &[Row],
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

    let provenance = FlushProvenance::derive(
        WriterNodeId::new("node-a"),
        WriterEpoch(1),
        SequenceRange::new(WalSequence(ordinal * 2 + 1), WalSequence(ordinal * 2 + 2)),
    );

    let clustering = Clustering::new(vec!["service".into(), "level".into()], MINUTE);

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

    let counts = segment
        .auxiliaries
        .iter()
        .find(|artifact| {
            artifact.artifact_type.as_str() == type_id::builtin::ARTIFACT_MINUTE_COUNTS
        })
        .expect("a clustered file gets minute counts")
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

    (file, counts)
}

struct World {
    engine: DataFusionEngine,
    _directory: tempfile::TempDir,
}

/// Two clustered files: the first two minutes of the rows, and the last two.
async fn world() -> World {
    let directory = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(directory.path()).unwrap());

    let writer = ParquetWriter::new(store.clone());
    let stream = definition();
    let rows = rows();
    let mut files = Vec::new();
    let mut keys = HashMap::new();

    for (ordinal, part) in [&rows[..120], &rows[120..]].into_iter().enumerate() {
        let (file, counts) = write(&writer, &stream, ordinal as u64, part).await;
        keys.insert(file.id.as_str().to_owned(), counts);
        files.push(file);
    }

    let engine = DataFusionEngine::new(Arc::new(Files(files)) as Arc<dyn QueryFileSource>, store)
        .with_streams(Arc::new(Streams(stream)))
        .with_minute_count_keys(keys);

    World {
        engine,
        _directory: directory,
    }
}

/// The result rows of `sql`, and the rows the engine read.
async fn run(world: &World, sql: &str) -> (Vec<Vec<Value>>, u64) {
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

    let result = world
        .engine
        .execute(request)
        .await
        .unwrap_or_else(|error| panic!("{sql}: {error}"));

    (result.rows, result.scanned_rows)
}

/// Rows in `[from, to)` that `keep` keeps.
fn count(from: i64, to: i64, keep: impl Fn(&Row) -> bool) -> i64 {
    rows()
        .iter()
        .filter(|row| from <= row.0 && row.0 < to && keep(row))
        .count() as i64
}

fn window(from: i64, to: i64) -> String {
    format!("_timestamp >= {from} AND _timestamp < {to}")
}

#[tokio::test]
async fn counts_with_conditions_on_the_clustering_fields_read_nothing() {
    let world = world().await;

    assert_eq!(
        run(&world, "SELECT count(*) FROM logs WHERE service = 'a'").await,
        (vec![vec![json!(count(0, i64::MAX, |row| row.1 == "a"))]], 0)
    );

    // The window cuts both files between minutes.
    let (from, to) = ((FIRST_MINUTE + 1) * MINUTE, (FIRST_MINUTE + 3) * MINUTE);

    assert_eq!(
        run(
            &world,
            &format!(
                "SELECT count(*) FROM logs WHERE {} AND level = 'ERROR' AND service = 'b'",
                window(from, to)
            )
        )
        .await,
        (
            vec![vec![json!(
                count(from, to, |row| row.2 == "ERROR" && row.1 == "b")
            )]],
            0
        )
    );

    assert_eq!(
        run(
            &world,
            &format!("SELECT count(*) FROM logs WHERE {}", window(from, to))
        )
        .await,
        (vec![vec![json!(count(from, to, |_| true))]], 0)
    );
}

#[tokio::test]
async fn a_window_inside_a_minute_or_another_field_is_scanned_to_the_same_answer() {
    let world = world().await;
    let (from, to) = ((FIRST_MINUTE + 1) * MINUTE + 30 * SECOND, i64::MAX);

    let (rows, scanned) = run(
        &world,
        &format!(
            "SELECT count(*) FROM logs WHERE {} AND level = 'ERROR'",
            window(from, to)
        ),
    )
    .await;

    assert_eq!(
        rows,
        vec![vec![json!(count(from, to, |row| row.2 == "ERROR"))]]
    );
    assert_eq!(
        scanned, 120,
        "only the file the window cuts inside a minute"
    );

    let (rows, scanned) = run(&world, "SELECT count(*) FROM logs WHERE host = 'h1'").await;

    assert_eq!(
        rows,
        vec![vec![json!(count(0, i64::MAX, |row| row.3 == "h1"))]]
    );
    assert_eq!(scanned, 240);
}

#[tokio::test]
async fn the_services_with_most_errors_read_nothing() {
    let world = world().await;

    let (rows, scanned) = run(
        &world,
        "SELECT service, COUNT(*) AS n FROM logs WHERE level = 'ERROR' \
         GROUP BY service ORDER BY n DESC, service ASC LIMIT 2",
    )
    .await;

    let mut expected = ["a", "b", "c"]
        .into_iter()
        .map(|service| {
            let errors = count(0, i64::MAX, |row| row.1 == service && row.2 == "ERROR");
            (service, errors)
        })
        .collect::<Vec<_>>();

    expected.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(right.0)));

    let expected = expected[..2]
        .iter()
        .map(|(service, errors)| vec![json!(service), json!(errors)])
        .collect::<Vec<_>>();

    assert_eq!((rows, scanned), (expected, 0));
}

#[tokio::test]
async fn errors_by_minute_read_nothing() {
    let world = world().await;
    let (from, to) = (FIRST_MINUTE * MINUTE, (FIRST_MINUTE + 3) * MINUTE);

    let (rows, scanned) = run(
        &world,
        &format!(
            "SELECT CAST(FLOOR(CAST(_timestamp AS BIGINT) / {MINUTE}) AS BIGINT) * {MINUTE} \
             AS bucket, COUNT(*) AS n FROM logs WHERE {} AND level = 'ERROR' \
             GROUP BY bucket ORDER BY bucket ASC",
            window(from, to)
        ),
    )
    .await;

    let expected = (FIRST_MINUTE..FIRST_MINUTE + 3)
        .map(|minute| {
            let start = minute * MINUTE;
            let errors = count(start, start + MINUTE, |row| row.2 == "ERROR");
            vec![json!(start), json!(errors)]
        })
        .collect::<Vec<_>>();

    assert_eq!((rows, scanned), (expected, 0));
}

#[tokio::test]
async fn an_explorer_histogram_of_whole_minutes_reads_nothing() {
    let world = world().await;
    let (from, to) = (FIRST_MINUTE * MINUTE, (FIRST_MINUTE + 4) * MINUTE);
    let width = 2 * MINUTE;

    let (rows, scanned) = run(
        &world,
        &format!(
            "SELECT (CAST(_timestamp AS BIGINT) - {from}) / {width} AS b, COUNT(*) AS t, \
             SUM(CASE WHEN UPPER(CAST(level AS VARCHAR)) = 'ERROR' THEN 1 ELSE 0 END) AS e \
             FROM logs WHERE CAST(_timestamp AS BIGINT) >= {from} \
             AND CAST(_timestamp AS BIGINT) < {to} AND service = 'c' GROUP BY 1"
        ),
    )
    .await;

    let expected = (0..2)
        .map(|bar| {
            let start = from + bar * width;
            let total = count(start, start + width, |row| row.1 == "c");
            let errors = count(start, start + width, |row| row.1 == "c" && row.2 == "ERROR");
            vec![json!(bar), json!(total), json!(errors)]
        })
        .collect::<Vec<_>>();

    assert_eq!((rows, scanned), (expected, 0));
}

/// The times of the `fetch` newest (oldest unless `descending`) rows `keep` keeps.
fn page(keep: impl Fn(&Row) -> bool, fetch: usize, descending: bool) -> Vec<Value> {
    let mut times = rows()
        .into_iter()
        .filter(|row| keep(row))
        .map(|row| row.0)
        .collect::<Vec<_>>();

    times.sort_unstable();

    if descending {
        times.reverse();
    }

    times
        .into_iter()
        .take(fetch)
        .map(|time| json!(time))
        .collect()
}

/// The second column of every row.
fn values(rows: &[Vec<Value>]) -> Vec<Value> {
    rows.iter().map(|row| row[1].clone()).collect()
}

#[tokio::test]
async fn a_page_of_the_newest_or_oldest_rows_is_read_from_the_minutes_the_counts_point_to() {
    let world = world().await;

    // The newest minute holds the page: the older file is not read.
    let (rows, scanned) = run(
        &world,
        "SELECT _timestamp, value FROM logs ORDER BY _timestamp DESC LIMIT 5",
    )
    .await;

    assert_eq!(values(&rows), page(|_| true, 5, true));
    assert_eq!(scanned, 120);

    let (rows, scanned) = run(
        &world,
        "SELECT _timestamp, value FROM logs WHERE level = 'ERROR' AND service = 'a' \
         ORDER BY _timestamp DESC LIMIT 3",
    )
    .await;

    assert_eq!(
        values(&rows),
        page(|row| row.2 == "ERROR" && row.1 == "a", 3, true)
    );
    assert_eq!(scanned, 120);

    let (rows, scanned) = run(
        &world,
        "SELECT _timestamp, value FROM logs ORDER BY _timestamp ASC LIMIT 4",
    )
    .await;

    assert_eq!(values(&rows), page(|_| true, 4, false));
    assert_eq!(scanned, 120);

    // The counts are not kept by the host: both files are read.
    let (rows, scanned) = run(
        &world,
        "SELECT _timestamp, value FROM logs WHERE host = 'h1' ORDER BY _timestamp DESC LIMIT 5",
    )
    .await;

    assert_eq!(values(&rows), page(|row| row.3 == "h1", 5, true));
    assert_eq!(scanned, 240);
}
