// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The current-version views of the versioned RUM streams.
//!
//! The rows of a view or a session are written again as the entity changes, one per revision, and
//! only the highest revision of each counts. `rum_views_latest` and `rum_sessions_latest` are what
//! SQL reads to see the entities as they stand: each is the stream it is named after, reduced to
//! the newest revision of every entity.
//!
//! A view is built over the table the engine registered for the query, so it reads what the time
//! range of the request and the masking of the caller allow, and no more. The engine registers it
//! when the statement names it.
//!
//! Anything that prunes files by what a statement asks must stay away from a stream that is read
//! through a view: a row that does not match may be the newest revision of an entity whose older
//! revision does.

use std::sync::Arc;

use datafusion::{
    catalog::TableProvider,
    common::Column,
    functions_window::expr_fn::row_number,
    logical_expr::{Expr, ExprFunctionExt, lit},
    prelude::SessionContext,
};

use crate::{
    domain::rum::event::RumStream,
    shared::{Error, Result},
};

/// The column the rank of a revision is kept in while the newest is picked; the view does not
/// expose it.
const RANK: &str = "__rn";

/// A stream that is read through its newest revisions, and the name SQL asks for them by.
#[derive(Debug, Clone, Copy)]
pub struct CurrentView {
    pub name: &'static str,
    pub stream: RumStream,
}

pub static CURRENT_VIEWS: [CurrentView; 2] = [
    CurrentView {
        name: "rum_views_latest",
        stream: RumStream::Views,
    },
    CurrentView {
        name: "rum_sessions_latest",
        stream: RumStream::Sessions,
    },
];

/// The views a statement mentions, by name and whatever the case.
pub fn mentioned(sql: &str) -> Vec<&'static CurrentView> {
    let lowered = sql.to_ascii_lowercase();

    CURRENT_VIEWS
        .iter()
        .filter(|view| lowered.contains(view.name))
        .collect()
}

/// The stream a view name stands for, whatever the case; `None` for any other name.
pub fn base_stream(name: &str) -> Option<&'static str> {
    CURRENT_VIEWS
        .iter()
        .find(|view| view.name.eq_ignore_ascii_case(name))
        .map(CurrentView::base)
}

