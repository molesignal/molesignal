// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The statements of the panels run on the real schema of the views, and come to what the explorer's
//! aggregate comes to on the same rows.

use std::sync::Arc;

use arrow::{
    array::{ArrayRef, Int64Array, RecordBatch, StringArray, TimestampMicrosecondArray},
    datatypes::SchemaRef,
};
use datafusion::{datasource::MemTable, prelude::SessionContext};
use serde_json::{Value, json};

use super::*;
use crate::{
    domain::rum::event::{RumStream, stream_schema},
    infra::{
        query::{
            analyzer::session_context, arrow_json::batches_to_json, current_views::CURRENT_VIEWS,
        },
        rum::query::{BucketPlan, Filter, PanelRole, Scope},
        storage::arrow_schema::{align_batch_to_schema, to_arrow},
    },
};

const HOUR: i64 = 3_600_000_000;

enum Col {
    Time(Vec<i64>),
    Text(Vec<Option<&'static str>>),
    Int(Vec<Option<i64>>),
}

fn text(values: &[&'static str]) -> Col {
    Col::Text(values.iter().map(|value| Some(*value)).collect())
}

/// Seven rows of views: two revisions of the first, a session a check ran, and a view that never
/// said how long it took to load.
fn views() -> (SchemaRef, Vec<RecordBatch>) {
    let target = to_arrow(&stream_schema(RumStream::Views));

    let columns: Vec<(&str, Col)> = vec![
        (
            "_timestamp",
            Col::Time(vec![
                HOUR + 1,
                HOUR + 1,
                HOUR + 2,
                HOUR + 3,
                2 * HOUR + 1,
                2 * HOUR + 2,
                2 * HOUR + 3,
            ]),
        ),
        (
            "_event_id",
            text(&["e0", "e1", "e2", "e3", "e4", "e5", "e6"]),
        ),
        ("application.id", Col::Text(vec![Some("shop"); 7])),
        (
            "session.id",
            text(&["s1", "s1", "s2", "s3", "s4", "s5", "s6"]),
        ),
        (
            "session.type",
            Col::Text(vec![
                Some("user"),
                Some("user"),
                None,
                Some("user"),
                Some("user"),
                Some("synthetics"),
                Some("user"),
            ]),
        ),
        ("view.id", text(&["v1", "v1", "v2", "v3", "v4", "v5", "v6"])),
        (
            "view.revision",
            Col::Int(vec![
                Some(1),
                Some(2),
                Some(1),
                Some(1),
                Some(1),
                Some(1),
                Some(1),
            ]),
        ),
        (
            "view.url_path_group",
            text(&["/", "/", "/cart", "/", "/checkout", "/", "/"]),
        ),
        (
            "browser.name",
            text(&[
                "Chrome", "Chrome", "Chrome", "Safari", "Firefox", "Chrome", "Chrome",
            ]),
        ),
        (
            "view.loading_time",
            Col::Int(vec![
                Some(900_000_000),
                Some(1_200_000_000),
                Some(2_000_000_000),
                Some(3_000_000_000),
                Some(4_000_000_000),
                Some(10_000_000_000),
                None,
            ]),
        ),
    ];

    let arrays = columns.into_iter().map(|(name, column)| {
        let array: ArrayRef = match column {
            Col::Time(values) => {
                Arc::new(TimestampMicrosecondArray::from(values).with_timezone("UTC"))
            }
            Col::Text(values) => Arc::new(StringArray::from(values)),
            Col::Int(values) => Arc::new(Int64Array::from(values)),
        };

        (name, array)
    });

    let batch = RecordBatch::try_from_iter(arrays).unwrap();

    (
        target.clone(),
        vec![align_batch_to_schema(&batch, &target).unwrap()],
    )
}

fn context() -> SessionContext {
    let ctx = session_context(0);
    let (schema, batches) = views();
    let table = Arc::new(MemTable::try_new(schema, vec![batches]).unwrap());

    ctx.register_table(RumStream::Views.name(), table.clone())
        .unwrap();

    for view in CURRENT_VIEWS
        .iter()
        .filter(|view| view.stream == RumStream::Views)
    {
        view.register(&ctx, table.clone()).unwrap();
    }

    ctx
}

async fn run(ctx: &SessionContext, sql: &str) -> (Vec<String>, Vec<Vec<Value>>) {
    let sql = sql.replace("${interval}", "1 hour");

    let batches = ctx
        .sql(&sql)
        .await
        .unwrap_or_else(|error| panic!("{error}\n{sql}"))
        .collect()
        .await
        .unwrap_or_else(|error| panic!("{error}\n{sql}"));

    batches_to_json(&batches)
}

fn selection(filters: Vec<Filter>) -> Selection {
    Selection::new(Some("views"), None, None, filters, Vec::new()).unwrap()
}

fn filter(field: &str, op: &str, value: &str) -> Filter {
    Filter {
        field: field.into(),
        op: op.into(),
        value: value.into(),
        quoted: false,
    }
}

fn measure(function: &str, field: Option<&str>, name: Option<&str>) -> MeasureSpec {
    MeasureSpec {
        function: function.into(),
        field: field.map(str::to_string),
        fallback: None,
        filters: Vec::new(),
        name: name.map(str::to_string),
    }
}

struct Ask<'a> {
    chart: Chart,
    measures: &'a [MeasureSpec],
    group_by: &'a [String],
    group_names: &'a [String],
    limit: Option<usize>,
    field: Option<&'a str>,
    interval: Option<&'a str>,
}

const ASK: Ask<'static> = Ask {
    chart: Chart::Toplist,
    measures: &[],
    group_by: &[],
    group_names: &[],
    limit: None,
    field: None,
    interval: None,
};

