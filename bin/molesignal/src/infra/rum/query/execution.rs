// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The statements, run on the engine that answers them in production over tables with the fixed
//! columns of the RUM streams. The text of each is tested beside it; this is where the engine has
//! its say: that it plans them, that only the latest revision of a view or a session is counted,
//! and that every filter means what the explorer says it means.

use std::{collections::HashMap, sync::Arc};

use arrow::{
    array::{
        ArrayRef, BooleanArray, Float64Array, Int64Array, RecordBatch, StringArray,
        TimestampMicrosecondArray,
    },
    datatypes::DataType,
    util::display::array_value_to_string,
};
use datafusion::{datasource::MemTable, prelude::SessionContext};

use super::{
    Column, Filter, ListRequest, Scope, Seek, conditions, facet_sql, histogram_sql, list_sql,
    session_rows_sql, text_columns, text_conditions,
};
use crate::{
    domain::rum::event::{RumStream, field, stream_schema},
    infra::{query::analyzer::session_context, storage::arrow_schema::to_arrow},
    shared::cursor::CursorDirection,
};

mod aggregates;
mod members;

pub(in crate::infra::rum) enum Cell {
    Text(&'static str),
    Int(i64),
    Bool(bool),
}

/// A row as written: the time it is dated at, its event id, and the columns it sets (the others
/// are null).
pub(in crate::infra::rum) struct Written {
    ts: i64,
    id: &'static str,
    cells: Vec<(&'static str, Cell)>,
}

pub(in crate::infra::rum) fn written(
    ts: i64,
    id: &'static str,
    cells: Vec<(&'static str, Cell)>,
) -> Written {
    Written { ts, id, cells }
}

fn cell<'a>(row: &'a Written, name: &str) -> Option<&'a Cell> {
    row.cells
        .iter()
        .find(|(cell, _)| *cell == name)
        .map(|(_, value)| value)
}

pub(in crate::infra::rum) fn table(stream: RumStream, rows: &[Written]) -> MemTable {
    let schema = to_arrow(&stream_schema(stream));

    let columns = schema
        .fields()
        .iter()
        .map(|column| {
            let name = column.name().as_str();

            let array: ArrayRef = match column.data_type() {
                DataType::Timestamp(..) => Arc::new(
                    TimestampMicrosecondArray::from(
                        rows.iter().map(|row| row.ts).collect::<Vec<_>>(),
                    )
                    .with_timezone("UTC"),
                ),
                DataType::Utf8 => Arc::new(StringArray::from(
                    rows.iter()
                        .map(|row| match cell(row, name) {
                            _ if name == "_event_id" => Some(row.id.to_string()),
                            Some(Cell::Text(text)) => Some((*text).to_string()),
                            _ => None,
                        })
                        .collect::<Vec<_>>(),
                )),
                DataType::Int64 => Arc::new(Int64Array::from(
                    rows.iter()
                        .map(|row| match cell(row, name) {
                            Some(Cell::Int(value)) => Some(*value),
                            _ => None,
                        })
                        .collect::<Vec<_>>(),
                )),
                DataType::Float64 => Arc::new(Float64Array::from(
                    rows.iter()
                        .map(|row| match cell(row, name) {
                            Some(Cell::Int(value)) => Some(*value as f64),
                            _ => None,
                        })
                        .collect::<Vec<_>>(),
                )),
                DataType::Boolean => Arc::new(BooleanArray::from(
                    rows.iter()
                        .map(|row| match cell(row, name) {
                            Some(Cell::Bool(value)) => Some(*value),
                            _ => None,
                        })
                        .collect::<Vec<_>>(),
                )),
                other => {
                    panic!("`{name}` has the column type {other:?}, which the test cannot fill")
                }
            };

            array
        })
        .collect::<Vec<_>>();

    let batch = RecordBatch::try_new(schema.clone(), columns).expect("a batch of the schema");

    MemTable::try_new(schema, vec![vec![batch]]).expect("a table of the batch")
}

fn context(stream: RumStream, rows: &[Written]) -> SessionContext {
    let context = session_context(0);

    context
        .register_table(stream.name(), Arc::new(table(stream, rows)))
        .expect("the table is registered once");

    context
}

pub(in crate::infra::rum) type Answer = Vec<HashMap<String, String>>;

/// The rows a statement answers with, every cell as text, by column name.
pub(in crate::infra::rum) async fn answer(context: &SessionContext, sql: &str) -> Answer {
    let batches = context
        .sql(sql)
        .await
        .unwrap_or_else(|error| panic!("{error}\n{sql}"))
        .collect()
        .await
        .unwrap_or_else(|error| panic!("{error}\n{sql}"));

    let mut rows = Vec::new();

    for batch in &batches {
        for index in 0..batch.num_rows() {
            rows.push(
                batch
                    .schema()
                    .fields()
                    .iter()
                    .enumerate()
                    .map(|(column, field)| {
                        let value = array_value_to_string(batch.column(column), index)
                            .expect("a cell is text");

                        (field.name().clone(), value)
                    })
                    .collect(),
            );
        }
    }

    rows
}

fn scope(stream: RumStream, conditions: &[super::Condition]) -> Scope<'_> {
    Scope {
        stream,
        from: 0,
        to: 1_000_000,
        conditions,
    }
}