impl CurrentView {
    /// The stream the view is made of, which is also the name of its table in a query.
    pub fn base(&self) -> &'static str {
        self.stream.name()
    }

    /// Registers the view in `ctx`, over `table`, the table registered for its stream. A view
    /// holds the table it was made over: register it again whenever the table is.
    pub fn register(&self, ctx: &SessionContext, table: Arc<dyn TableProvider>) -> Result<()> {
        let versioned = self.stream.versioned().ok_or_else(|| {
            Error::internal(format!(
                "`{}` is made of a stream without revisions",
                self.name
            ))
        })?;

        let failed = |error: datafusion::error::DataFusionError| {
            Error::internal(format!("current view `{}`: {error}", self.name))
        };

        let frame = ctx.read_table(table).map_err(failed)?;
        let schema = frame.schema().clone();

        let column = |name: &str| -> Option<Expr> {
            schema
                .qualified_field_with_unqualified_name(name)
                .ok()
                .map(|field| Expr::Column(Column::from(field)))
        };

        let required = |name: &str| {
            column(name).ok_or_else(|| {
                Error::invalid(format!(
                    "`{}` needs the column `{name}` of `{}`",
                    self.name,
                    self.base()
                ))
            })
        };

        // What names an entity never changes between its revisions. Keeping the application in the
        // partition lets a filter on it reach the files before the newest revision is picked.
        let mut partition = vec![required("_timestamp")?];
        partition.extend(column("application.id"));
        partition.push(required(versioned.key)?);

        // A row without a revision comes last, and the later event wins between equal revisions.
        let mut order = vec![required(versioned.revision)?.sort(false, false)];
        order.extend(column("_event_id").map(|event| event.sort(false, false)));

        let rank = row_number()
            .partition_by(partition)
            .order_by(order)
            .build()
            .map_err(failed)?
            .alias(RANK);

        let kept: Vec<Expr> = schema.columns().into_iter().map(Expr::Column).collect();

        let view = frame
            .window(vec![rank])
            .map_err(failed)?
            .filter(Expr::Column(Column::from_name(RANK)).eq(lit(1_u64)))
            .map_err(failed)?
            .select(kept)
            .map_err(failed)?;

        ctx.deregister_table(self.name).map_err(failed)?;
        ctx.register_table(self.name, view.into_view())
            .map_err(failed)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use arrow::{
        array::{ArrayRef, Int64Array, RecordBatch, StringArray, TimestampMicrosecondArray},
        datatypes::{DataType, Field, Schema, SchemaRef, TimeUnit},
    };
    use datafusion::datasource::MemTable;

    use super::*;
    use crate::infra::query::analyzer::session_context;

    struct Row {
        time: i64,
        application: &'static str,
        view: &'static str,
        revision: Option<i64>,
        event: &'static str,
        loading: i64,
    }

    const fn row(
        time: i64,
        application: &'static str,
        view: &'static str,
        revision: Option<i64>,
        event: &'static str,
        loading: i64,
    ) -> Row {
        Row {
            time,
            application,
            view,
            revision,
            event,
            loading,
        }
    }

    fn schema() -> SchemaRef {
        Arc::new(Schema::new(vec![
            Field::new(
                "_timestamp",
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
            Field::new("_event_id", DataType::Utf8, true),
            Field::new("application.id", DataType::Utf8, false),
            Field::new("view.id", DataType::Utf8, false),
            Field::new("view.revision", DataType::Int64, true),
            Field::new("view.loading_time", DataType::Int64, true),
        ]))
    }

    fn context(rows: &[Row]) -> SessionContext {
        let columns: Vec<ArrayRef> = vec![
            Arc::new(
                TimestampMicrosecondArray::from(rows.iter().map(|r| r.time).collect::<Vec<_>>())
                    .with_timezone("UTC"),
            ),
            Arc::new(StringArray::from(
                rows.iter().map(|r| r.event).collect::<Vec<_>>(),
            )),
            Arc::new(StringArray::from(
                rows.iter().map(|r| r.application).collect::<Vec<_>>(),
            )),
            Arc::new(StringArray::from(
                rows.iter().map(|r| r.view).collect::<Vec<_>>(),
            )),
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.revision).collect::<Vec<_>>(),
            )),
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.loading).collect::<Vec<_>>(),
            )),
        ];

        let batch = RecordBatch::try_new(schema(), columns).unwrap();
        let table = Arc::new(MemTable::try_new(schema(), vec![vec![batch]]).unwrap());
        let ctx = session_context(0);

        ctx.register_table("rum_views", table.clone()).unwrap();
        CURRENT_VIEWS[0].register(&ctx, table).unwrap();

        ctx
    }

    async fn loading_times(ctx: &SessionContext, sql: &str) -> Vec<(String, i64)> {
        let batches = ctx.sql(sql).await.unwrap().collect().await.unwrap();
        let mut out = Vec::new();

        for batch in batches {
            let views = batch
                .column(0)
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();

            let loading = batch
                .column(1)
                .as_any()
                .downcast_ref::<Int64Array>()
                .unwrap();

            for index in 0..batch.num_rows() {
                out.push((views.value(index).to_string(), loading.value(index)));
            }
        }

        out
    }

    const ALL: &str =
        "SELECT \"view.id\", \"view.loading_time\" FROM rum_views_latest ORDER BY \"view.id\"";

    #[tokio::test]
    async fn an_entity_is_read_at_its_newest_revision() {
        let ctx = context(&[
            row(10, "a", "v1", Some(1), "e1", 100),
            row(10, "a", "v1", Some(3), "e3", 300),
            row(10, "a", "v1", Some(2), "e2", 200),
            row(20, "a", "v2", Some(1), "e4", 50),
        ]);

        assert_eq!(
            loading_times(&ctx, ALL).await,
            vec![("v1".to_string(), 300), ("v2".to_string(), 50)]
        );
    }

    #[tokio::test]
    async fn the_later_event_wins_between_equal_revisions() {
        let ctx = context(&[
            row(10, "a", "v1", Some(2), "e1", 111),
            row(10, "a", "v1", Some(2), "e9", 999),
        ]);

        assert_eq!(
            loading_times(&ctx, ALL).await,
            vec![("v1".to_string(), 999)]
        );
    }

    #[tokio::test]
    async fn a_row_without_a_revision_gives_way_to_any_revision() {
        let ctx = context(&[
            row(10, "a", "v1", None, "e9", 999),
            row(10, "a", "v1", Some(1), "e1", 100),
            row(20, "a", "v2", None, "e2", 20),
        ]);

        assert_eq!(
            loading_times(&ctx, ALL).await,
            vec![("v1".to_string(), 100), ("v2".to_string(), 20)]
        );
    }

    #[tokio::test]
    async fn entities_of_two_applications_are_kept_apart() {
        let ctx = context(&[
            row(10, "a", "v1", Some(1), "e1", 100),
            row(10, "b", "v1", Some(5), "e2", 500),
        ]);

        let counted = loading_times(
            &ctx,
            "SELECT \"application.id\", count(*) FROM rum_views_latest GROUP BY 1 ORDER BY 1",
        )
        .await;

        assert_eq!(counted, vec![("a".to_string(), 1), ("b".to_string(), 1)]);
    }

    #[tokio::test]
    async fn the_rank_is_not_a_column_of_the_view() {
        let ctx = context(&[row(10, "a", "v1", Some(1), "e1", 100)]);

        let frame = ctx.sql("SELECT * FROM rum_views_latest").await.unwrap();
        let names = frame
            .schema()
            .fields()
            .iter()
            .map(|field| field.name().clone())
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            schema()
                .fields()
                .iter()
                .map(|field| field.name().clone())
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn the_time_of_a_view_compares_with_microseconds() {
        let ctx = context(&[
            row(10, "a", "v1", Some(1), "e1", 100),
            row(20, "a", "v2", Some(1), "e2", 200),
        ]);

        let sql = "SELECT \"view.id\", \"view.loading_time\" FROM rum_views_latest \
                   WHERE _timestamp >= 15 ORDER BY \"view.id\"";

        assert_eq!(
            loading_times(&ctx, sql).await,
            vec![("v2".to_string(), 200)]
        );
    }

    async fn plan(ctx: &SessionContext, sql: &str) -> String {
        let plan = ctx.sql(sql).await.unwrap().into_optimized_plan().unwrap();

        format!("{}", plan.display_indent())
    }

    /// Whether the plan asks `condition` of the stream before the newest revision is picked.
    fn asked_before_the_newest(plan: &str, condition: &str) -> bool {
        let window = plan.find("WindowAggr").expect("a window in the plan");
        let asked = plan
            .find(condition)
            .unwrap_or_else(|| panic!("`{condition}` in the plan:\n{plan}"));

        asked > window
    }

    #[tokio::test]
    async fn what_names_an_entity_is_asked_before_the_newest_revision_is_picked() {
        let ctx = context(&[row(10, "a", "v1", Some(1), "e1", 100)]);

        let identity = plan(
            &ctx,
            "SELECT count(*) FROM rum_views_latest WHERE \"application.id\" = 'a'",
        )
        .await;

        assert!(
            asked_before_the_newest(&identity, "application.id = Utf8"),
            "{identity}"
        );
    }

    #[tokio::test]
    async fn what_a_revision_says_is_asked_of_the_newest_alone() {
        let ctx = context(&[row(10, "a", "v1", Some(1), "e1", 100)]);

        let state = plan(
            &ctx,
            "SELECT count(*) FROM rum_views_latest WHERE \"view.loading_time\" > 1000",
        )
        .await;

        assert!(!asked_before_the_newest(&state, "> Int64(1000)"), "{state}");
    }

    #[tokio::test]
    async fn the_view_is_made_over_the_table_registered_last() {
        let ctx = context(&[row(10, "a", "v1", Some(1), "e1", 100)]);

        let again = Arc::new(MemTable::try_new(schema(), vec![vec![]]).unwrap());
        ctx.deregister_table("rum_views").unwrap();
        ctx.register_table("rum_views", again.clone()).unwrap();
        CURRENT_VIEWS[0].register(&ctx, again).unwrap();

        assert_eq!(loading_times(&ctx, ALL).await, Vec::new());
    }

    #[test]
    fn a_stream_without_the_columns_of_its_revisions_is_refused() {
        let ctx = session_context(0);

        let bare = Arc::new(Schema::new(vec![Field::new(
            "_timestamp",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        )]));

        let table = Arc::new(MemTable::try_new(bare, vec![vec![]]).unwrap());
        let refused = CURRENT_VIEWS[0].register(&ctx, table).unwrap_err();

        assert!(refused.to_string().contains("view.id"), "{refused}");
    }

    #[test]
    fn every_view_is_named_after_a_versioned_stream() {
        for view in &CURRENT_VIEWS {
            assert!(view.stream.versioned().is_some(), "{}", view.name);
            assert_eq!(view.name, format!("{}_latest", view.base()));
        }
    }

    #[test]
    fn a_name_is_resolved_to_its_stream_whatever_the_case() {
        assert_eq!(base_stream("rum_views_latest"), Some("rum_views"));
        assert_eq!(base_stream("RUM_Sessions_Latest"), Some("rum_sessions"));
        assert_eq!(base_stream("rum_views"), None);
        assert_eq!(base_stream("logs"), None);
    }

    #[test]
    fn a_statement_names_the_views_it_reads() {
        let sql = "select count(*) from RUM_VIEWS_LATEST v join rum_sessions_latest s on 1=1";
        let names = mentioned(sql)
            .iter()
            .map(|view| view.name)
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["rum_views_latest", "rum_sessions_latest"]);
        assert!(mentioned("select * from rum_views").is_empty());
    }
}