fn panel_of(selection: &Selection, ask: &Ask<'_>) -> Result<Panel> {
    panel(&Request {
        selection,
        chart: ask.chart,
        measures: ask.measures,
        group_by: ask.group_by,
        group_names: ask.group_names,
        order_by: None,
        descending: None,
        limit: ask.limit,
        field: ask.field,
        interval: ask.interval,
    })
}

/// What the explorer's aggregate answers for the same question over every row, a group with its
/// values, in the order it ranks them.
async fn aggregate(
    ctx: &SessionContext,
    selection: &Selection,
    measures: &[MeasureSpec],
    group_by: &[String],
    limit: Option<usize>,
) -> Vec<(Vec<Option<String>>, Vec<Value>)> {
    let plan = Plan::stored(
        RumStream::Views,
        &AggregateSpec {
            measures,
            group_by,
            bars: None,
            order_by: None,
            descending: None,
            limit,
        },
    )
    .unwrap();

    let conditions = selection.conditions().unwrap();
    let scope = Scope {
        stream: RumStream::Views,
        from: 0,
        to: i64::MAX,
        conditions: &conditions,
    };

    let (columns, rows) = run(ctx, &plan.first_sql(&scope)).await;

    plan.answer(&columns, &rows)
        .unwrap()
        .rows
        .into_iter()
        .map(|row| (row.group, row.values))
        .collect()
}

#[tokio::test]
async fn a_top_list_ranks_the_groups_as_the_explorer_does() {
    let ctx = context();
    let people = selection(Vec::new());
    let measures = [measure("count", None, Some("视图"))];
    let group_by = ["browser.name".to_string()];
    let names = ["浏览器".to_string()];

    let panel = panel_of(
        &people,
        &Ask {
            measures: &measures,
            group_by: &group_by,
            group_names: &names,
            limit: Some(5),
            ..ASK
        },
    )
    .unwrap();

    assert_eq!(panel.stream, "rum_views");
    assert!(
        panel.statement.contains("FROM \"rum_views_latest\""),
        "{}",
        panel.statement
    );
    assert!(
        !panel.statement.contains("_timestamp"),
        "{}",
        panel.statement
    );
    assert_eq!(panel.limit, 5);

    let (columns, rows) = run(&ctx, &panel.statement).await;

    assert_eq!(columns, ["浏览器", "视图"]);

    // The newest revision of the first view counts once, and the check's session not at all.
    assert_eq!(
        rows,
        vec![
            vec![json!("Chrome"), json!(3)],
            vec![json!("Firefox"), json!(1)],
            vec![json!("Safari"), json!(1)],
        ]
    );

    let explorer = aggregate(&ctx, &people, &measures, &group_by, Some(5)).await;

    assert_eq!(
        explorer
            .iter()
            .map(|(group, values)| vec![json!(group[0]), values[0].clone()])
            .collect::<Vec<_>>(),
        rows
    );
}

