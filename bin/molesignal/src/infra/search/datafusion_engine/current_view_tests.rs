// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `rum_views_latest` and `rum_sessions_latest` through the engine: files written one revision at a
//! time, the request's time range, and the statements that must not prune what they read.

use std::sync::Arc;

use arrow::{
    array::{ArrayRef, Int64Array, RecordBatch, StringArray, TimestampMicrosecondArray},
    datatypes::{DataType, Field, Schema as ArrowSchema, TimeUnit},
};
use async_trait::async_trait;
use object_store::{ObjectStore, local::LocalFileSystem};
use parking_lot::Mutex;
use serde_json::{Value, json};

use super::DataFusionEngine;
use crate::{
    domain::{
        query::{QueryEngine, QueryLanguage, QueryRequest, QueryResult, StreamHint},
        storage::{
            ArtifactRole, ArtifactState, DatasetState, PhysicalDataset, PhysicalDatasetId,
            QueryFile, QueryFileSource, builtin_registry, primary_dataset_type, type_id,
        },
        stream::{
            FieldDef, FieldType, Retention, Schema, StreamDefinition, StreamRepository, StreamType,
        },
    },
    infra::storage::parquet::writer::ParquetWriter,
    shared::{
        Error, Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

const HOUR: i64 = 3_600_000_000;

#[derive(Default)]
struct Files(Mutex<Vec<QueryFile>>);

#[async_trait]
impl QueryFileSource for Files {
    async fn find(
        &self,
        org: &Id,
        stream: &str,
        stream_type: StreamType,
        range: TimeRange,
    ) -> Result<Vec<QueryFile>> {
        Ok(self
            .0
            .lock()
            .iter()
            .filter(|file| {
                &file.org_id == org
                    && file.stream == stream
                    && file.stream_type == stream_type
                    && file.time_range.end.0 >= range.start.0
                    && file.time_range.start.0 <= range.end.0
            })
            .cloned()
            .collect())
    }
}

struct Streams(Vec<StreamDefinition>);

#[async_trait]
impl StreamRepository for Streams {
    async fn create(&self, definition: StreamDefinition) -> Result<StreamDefinition> {
        Ok(definition)
    }

    async fn update_schema(&self, _: &Id, _: Schema) -> Result<()> {
        Ok(())
    }

    async fn get(&self, org: &Id, name: &str, stream_type: StreamType) -> Result<StreamDefinition> {
        self.0
            .iter()
            .find(|definition| {
                &definition.org_id == org
                    && definition.name == name
                    && definition.stream_type == stream_type
            })
            .cloned()
            .ok_or_else(|| Error::not_found("stream"))
    }

    async fn list(&self, org: &Id) -> Result<Vec<StreamDefinition>> {
        Ok(self
            .0
            .iter()
            .filter(|definition| &definition.org_id == org)
            .cloned()
            .collect())
    }

    async fn delete(&self, _: &Id) -> Result<()> {
        Ok(())
    }
}

fn definition(org: &str, name: &str, key: &str, revision: &str, value: &str) -> StreamDefinition {
    let field = |name: &str, data_type| FieldDef {
        name: name.into(),
        data_type,
        nullable: true,
        index_type: None,
        indexed: false,
        encrypted: false,
        exact: false,
    };

    StreamDefinition {
        id: Id::new(),
        org_id: Id::from_string(org),
        name: name.into(),
        stream_type: StreamType::LOGS,
        schema: Schema {
            fields: vec![
                field("_event_id", FieldType::Utf8),
                field("application.id", FieldType::Utf8),
                field(key, FieldType::Utf8),
                field(revision, FieldType::Int64),
                field(value, FieldType::Int64),
            ],
        },
        retention: Some(Retention { days: 7 }),
        created_at: TimestampMicros::now(),
        updated_at: TimestampMicros::now(),
    }
}

/// One row of a stream: when, which application, which entity, its revision and a figure.
struct Row {
    at: i64,
    application: &'static str,
    entity: &'static str,
    revision: i64,
    value: i64,
}

const fn row(
    at: i64,
    application: &'static str,
    entity: &'static str,
    revision: i64,
    value: i64,
) -> Row {
    Row {
        at,
        application,
        entity,
        revision,
        value,
    }
}

fn batch(definition: &StreamDefinition, rows: &[Row]) -> RecordBatch {
    let fields = definition
        .schema
        .fields
        .iter()
        .map(|field| match field.data_type {
            FieldType::Int64 => Field::new(&field.name, DataType::Int64, true),
            _ => Field::new(&field.name, DataType::Utf8, true),
        })
        .collect::<Vec<_>>();

    let mut schema_fields = vec![Field::new(
        "_timestamp",
        DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
        false,
    )];

    schema_fields.extend(fields);

    let columns: Vec<ArrayRef> = vec![
        Arc::new(
            TimestampMicrosecondArray::from(rows.iter().map(|r| r.at).collect::<Vec<_>>())
                .with_timezone("UTC"),
        ),
        Arc::new(StringArray::from(
            (0..rows.len())
                .map(|index| format!("event-{}-{index}", rows[index].revision))
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            rows.iter().map(|r| r.application).collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            rows.iter().map(|r| r.entity).collect::<Vec<_>>(),
        )),
        Arc::new(Int64Array::from(
            rows.iter().map(|r| r.revision).collect::<Vec<_>>(),
        )),
        Arc::new(Int64Array::from(
            rows.iter().map(|r| r.value).collect::<Vec<_>>(),
        )),
    ];

    RecordBatch::try_new(Arc::new(ArrowSchema::new(schema_fields)), columns).unwrap()
}

async fn write(writer: &ParquetWriter, stream: &StreamDefinition, batch: RecordBatch) -> QueryFile {
    let dataset_type = primary_dataset_type(stream.stream_type).unwrap();
    let registry = builtin_registry();

    let spec = registry
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

    assert_eq!(segments.len(), 1, "a fixture batch fits one partition");

    let segment = segments.remove(0);

    assert_eq!(segment.primary.role, ArtifactRole::PrimaryData);
    assert_eq!(segment.primary.state, ArtifactState::Ready);
    assert_eq!(
        segment.primary.artifact_type.as_str(),
        type_id::builtin::ARTIFACT_PARQUET
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
    }
}

struct World {
    engine: DataFusionEngine,
    _directory: tempfile::TempDir,
}

/// Two organizations; the first has views written as five files, one revision (or entity) each,
/// and sessions as two.
async fn world() -> World {
    let directory = tempfile::tempdir().unwrap();
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFileSystem::new_with_prefix(directory.path()).unwrap());

    let writer = ParquetWriter::new(store.clone());
    let files = Arc::new(Files::default());

    let views = definition(
        "org-a",
        "rum_views",
        "view.id",
        "view.revision",
        "view.loading_time",
    );
    let sessions = definition(
        "org-a",
        "rum_sessions",
        "session.id",
        "session.revision",
        "session.error.count",
    );
    let other = definition(
        "org-b",
        "rum_views",
        "view.id",
        "view.revision",
        "view.loading_time",
    );

    let t = HOUR;

    // `v1` is reported three times, each revision in a file of its own, as flushes come. The
    // figure of a revision is not always lower than that of the next.
    let written = [
        (
            &views,
            vec![
                row(t + 1, "a", "v1", 1, 100),
                row(t + 2, "a", "v2", 1, 50),
                row(t + 3, "b", "v9", 1, 999),
            ],
        ),
        (&views, vec![row(t + 1, "a", "v1", 2, 200)]),
        (&views, vec![row(t + 1, "a", "v1", 3, 300)]),
        (&views, vec![row(3 * HOUR + 5, "a", "v3", 1, 7)]),
        (&other, vec![row(t + 1, "a", "v1", 1, 11)]),
        (
            &sessions,
            vec![row(t + 1, "a", "s1", 1, 0), row(t + 2, "a", "s2", 1, 0)],
        ),
        (&sessions, vec![row(t + 1, "a", "s1", 2, 4)]),
    ];

    for (stream, rows) in written {
        let file = write(&writer, stream, batch(stream, &rows)).await;

        files.0.lock().push(file);
    }

    let engine = DataFusionEngine::new(files as Arc<dyn QueryFileSource>, store)
        .with_streams(Arc::new(Streams(vec![views, sessions, other])));

    World {
        engine,
        _directory: directory,
    }
}

fn request(sql: &str, hint: &str, range: TimeRange) -> QueryRequest {
    QueryRequest {
        org_id: Id::from_string("org-a"),
        language: QueryLanguage::Sql,
        statement: sql.into(),
        time_range: range,
        stream: Some(StreamHint {
            name: hint.into(),
            stream_type: StreamType::LOGS,
        }),
        limit: None,
        step: None,
        federation_clusters: Vec::new(),
    }
}

fn everything() -> TimeRange {
    TimeRange::new(TimestampMicros(0), TimestampMicros(i64::MAX))
}

async fn rows(world: &World, sql: &str, hint: &str, range: TimeRange) -> Vec<Vec<Value>> {
    let result: QueryResult = world
        .engine
        .execute(request(sql, hint, range))
        .await
        .unwrap_or_else(|error| panic!("{sql}: {error}"));

    result.rows
}

const ENTITIES: &str = r#"SELECT "view.id", "view.loading_time" FROM rum_views_latest WHERE "application.id" = 'a' ORDER BY "view.id""#;

#[tokio::test]
async fn an_entity_is_read_at_its_newest_revision_across_the_files_it_was_written_in() {
    let world = world().await;

    assert_eq!(
        rows(&world, ENTITIES, "rum_views", everything()).await,
        vec![
            vec![json!("v1"), json!(300)],
            vec![json!("v2"), json!(50)],
            vec![json!("v3"), json!(7)],
        ]
    );
}

#[tokio::test]
async fn the_time_range_of_the_request_bounds_a_statement_that_names_no_time() {
    let world = world().await;

    // The third hour holds `v3` only; a range that ends before it must not see it.
    let first_two_hours = TimeRange::new(TimestampMicros(0), TimestampMicros(3 * HOUR));

    assert_eq!(
        rows(&world, ENTITIES, "rum_views", first_two_hours).await,
        vec![vec![json!("v1"), json!(300)], vec![json!("v2"), json!(50)]]
    );

    let only_the_third = TimeRange::new(TimestampMicros(3 * HOUR), TimestampMicros(4 * HOUR));

    assert_eq!(
        rows(&world, ENTITIES, "rum_views", only_the_third).await,
        vec![vec![json!("v3"), json!(7)]]
    );
}

#[tokio::test]
async fn a_hint_that_names_the_view_is_the_stream_behind_it() {
    let world = world().await;

    assert_eq!(
        rows(&world, ENTITIES, "rum_views_latest", everything()).await,
        rows(&world, ENTITIES, "rum_views", everything()).await
    );
}

// The files an older revision is in may say a row matches when the newest does not, and the other
// way round: a statement over the current versions must read every file of the time range.
#[tokio::test]
async fn what_the_newest_revision_says_decides_not_what_an_older_one_said() {
    let world = world().await;

    // `v1` was 100 and 200 before it was 300: it is not under 250.
    let under = r#"SELECT "view.id" FROM rum_views_latest WHERE "application.id" = 'a' AND "view.loading_time" < 250 ORDER BY 1"#;

    assert_eq!(
        rows(&world, under, "rum_views", everything()).await,
        vec![vec![json!("v2")], vec![json!("v3")]]
    );

    // And it is over 250 although the first revision was not.
    let over = r#"SELECT "view.id" FROM rum_views_latest WHERE "application.id" = 'a' AND "view.loading_time" > 250"#;

    assert_eq!(
        rows(&world, over, "rum_views", everything()).await,
        vec![vec![json!("v1")]]
    );
}

#[tokio::test]
async fn a_count_is_of_entities_and_the_stream_itself_still_holds_every_revision() {
    let world = world().await;

    assert_eq!(
        rows(
            &world,
            "SELECT count(*) FROM rum_views_latest",
            "rum_views",
            everything()
        )
        .await,
        vec![vec![json!(4)]]
    );

    assert_eq!(
        rows(
            &world,
            "SELECT count(*) FROM rum_views",
            "rum_views",
            everything()
        )
        .await,
        vec![vec![json!(6)]]
    );
}

#[tokio::test]
async fn a_view_of_another_stream_is_read_beside_the_hinted_one() {
    let world = world().await;

    let sql = r#"SELECT v."view.id", s."session.error.count" FROM rum_views_latest v JOIN rum_sessions_latest s ON s."session.id" = 's1' WHERE v."view.id" = 'v2'"#;

    assert_eq!(
        rows(&world, sql, "rum_views", everything()).await,
        vec![vec![json!("v2"), json!(4)]]
    );

    // Hinted at the sessions, a statement on views reads the same.
    assert_eq!(
        rows(&world, ENTITIES, "rum_sessions", everything()).await,
        rows(&world, ENTITIES, "rum_views", everything()).await
    );
}

#[tokio::test]
async fn the_view_of_an_organization_holds_its_own_rows() {
    let world = world().await;

    let mut other = request(ENTITIES, "rum_views", everything());
    other.org_id = Id::from_string("org-b");

    let result = world.engine.execute(other).await.unwrap();

    assert_eq!(result.rows, vec![vec![json!("v1"), json!(11)]]);
}