fn filter(field: &str, op: &str, value: &str) -> Filter {
    Filter {
        field: field.into(),
        op: op.into(),
        value: value.into(),
        quoted: false,
    }
}

fn view(id: &'static str, view: &'static str, revision: i64, ts: i64, errors: i64) -> Written {
    written(
        ts,
        id,
        vec![
            ("view.id", Cell::Text(view)),
            ("view.revision", Cell::Int(revision)),
            ("view.error.count", Cell::Int(errors)),
            ("view.is_active", Cell::Bool(revision < 2)),
            ("session.id", Cell::Text("s-1")),
        ],
    )
}

fn resource(
    id: &'static str,
    ts: i64,
    kind: &'static str,
    url: &'static str,
    status: Option<i64>,
) -> Written {
    let mut cells = vec![
        ("resource.type", Cell::Text(kind)),
        ("resource.url", Cell::Text(url)),
    ];

    if let Some(status) = status {
        cells.push(("resource.status_code", Cell::Int(status)));
    }

    written(ts, id, cells)
}

fn resources() -> SessionContext {
    context(
        RumStream::Resources,
        &[
            resource(
                "r1",
                1_000,
                "fetch",
                "https://shop.example.com/api/cart",
                Some(200),
            ),
            resource(
                "r2",
                2_000,
                "fetch",
                "https://shop.example.com/api/pay",
                Some(404),
            ),
            resource(
                "r3",
                3_000,
                "xhr",
                "https://shop.example.com/api/pay",
                Some(500),
            ),
            resource(
                "r4",
                4_000,
                "image",
                "https://cdn.example.com/hero.webp",
                None,
            ),
            resource(
                "r5",
                5_000,
                "fetch",
                "https://shop.example.com/api/ping",
                Some(0),
            ),
        ],
    )
}

async fn matching(context: &SessionContext, filters: &[Filter]) -> Vec<String> {
    let conditions = conditions(RumStream::Resources, filters).expect("conditions");

    let sql = list_sql(&ListRequest {
        scope: scope(RumStream::Resources, &conditions),
        columns: &["resource.url"],
        limit: 50,
        seek: None,
        oldest_first: true,
    });

    answer(context, &sql)
        .await
        .into_iter()
        .map(|row| row["__id"].clone())
        .collect()
}

#[tokio::test]
async fn a_view_is_listed_once_in_its_latest_revision() {
    let context = context(
        RumStream::Views,
        &[
            view("e1", "v-1", 1, 1_000, 0),
            view("e2", "v-1", 2, 1_000, 2),
            view("e3", "v-2", 1, 2_000, 1),
        ],
    );

    let sql = list_sql(&ListRequest {
        scope: scope(RumStream::Views, &[]),
        columns: &["view.id", "view.revision", "view.error.count"],
        limit: 10,
        seek: None,
        oldest_first: false,
    });

    let rows = answer(&context, &sql).await;

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["view.id"], "v-2");
    assert_eq!(rows[1]["view.id"], "v-1");
    assert_eq!(rows[1]["view.revision"], "2");
    assert_eq!(rows[1]["view.error.count"], "2");
    assert_eq!(rows[1]["__id"], "v-1");
    assert_eq!(rows[1]["__ts"], "1000");
}