#[tokio::test]
async fn the_sessions_a_check_ran_count_when_the_query_asks_for_them() {
    let ctx = context();
    let checks = selection(vec![filter("session.type", "=", "synthetics")]);
    let measures = [measure("count", None, None)];
    let group_by = ["browser.name".to_string()];

    let panel = panel_of(
        &checks,
        &Ask {
            measures: &measures,
            group_by: &group_by,
            ..ASK
        },
    )
    .unwrap();

    let (_, rows) = run(&ctx, &panel.statement).await;

    assert_eq!(rows, vec![vec![json!("Chrome"), json!(1)]]);
}

#[tokio::test]
async fn a_table_says_durations_in_milliseconds() {
    let ctx = context();
    let people = selection(Vec::new());
    let measures = [
        measure("count", None, None),
        measure("max", Some("view.loading_time"), None),
    ];
    let group_by = ["view.url_path_group".to_string()];

    let panel = panel_of(
        &people,
        &Ask {
            chart: Chart::Table,
            measures: &measures,
            group_by: &group_by,
            ..ASK
        },
    )
    .unwrap();

    assert_eq!(
        panel
            .columns
            .iter()
            .map(|column| (column.name.as_str(), column.role, column.unit))
            .collect::<Vec<_>>(),
        [
            ("view.url_path_group", PanelRole::Group, "none"),
            ("count", PanelRole::Measure, "count"),
            ("max(view.loading_time)", PanelRole::Measure, "ms"),
        ]
    );

    let (_, rows) = run(&ctx, &panel.statement).await;
    let explorer = aggregate(&ctx, &people, &measures, &group_by, None).await;

    assert_eq!(rows.len(), explorer.len());

    for (row, (group, values)) in rows.iter().zip(&explorer) {
        assert_eq!(row[0], json!(group[0]));
        assert_eq!(row[1], values[0]);
        assert_eq!(row[2].as_f64(), values[1].as_f64().map(|ns| ns / 1e6));
    }
}

#[tokio::test]
async fn a_chart_over_time_follows_the_groups_that_rank_first() {
    let ctx = context();
    let people = selection(Vec::new());
    let measures = [measure("count", None, None)];
    let group_by = ["browser.name".to_string()];

    let panel = panel_of(
        &people,
        &Ask {
            chart: Chart::Timeseries,
            measures: &measures,
            group_by: &group_by,
            limit: Some(2),
            interval: Some("${interval}"),
            ..ASK
        },
    )
    .unwrap();

    assert!(
        panel
            .statement
            .contains("date_bin(INTERVAL '${interval}', _timestamp) AS \"time\"")
    );
    assert_eq!(panel.columns[0].role, PanelRole::Time);

    let (columns, rows) = run(&ctx, &panel.statement).await;

    assert_eq!(columns, ["time", "browser.name", "count"]);

    // Chrome and Firefox rank first; Safari ties with Firefox and comes after it by name.
    assert_eq!(
        rows,
        vec![
            vec![json!(HOUR), json!("Chrome"), json!(2)],
            vec![json!(2 * HOUR), json!("Chrome"), json!(1)],
            vec![json!(2 * HOUR), json!("Firefox"), json!(1)],
        ]
    );

    // The explorer finds the same groups and follows them through the same bars.
    let plan = Plan::stored(
        RumStream::Views,
        &AggregateSpec {
            measures: &measures,
            group_by: &group_by,
            bars: Some(BucketPlan::new(Some(0), Some(3 * HOUR), Some(HOUR), Some(0)).unwrap()),
            order_by: None,
            descending: None,
            limit: Some(2),
        },
    )
    .unwrap();

    let conditions = people.conditions().unwrap();
    let scope = Scope {
        stream: RumStream::Views,
        from: 0,
        to: 3 * HOUR,
        conditions: &conditions,
    };

    let (columns, first) = run(&ctx, &plan.first_sql(&scope)).await;
    let groups = plan.groups_of(&columns, &first).unwrap();
    let (columns, series) = run(&ctx, &plan.series_sql(&scope, &groups)).await;

    let explorer = plan
        .answer(&columns, &series)
        .unwrap()
        .rows
        .into_iter()
        .map(|row| {
            vec![
                json!(row.start_time),
                json!(row.group[0]),
                row.values[0].clone(),
            ]
        })
        .collect::<Vec<_>>();

    assert_eq!(explorer, rows);
}

