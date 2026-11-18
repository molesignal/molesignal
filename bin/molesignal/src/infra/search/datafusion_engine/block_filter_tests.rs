// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `LIKE` 只读它可能成立的块，经过引擎：段自己的过滤器 Artifact 里有 ngram 字段每块的过滤器，
//! 字面量的三元组不全在一块的过滤器里时这块不读。答案与读整个文件相同。两个段的行同样排布：
//! 较新的段的时间从 [`NEWER`] 起，较旧的从 0 起。

use std::sync::Arc;

use arrow::{
    array::{ArrayRef, RecordBatch, StringArray, TimestampMicrosecondArray},
    datatypes::{DataType, Field, Schema as ArrowSchema, TimeUnit},
};
use async_trait::async_trait;
use domain::{
    query::{QueryEngine, QueryLanguage, QueryRequest, QueryResult, StreamHint},
    storage::{QueryFile, QueryFileSource, primary_dataset_type},
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
use object_store::{ObjectStore, memory::InMemory};
use serde_json::{Value, json};
use settings::ObjectCacheSettings;

use super::DataFusionEngine;
use crate::infra::{
    intake::{BufferPool, dataset_resolver::test_support::test_catalog_and_resolver},
    query::catalog_source::CatalogQuerySource,
    storage::{
        filters::BLOCK_ROWS, manifest::PartitionManifestReader, object_reader::ObjectReader,
        parquet::writer::ParquetWriter,
    },
};

/// 一个段的行数：三整块，再加不满的一块。
const ROWS: usize = 3 * BLOCK_ROWS + 100;

/// 段里第几行含字面量：两行在第二块，一行在最后不满的一块。
const NEEDLES: [usize; 3] = [BLOCK_ROWS + 476, BLOCK_ROWS + 477, 3 * BLOCK_ROWS + 28];

/// 一个段要读的行：第二块与最后一块。
const READ: u64 = (BLOCK_ROWS + ROWS - 3 * BLOCK_ROWS) as u64;

/// 较新的段的第一行（最旧的一行）的时间。
const NEWER: i64 = 1_000_000;

/// 时间从 `start` 起的段里第 `row` 行的时间：文件里的行按时间从新到旧排，批次也照这个次序写。
fn time(start: i64, row: usize) -> i64 {
    start + (ROWS - 1 - row) as i64
}

fn needle(segment: &str, row: usize) -> String {
    format!("request failed: Needle0001 in {segment} at row {row}")
}

struct NoFiles;

#[async_trait]
impl QueryFileSource for NoFiles {
    async fn find(&self, _: &Id, _: &str, _: StreamType, _: TimeRange) -> Result<Vec<QueryFile>> {
        Ok(Vec::new())
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
    let mut message = FieldDef {
        name: "message".into(),
        data_type: FieldType::Utf8,
        nullable: true,
        index_type: StreamIndexType::None,
        encrypted: false,
    };

    message.index_type = StreamIndexType::Ngram;

    StreamDefinition {
        id: Id::new(),
        org_id: Id::from_string("org-a"),
        name: "logs".into(),
        stream_type: StreamType::LOGS,
        schema: Schema {
            fields: vec![message],
        },
        retention: None,
        created_at: TimestampMicros::now(),
        updated_at: TimestampMicros::now(),
    }
}

/// 名为 `segment`、时间从 `start` 起的段：每行早 1 µs，[`NEEDLES`] 的行含 `Needle0001`。
fn batch(segment: &str, start: i64) -> RecordBatch {
    let schema = ArrowSchema::new(vec![
        Field::new(
            "_timestamp",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        ),
        Field::new("message", DataType::Utf8, true),
    ]);

    let messages = (0..ROWS)
        .map(|row| match NEEDLES.contains(&row) {
            true => needle(segment, row),
            false => format!("request processed code={}", row % 7),
        })
        .collect::<Vec<_>>();

    let columns: Vec<ArrayRef> = vec![
        Arc::new(
            TimestampMicrosecondArray::from(
                (0..ROWS).map(|row| time(start, row)).collect::<Vec<_>>(),
            )
            .with_timezone("UTC"),
        ),
        Arc::new(StringArray::from(messages)),
    ];

    RecordBatch::try_new(Arc::new(schema), columns).unwrap()
}

/// 两个段写进 Catalog，引擎从 Catalog 找文件与它们的过滤器 Artifact。
async fn engine() -> DataFusionEngine {
    let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    let stream = definition();
    let (catalog, resolver) = test_catalog_and_resolver();

    let raw = resolver
        .resolve(&stream, primary_dataset_type(stream.stream_type).unwrap())
        .await
        .unwrap();

    let writer = ParquetWriter::new(store.clone());

    for (segment, start) in [("newer", NEWER), ("older", 0)] {
        let segments = writer
            .write_compaction_catalog(&stream, &raw.dataset, batch(segment, start))
            .await
            .unwrap();

        assert_eq!(segments.len(), 1, "the rows fit in one segment");
        catalog.seed_segment(segments.into_iter().next().unwrap());
    }

    let objects =
        ObjectReader::build(store.clone(), "local", &ObjectCacheSettings::default()).unwrap();

    let source = CatalogQuerySource::new(
        catalog,
        Arc::new(BufferPool::new()),
        Arc::new(Streams(stream.clone())),
        objects.clone(),
        Arc::new(PartitionManifestReader::new(objects, 1024 * 1024)),
    );

    DataFusionEngine::new(Arc::new(NoFiles) as Arc<dyn QueryFileSource>, store)
        .with_streams(Arc::new(Streams(stream)))
        .with_catalog_source(Arc::new(source))
}

async fn run(engine: &DataFusionEngine, sql: &str) -> QueryResult {
    let request = QueryRequest {
        org_id: Id::from_string("org-a"),
        language: QueryLanguage::Sql,
        statement: sql.into(),
        time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(NEWER + ROWS as i64)),
        stream: Some(StreamHint {
            name: "logs".into(),
            stream_type: StreamType::LOGS,
        }),
        limit: None,
        step: None,
        federation_clusters: Vec::new(),
    };

    engine
        .execute(request)
        .await
        .unwrap_or_else(|error| panic!("{sql}: {error}"))
}

/// `(count, rows scanned)` of `SELECT count(*)` under `condition`.
async fn count(engine: &DataFusionEngine, condition: &str) -> (Value, u64) {
    let result = run(
        engine,
        &format!("SELECT count(*) AS n FROM logs WHERE {condition}"),
    )
    .await;

    (result.rows[0][0].clone(), result.scanned_rows)
}

#[tokio::test]
async fn a_like_reads_only_the_blocks_that_may_hold_its_literal() {
    let engine = engine().await;

    assert_eq!(
        count(&engine, "message LIKE '%Needle0001%'").await,
        (json!(6), 2 * READ)
    );
    assert_eq!(
        count(&engine, "message ILIKE '%needle0001%'").await,
        (json!(6), 2 * READ)
    );

    // `processed` 在每一块里：OR 的另一支让每块都可能成立，整个文件都读。
    assert_eq!(
        count(
            &engine,
            "message ILIKE '%needle0001%' OR message LIKE '%processed%'"
        )
        .await,
        (json!(2 * ROWS), 2 * ROWS as u64)
    );
}

#[tokio::test]
async fn a_page_by_time_reads_only_those_blocks_too() {
    let engine = engine().await;

    let result = run(
        &engine,
        "SELECT _timestamp, message FROM logs WHERE message ILIKE '%needle0001%' \
         ORDER BY _timestamp DESC LIMIT 2",
    )
    .await;

    let messages = result
        .rows
        .iter()
        .map(|row| row[1].clone())
        .collect::<Vec<_>>();

    // 最新的两行在较新的段的第二块；两个段都只读第二块与最后一块。
    assert_eq!(
        messages,
        [NEEDLES[0], NEEDLES[1]].map(|row| json!(needle("newer", row)))
    );
    assert_eq!(result.scanned_rows, 2 * READ);
}