#[tokio::test]
async fn what_a_view_is_now_decides_not_what_it_was() {
    let context = context(
        RumStream::Views,
        &[
            view("e1", "v-1", 1, 1_000, 0),
            view("e2", "v-1", 2, 1_000, 2),
            view("e3", "v-2", 1, 2_000, 1),
        ],
    );

    let asked = |filters: Vec<Filter>| {
        let conditions = conditions(RumStream::Views, &filters).expect("conditions");

        list_sql(&ListRequest {
            scope: scope(RumStream::Views, &conditions),
            columns: &["view.id"],
            limit: 10,
            seek: None,
            oldest_first: true,
        })
    };

    // The first revision of v-1 had no errors; the latest has two.
    let sql = asked(vec![filter("view.error.count", "=", "0")]);
    assert!(answer(&context, &sql).await.is_empty());

    let sql = asked(vec![filter("view.error.count", ">", "0")]);
    assert_eq!(answer(&context, &sql).await.len(), 2);

    // The first revision of v-1 was active; the latest is not.
    let sql = asked(vec![filter("view.is_active", "=", "true")]);
    assert_eq!(answer(&context, &sql).await.len(), 1);

    // An identity is asked before the latest is taken, and still finds the latest.
    let sql = asked(vec![filter("view.id", "=", "v-1")]);
    let rows = answer(&context, &sql).await;

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["__id"], "v-1");

    let sql = asked(vec![filter("view.id", "in", r#"["v-1","v-2"]"#)]);
    assert_eq!(answer(&context, &sql).await.len(), 2);
}

#[tokio::test]
async fn a_list_is_read_page_by_page() {
    let context = resources();

    let page = |seek: Option<Seek>| {
        list_sql(&ListRequest {
            scope: scope(RumStream::Resources, &[]),
            columns: &["resource.url"],
            limit: 2,
            seek,
            oldest_first: false,
        })
    };

    let first = answer(&context, &page(None)).await;

    assert_eq!(
        first
            .iter()
            .map(|row| row["__id"].as_str())
            .collect::<Vec<_>>(),
        ["r5", "r4"]
    );

    let second = answer(
        &context,
        &page(Some(Seek {
            direction: CursorDirection::After,
            ts: 4_000,
            tie: "r4".into(),
        })),
    )
    .await;

    assert_eq!(
        second
            .iter()
            .map(|row| row["__id"].as_str())
            .collect::<Vec<_>>(),
        ["r3", "r2"]
    );

    // The page before a cursor is read the other way round; the caller turns it round.
    let before = answer(
        &context,
        &page(Some(Seek {
            direction: CursorDirection::Before,
            ts: 3_000,
            tie: "r3".into(),
        })),
    )
    .await;

    assert_eq!(
        before
            .iter()
            .map(|row| row["__id"].as_str())
            .collect::<Vec<_>>(),
        ["r4", "r5"]
    );
}

#[tokio::test]
async fn every_operator_means_what_the_explorer_says() {
    let context = resources();

    assert_eq!(
        matching(&context, &[filter("resource.status_code", ">=", "400")]).await,
        ["r2", "r3"]
    );
    assert_eq!(
        matching(&context, &[filter("resource.status_code", "=", "200")]).await,
        ["r1"]
    );
    assert_eq!(
        matching(&context, &[filter("resource.status_code", "!=", "200")]).await,
        ["r2", "r3", "r4", "r5"]
    );
    assert_eq!(
        matching(&context, &[filter("resource.status_code", "exists", "")]).await,
        ["r1", "r2", "r3", "r5"]
    );
    assert_eq!(
        matching(&context, &[filter("resource.status_code", "missing", "")]).await,
        ["r4"]
    );
    assert_eq!(
        matching(&context, &[filter("resource.status_code", "<", "300")]).await,
        ["r1", "r5"]
    );

    assert_eq!(
        matching(
            &context,
            &[filter("resource.type", "in", r#"["fetch","xhr"]"#)]
        )
        .await,
        ["r1", "r2", "r3", "r5"]
    );
    assert_eq!(
        matching(
            &context,
            &[filter("resource.type", "not_in", r#"["fetch"]"#)]
        )
        .await,
        ["r3", "r4"]
    );
    assert_eq!(
        matching(&context, &[filter("resource.type", "=", "f*")]).await,
        ["r1", "r2", "r5"]
    );
    assert_eq!(
        matching(&context, &[filter("resource.type", "!=", "f*")]).await,
        ["r3", "r4"]
    );
    assert_eq!(
        matching(&context, &[filter("resource.url", "contains", "API/PAY")]).await,
        ["r2", "r3"]
    );

    // An empty value is no value.
    assert_eq!(
        matching(&context, &[filter("resource.status_code", "=", "")]).await,
        ["r4"]
    );
    assert_eq!(
        matching(&context, &[filter("resource.type", "!=", "")]).await,
        ["r1", "r2", "r3", "r4", "r5"]
    );

    // Every filter has to be met.
    assert_eq!(
        matching(
            &context,
            &[
                filter("resource.type", "=", "fetch"),
                filter("resource.status_code", ">=", "400"),
            ],
        )
        .await,
        ["r2"]
    );
}

#[tokio::test]
async fn words_are_found_in_the_text_columns() {
    let context = resources();

    let found = |words: &[&str]| {
        let words = words
            .iter()
            .map(|word| (*word).to_string())
            .collect::<Vec<_>>();
        let conditions =
            text_conditions(text_columns(RumStream::Resources), &words).expect("conditions");

        list_sql(&ListRequest {
            scope: scope(RumStream::Resources, &conditions),
            columns: &["resource.url"],
            limit: 50,
            seek: None,
            oldest_first: true,
        })
    };

    let ids = |rows: Answer| {
        rows.into_iter()
            .map(|row| row["__id"].clone())
            .collect::<Vec<_>>()
    };

    assert_eq!(ids(answer(&context, &found(&["HERO"])).await), ["r4"]);
    assert_eq!(
        ids(answer(&context, &found(&["api", "pay"])).await),
        ["r2", "r3"]
    );
    assert!(ids(answer(&context, &found(&["no such text"])).await).is_empty());
    // A wildcard of LIKE is a character of the text here.
    assert!(ids(answer(&context, &found(&["%"])).await).is_empty());
}

#[tokio::test]
async fn the_commonest_values_come_first() {
    let context = resources();
    let column = Column::Stored(field(RumStream::Resources, "resource.type").expect("a column"));

    let sql = facet_sql(&scope(RumStream::Resources, &[]), &column, 10).expect("a statement");
    let rows = answer(&context, &sql).await;

    let values = rows
        .iter()
        .map(|row| (row["__value"].as_str(), row["__count"].as_str()))
        .collect::<Vec<_>>();

    assert_eq!(values, [("fetch", "3"), ("image", "1"), ("xhr", "1")]);

    let limited = facet_sql(&scope(RumStream::Resources, &[]), &column, 1).expect("a statement");

    assert_eq!(answer(&context, &limited).await.len(), 1);

    let numbers =
        Column::Stored(field(RumStream::Resources, "resource.status_code").expect("a column"));
    let sql = facet_sql(&scope(RumStream::Resources, &[]), &numbers, 10).expect("a statement");

    assert_eq!(answer(&context, &sql).await.len(), 4);
}

#[tokio::test]
async fn bars_count_the_rows_and_the_troubled_among_them() {
    let context = resources();

    let sql = histogram_sql(&scope(RumStream::Resources, &[]), 0, 2_000);
    let mut bars = answer(&context, &sql)
        .await
        .into_iter()
        .map(|row| {
            (
                row["__histogram_bucket"].parse::<i64>().expect("a bucket"),
                row["__histogram_total"].clone(),
                row["__histogram_errors"].clone(),
            )
        })
        .collect::<Vec<_>>();

    bars.sort();

    assert_eq!(
        bars,
        [
            (0, "1".to_string(), "0".to_string()),
            (1, "2".to_string(), "2".to_string()),
            (2, "2".to_string(), "1".to_string()),
        ]
    );
}

#[tokio::test]
async fn a_view_is_counted_in_a_bar_once() {
    let context = context(
        RumStream::Views,
        &[
            view("e1", "v-1", 1, 1_000, 0),
            view("e2", "v-1", 2, 1_000, 2),
            view("e3", "v-2", 1, 3_000, 0),
        ],
    );

    let sql = histogram_sql(&scope(RumStream::Views, &[]), 0, 2_000);
    let mut bars = answer(&context, &sql)
        .await
        .into_iter()
        .map(|row| {
            (
                row["__histogram_bucket"].parse::<i64>().expect("a bucket"),
                row["__histogram_total"].clone(),
                row["__histogram_errors"].clone(),
            )
        })
        .collect::<Vec<_>>();

    bars.sort();

    assert_eq!(
        bars,
        [
            (0, "1".to_string(), "1".to_string()),
            (1, "1".to_string(), "0".to_string())
        ]
    );
}

#[tokio::test]
async fn the_rows_written_for_a_session_come_back_newest_revision_first() {
    let session = |id: &'static str, revision: i64, other: &'static str| {
        written(
            1_000,
            id,
            vec![
                ("session.id", Cell::Text(other)),
                ("session.revision", Cell::Int(revision)),
                ("session.view_state", Cell::Text("{}")),
            ],
        )
    };

    let context = context(
        RumStream::Sessions,
        &[
            session("e1", 5, "s-1"),
            session("e2", 7, "s-1"),
            session("e3", 9, "s-2"),
            session("e4", 6, "s-1"),
        ],
    );

    let rows = answer(&context, &session_rows_sql("s-1", 0, 2_000, 8)).await;

    let revisions = rows
        .iter()
        .map(|row| row["session.revision"].as_str())
        .collect::<Vec<_>>();

    assert_eq!(revisions, ["7", "6", "5"]);
    assert_eq!(rows[0]["session.view_state"], "{}");

    // The window is of the date the rows are written at.
    assert!(
        answer(&context, &session_rows_sql("s-1", 2_000, 3_000, 8))
            .await
            .is_empty()
    );
}