#[tokio::test]
async fn a_chart_over_time_of_everything_is_a_line() {
    let ctx = context();
    let measures = [measure("count", None, None)];

    let panel = panel_of(
        &selection(Vec::new()),
        &Ask {
            chart: Chart::Timeseries,
            measures: &measures,
            interval: Some("1 hour"),
            ..ASK
        },
    )
    .unwrap();

    let (_, rows) = run(&ctx, &panel.statement).await;

    assert_eq!(
        rows,
        vec![vec![json!(HOUR), json!(3)], vec![json!(2 * HOUR), json!(2)]]
    );
}

#[tokio::test]
async fn a_distribution_counts_every_value_once_in_bars_named_by_where_they_begin() {
    let ctx = context();
    let measures = [measure("count", None, Some("视图"))];
    let names = ["加载时间".to_string()];

    let panel = panel_of(
        &selection(Vec::new()),
        &Ask {
            chart: Chart::Distribution,
            measures: &measures,
            group_names: &names,
            field: Some("view.loading_time"),
            ..ASK
        },
    )
    .unwrap();

    let (columns, rows) = run(&ctx, &panel.statement).await;

    assert_eq!(columns, ["加载时间", "视图"]);

    // The four views of people that said how long they took, the newest revision of the first.
    let counted: i64 = rows.iter().map(|row| row[1].as_i64().unwrap()).sum();

    assert_eq!(counted, 4);
    assert_eq!(rows[0][0], json!("1200 ms"));
    assert!(
        rows.iter()
            .all(|row| row[0].as_str().unwrap().ends_with(" ms"))
    );
}

#[test]
fn what_a_chart_cannot_be_is_refused() {
    let people = selection(Vec::new());
    let measures = [measure("count", None, None)];
    let two = [
        "browser.name".to_string(),
        "view.url_path_group".to_string(),
    ];

    assert!(
        panel_of(
            &people,
            &Ask {
                measures: &measures,
                ..ASK
            }
        )
        .is_err()
    );
    assert!(
        panel_of(
            &people,
            &Ask {
                measures: &measures,
                group_by: &two,
                ..ASK
            }
        )
        .is_err()
    );
    assert!(
        panel_of(
            &people,
            &Ask {
                chart: Chart::Timeseries,
                measures: &measures,
                ..ASK
            }
        )
        .is_err()
    );
    assert!(
        panel_of(
            &people,
            &Ask {
                chart: Chart::Timeseries,
                measures: &measures,
                interval: Some("1 hour'"),
                ..ASK
            }
        )
        .is_err()
    );
    assert!(
        panel_of(
            &people,
            &Ask {
                chart: Chart::Distribution,
                ..ASK
            }
        )
        .is_err()
    );
    assert!(
        panel_of(
            &people,
            &Ask {
                chart: Chart::Distribution,
                field: Some("view.url_path_group"),
                ..ASK
            }
        )
        .is_err()
    );
}
