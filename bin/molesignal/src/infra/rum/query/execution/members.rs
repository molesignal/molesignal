// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The statements over the keys of JSON columns, the heatmaps and the distributions, run on the
//! engine that answers them in production.

use super::{aggregates::run, *};
use crate::infra::rum::query::{
    AggregateSpec, BarPlan, Column, MeasureSpec, Plan, bars_sql, bounds_sql, clicks_sql, keys_sql,
    people_only, read_bars, read_bounds, read_clicks, read_scroll, scroll_sql,
};

fn flagged(id: &'static str, flags: &'static str, ms: i64) -> Written {
    written(
        100,
        id,
        vec![
            ("view.id", Cell::Text(id)),
            ("view.revision", Cell::Int(1)),
            ("session.id", Cell::Text(id)),
            ("feature_flags", Cell::Text(flags)),
            ("view.loading_time", Cell::Int(ms * 1_000_000)),
        ],
    )
}

fn flagged_views() -> SessionContext {
    context(
        RumStream::Views,
        &[
            flagged("a", r#"{"checkout_v2":"on","new_nav":true}"#, 100),
            flagged("b", r#"{"checkout_v2":"on"}"#, 300),
            flagged("c", r#"{"checkout_v2":"off"}"#, 500),
            written(
                100,
                "d",
                vec![
                    ("view.id", Cell::Text("d")),
                    ("view.revision", Cell::Int(1)),
                    ("session.id", Cell::Text("d")),
                ],
            ),
        ],
    )
}

#[tokio::test]
async fn a_key_of_a_json_column_is_filtered_by_like_a_column() {
    let found = conditions(
        RumStream::Views,
        &[filter("feature_flags.checkout_v2", "=", "on")],
    )
    .unwrap();

    let sql = list_sql(&ListRequest {
        scope: scope(RumStream::Views, &found),
        columns: &["view.id"],
        limit: 10,
        seek: None,
        oldest_first: true,
    });

    let rows = answer(&flagged_views(), &sql).await;

    assert_eq!(rows.len(), 2, "{rows:?}");
}

#[tokio::test]
async fn the_rows_are_told_apart_and_counted_by_a_key_of_a_json_column() {
    let measures = [MeasureSpec {
        function: "count".into(),
        field: None,
        fallback: None,
        filters: Vec::new(),
        name: Some("views".into()),
    }];
    let group_by = ["feature_flags.checkout_v2".to_string()];

    let plan = Plan::new(
        RumStream::Views,
        &AggregateSpec {
            measures: &measures,
            group_by: &group_by,
            bars: None,
            order_by: None,
            descending: None,
            limit: Some(10),
        },
    )
    .unwrap();

    let (columns, rows) = run(
        &flagged_views(),
        &plan.first_sql(&scope(RumStream::Views, &[])),
    )
    .await;
    let aggregate = plan.answer(&columns, &rows).unwrap();

    let counted = aggregate
        .rows
        .iter()
        .map(|row| (row.group[0].clone(), row.values[0].as_u64().unwrap()))
        .collect::<Vec<_>>();

    assert!(
        counted.contains(&(Some("on".to_string()), 2)),
        "{counted:?}"
    );
    assert!(
        counted.contains(&(Some("off".to_string()), 1)),
        "{counted:?}"
    );
}

#[tokio::test]
async fn a_key_of_a_json_column_is_counted_by_value_and_the_keys_are_listed() {
    let column = Column::resolve(RumStream::Views, "feature_flags.checkout_v2")
        .unwrap()
        .unwrap();
    let sql = facet_sql(&scope(RumStream::Views, &[]), &column, 10).unwrap();
    let rows = answer(&flagged_views(), &sql).await;

    assert_eq!(rows[0]["__value"], "on");
    assert_eq!(rows[0]["__count"], "2");

    let sql = keys_sql(&scope(RumStream::Views, &[]), "feature_flags", 10);
    let keys = answer(&flagged_views(), &sql).await;

    assert_eq!(keys[0]["__key"], "checkout_v2");
    assert_eq!(keys[0]["__sessions"], "3");
    assert_eq!(keys[1]["__key"], "new_nav");
}

#[tokio::test]
async fn a_read_of_real_users_leaves_out_what_a_test_drove() {
    let kind = |id: &'static str, kind: Option<&'static str>| {
        let mut cells = vec![
            ("view.id", Cell::Text(id)),
            ("view.revision", Cell::Int(1)),
            ("session.id", Cell::Text(id)),
        ];

        if let Some(kind) = kind {
            cells.push(("session.type", Cell::Text(kind)));
        }

        written(100, id, cells)
    };

    let context = context(
        RumStream::Views,
        &[
            kind("person", Some("user")),
            kind("old-sdk", None),
            kind("check", Some("synthetics")),
            kind("ci", Some("ci_test")),
        ],
    );

    let found = [people_only()];

    let sql = list_sql(&ListRequest {
        scope: scope(RumStream::Views, &found),
        columns: &["view.id"],
        limit: 10,
        seek: None,
        oldest_first: true,
    });

    let mut ids = answer(&context, &sql)
        .await
        .iter()
        .map(|row| row["view.id"].clone())
        .collect::<Vec<_>>();

    ids.sort();

    assert_eq!(ids, ["old-sdk", "person"]);
}

fn click(id: &'static str, session: &'static str, x: i64, y: i64) -> Written {
    written(
        100,
        id,
        vec![
            ("action.id", Cell::Text(id)),
            ("session.id", Cell::Text(session)),
            ("action.type", Cell::Text("click")),
            ("action.target.path", Cell::Text("body>main>button")),
            ("action.target.name", Cell::Text("Pay")),
            ("action.target.width", Cell::Int(100)),
            ("action.target.height", Cell::Int(20)),
            ("action.target.offset.x", Cell::Int(x)),
            ("action.target.offset.y", Cell::Int(y)),
            ("action.page_position.x", Cell::Int(600 + x)),
            ("action.page_position.y", Cell::Int(700 + y)),
        ],
    )
}

#[tokio::test]
async fn the_clicks_of_a_page_are_told_by_element_and_by_the_cell_they_landed_in() {
    let context = context(
        RumStream::Actions,
        &[
            click("1", "s-1", 5, 5),
            click("2", "s-1", 6, 5),
            click("3", "s-2", 95, 15),
        ],
    );

    let (elements, cells) = clicks_sql(&scope(RumStream::Actions, &[]), 10);
    let (element_columns, element_rows) = run(&context, &elements).await;
    let (cell_columns, cell_rows) = run(&context, &cells).await;

    let map = read_clicks(
        (&element_columns, &element_rows),
        (&cell_columns, &cell_rows),
    )
    .unwrap();

    assert_eq!(map.clicks, 3);
    assert_eq!(map.elements.len(), 1);

    let element = &map.elements[0];

    assert_eq!(element.element, "body>main>button");
    assert_eq!(element.clicks, 3);
    assert_eq!(element.sessions, 2);
    assert!(element.cells.contains(&[0, 2, 2]), "{:?}", element.cells);
    assert!(element.cells.contains(&[9, 7, 1]), "{:?}", element.cells);
}

fn depth(id: &'static str, revision: i64, deep: i64) -> Written {
    written(
        100,
        id,
        vec![
            ("view.id", Cell::Text(id)),
            ("view.revision", Cell::Int(revision)),
            ("session.id", Cell::Text(id)),
            ("view.scroll.max_depth", Cell::Int(deep)),
            ("view.scroll.max_scroll_height", Cell::Int(1_000)),
        ],
    )
}

#[tokio::test]
async fn a_view_that_stopped_somewhere_saw_everything_above_it_and_counts_at_its_latest_revision() {
    let context = context(
        RumStream::Views,
        &[
            depth("a", 1, 50),
            depth("a", 2, 250),
            depth("b", 1, 150),
            depth("c", 1, 950),
        ],
    );

    let (columns, rows) = run(&context, &scroll_sql(&scope(RumStream::Views, &[]))).await;
    let map = read_scroll(&columns, &rows).unwrap();

    assert_eq!(map.views, 3);
    assert_eq!(map.page_height, Some(1_000.0));
    assert_eq!(map.bands[0].reached, 1.0);
    assert!(
        (map.bands[2].reached - 2.0 / 3.0).abs() < 1e-9,
        "{:?}",
        map.bands
    );
    assert!(
        (map.bands[9].reached - 1.0 / 3.0).abs() < 1e-9,
        "{:?}",
        map.bands
    );
}

#[tokio::test]
async fn a_number_falls_into_bars_from_its_smallest_value_and_the_last_takes_what_is_far_off() {
    let context = context(
        RumStream::Views,
        &[
            flagged("a", "{}", 100),
            flagged("b", "{}", 100),
            flagged("c", "{}", 200),
            flagged("d", "{}", 300),
        ],
    );

    let column = Column::resolve(RumStream::Views, "view.loading_time")
        .unwrap()
        .unwrap();
    let scoped = scope(RumStream::Views, &[]);

    let (columns, rows) = run(&context, &bounds_sql(&scoped, &column)).await;
    let bounds = read_bounds(&columns, &rows);

    assert_eq!(bounds.rows, 4);
    assert_eq!(bounds.min, Some(100_000_000.0));
    assert_eq!(bounds.max, Some(300_000_000.0));

    let plan = BarPlan::of(&bounds, &column, 2).unwrap();
    let (columns, rows) = run(&context, &bars_sql(&scoped, &column, plan)).await;
    let read = read_bars(bounds, plan, &columns, &rows);

    assert_eq!(read.bars.iter().map(|bar| bar.rows).sum::<u64>(), 4);
    assert_eq!(read.bars[0].rows, 2);
    assert_eq!(read.bars[1].to, 300_000_000.0);
}
