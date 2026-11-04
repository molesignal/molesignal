// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The statement a page of Spans is read with: what it selects, the order it
//! keeps and where a page after the first begins.

use super::{SpanListContext, SpanListSort};
use crate::{
    api::http::{pagination::cursor::CursorDirection, routes::web::trace::red::statements::Source},
    domain::stream::Schema,
    infra::query::escape_sql_ident,
    shared::cursor::{CursorSortDirection, CursorValue, lexicographic_seek},
};

/// Result columns of what is worked out for a Span, named with a prefix no field has.
pub(super) const START: &str = "__spans_start";
pub(super) const DURATION: &str = "__spans_duration";

/// Where a Span keeps the HTTP status it answered with, the current name first.
const HTTP_STATUS_FIELDS: [&str; 2] = ["http.response.status_code", "http.status_code"];
/// The fields a row of the list shows, where the stream has them.
const LISTED_FIELDS: [&str; 6] = [
    "trace_id",
    "span_id",
    "parent_span_id",
    "service.name",
    "name",
    "status_code",
];

pub(super) struct SpanStatement<'a> {
    source: &'a Source,
    /// When a Span began, in nanoseconds.
    start: String,
    /// What a row selects.
    selected: Vec<String>,
}

impl<'a> SpanStatement<'a> {
    pub(super) fn new(schema: &Schema, source: &'a Source) -> Self {
        let declared = |name: &str| schema.fields.iter().any(|field| field.name == name);

        let start = if declared("start_time_unix_nano") {
            "CAST(\"start_time_unix_nano\" AS BIGINT)".to_string()
        } else {
            "(CAST(_timestamp AS BIGINT) * 1000)".to_string()
        };

        let mut selected = LISTED_FIELDS
            .into_iter()
            .chain(declared("kind").then_some("kind"))
            .chain(HTTP_STATUS_FIELDS)
            .filter(|name| declared(name))
            .map(quoted)
            .collect::<Vec<_>>();

        selected.push(format!("{start} AS {START}"));
        selected.push(format!("{} AS {DURATION}", source.duration()));

        Self {
            source,
            start,
            selected,
        }
    }

    pub(super) fn sql(&self, context: &SpanListContext, fetch_limit: usize) -> String {
        let seek = context
            .position()
            .map(|(position, direction)| {
                let keys = self.keys(context.sort);

                let fields = keys
                    .iter()
                    .map(|(expression, key, order)| {
                        let value = match key {
                            Key::Primary => CursorValue::Integer(position.primary),
                            Key::Start => CursorValue::Integer(position.start_ns),
                            Key::Span => CursorValue::Text(position.span_id.clone()),
                            Key::Trace => CursorValue::Text(position.trace_id.clone()),
                        };

                        (expression.as_str(), value, *order)
                    })
                    .collect::<Vec<_>>();

                lexicographic_seek(&fields, direction, literal)
            })
            .into_iter()
            .collect::<Vec<_>>();

        format!(
            "SELECT {} {} ORDER BY {} LIMIT {fetch_limit}",
            self.selected.join(", "),
            self.source.reading_with(&seek),
            self.order_by(context),
        )
    }

    /// The keys the list is ordered by, the first deciding most.
    fn keys(&self, sort: SpanListSort) -> Vec<(String, Key, CursorSortDirection)> {
        use CursorSortDirection::{Asc, Desc};

        let span = quoted("span_id");
        let trace = quoted("trace_id");
        let duration = self.source.duration().to_string();

        match sort {
            SpanListSort::Latest => vec![
                (self.start.clone(), Key::Start, Desc),
                (span, Key::Span, Desc),
                (trace, Key::Trace, Desc),
            ],
            SpanListSort::Earliest => vec![
                (self.start.clone(), Key::Start, Asc),
                (span, Key::Span, Asc),
                (trace, Key::Trace, Asc),
            ],
            SpanListSort::DurationDesc | SpanListSort::DurationAsc => vec![
                (
                    duration,
                    Key::Primary,
                    if sort == SpanListSort::DurationAsc {
                        Asc
                    } else {
                        Desc
                    },
                ),
                (self.start.clone(), Key::Start, Desc),
                (span, Key::Span, Desc),
                (trace, Key::Trace, Desc),
            ],
        }
    }

    fn order_by(&self, context: &SpanListContext) -> String {
        // A page before the cursor is read backwards and turned round after.
        let reverse = context
            .position()
            .is_some_and(|(_, direction)| direction == CursorDirection::Before);

        self.keys(context.sort)
            .into_iter()
            .map(|(expression, _, order)| {
                let ascending = (order == CursorSortDirection::Asc) != reverse;
                format!("{expression} {}", if ascending { "ASC" } else { "DESC" })
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Which part of a cursor's position a key compares with.
#[derive(Clone, Copy)]
enum Key {
    Primary,
    Start,
    Span,
    Trace,
}

/// Where each part of a Span is among a result's columns.
pub(super) struct ResultColumns {
    pub(super) trace_id: Option<usize>,
    pub(super) span_id: Option<usize>,
    pub(super) parent_span_id: Option<usize>,
    pub(super) service: Option<usize>,
    pub(super) operation: Option<usize>,
    pub(super) start: Option<usize>,
    pub(super) duration: Option<usize>,
    pub(super) status: Option<usize>,
    pub(super) kind: Option<usize>,
    /// The HTTP status columns the result has, the current name first.
    pub(super) http_status: Vec<usize>,
}

impl ResultColumns {
    pub(super) fn locate(position: &impl Fn(&str) -> Option<usize>) -> Self {
        Self {
            trace_id: position("trace_id"),
            span_id: position("span_id"),
            parent_span_id: position("parent_span_id"),
            service: position("service.name"),
            operation: position("name"),
            start: position(START),
            duration: position(DURATION),
            status: position("status_code"),
            kind: position("kind"),
            http_status: HTTP_STATUS_FIELDS
                .into_iter()
                .filter_map(position)
                .collect(),
        }
    }
}

fn quoted(name: &str) -> String {
    format!("\"{}\"", escape_sql_ident(name))
}

fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::{
        array::{Int64Array, RecordBatch, StringArray},
        datatypes::{DataType, Field, Schema as ArrowSchema},
    };
    use datafusion::{datasource::MemTable, prelude::SessionContext};

    use super::*;
    use crate::{
        api::http::routes::web::trace::{
            filter,
            red::RedScope,
            spans::{SpanCursorBoundary, SpanCursorPosition},
        },
        domain::stream::{FieldDef, FieldType, StreamDefinition, StreamType},
        infra::query::arrow_json::batches_to_json,
        shared::{ids::Id, time::TimestampMicros, trace::normalization::canonical_trace_schema},
    };

    fn definition() -> StreamDefinition {
        let mut schema = canonical_trace_schema();

        schema.fields.push(FieldDef {
            name: "http.response.status_code".into(),
            data_type: FieldType::Int64,
            nullable: true,
            index_type: None,
            indexed: false,
            encrypted: false,
            exact: false,
        });

        StreamDefinition {
            id: Id::from_string("spans"),
            org_id: Id::from_string("org"),
            name: "spans".into(),
            stream_type: StreamType::TRACES,
            schema,
            retention: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    fn context(
        sort: SpanListSort,
        filters: &str,
        boundary: Option<SpanCursorBoundary>,
    ) -> SpanListContext {
        SpanListContext {
            from: 0,
            to: 1_000,
            sort,
            page_size: 2,
            q: None,
            filters: filter::parse(Some(filters), &definition().schema, 32).expect("filters"),
            condition: None,
            boundary,
        }
    }

    /// Four Spans of two traces.
    async fn session() -> SessionContext {
        let session = SessionContext::new();
        let schema = Arc::new(ArrowSchema::new(vec![
            Field::new("trace_id", DataType::Utf8, false),
            Field::new("span_id", DataType::Utf8, false),
            Field::new("parent_span_id", DataType::Utf8, true),
            Field::new("service.name", DataType::Utf8, true),
            Field::new("name", DataType::Utf8, true),
            Field::new("status_code", DataType::Utf8, true),
            Field::new("kind", DataType::Int64, true),
            Field::new("http.response.status_code", DataType::Int64, true),
            Field::new("start_time_unix_nano", DataType::Int64, false),
            Field::new("duration_ns", DataType::Int64, false),
        ]));
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(StringArray::from(vec!["t1", "t1", "t2", "t2"])),
                Arc::new(StringArray::from(vec!["a", "b", "c", "d"])),
                Arc::new(StringArray::from(vec![None, Some("a"), None, Some("c")])),
                Arc::new(StringArray::from(vec!["api", "db", "api", "pay"])),
                Arc::new(StringArray::from(vec!["GET /", "SELECT", "GET /", "POST"])),
                Arc::new(StringArray::from(vec!["OK", "OK", "ERROR", "ERROR"])),
                Arc::new(Int64Array::from(vec![2, 3, 2, 3])),
                Arc::new(Int64Array::from(vec![
                    Some(200),
                    None,
                    Some(500),
                    Some(503),
                ])),
                Arc::new(Int64Array::from(vec![10, 20, 30, 40])),
                Arc::new(Int64Array::from(vec![50, 5, 80, 60])),
            ],
        )
        .expect("spans");
        session
            .register_table(
                "spans",
                Arc::new(MemTable::try_new(schema, vec![vec![batch]]).expect("table")),
            )
            .expect("register");
        session
    }

    async fn ids(context: &SpanListContext) -> Vec<String> {
        let definition = definition();
        let query = filter::TraceCondition::of(&context.filters, None, None);
        let source = Source::new(&definition, RedScope::All, &query).expect("source");
        let statement = SpanStatement::new(&definition.schema, &source);
        let batches = session()
            .await
            .sql(&statement.sql(context, 3))
            .await
            .expect("plan")
            .collect()
            .await
            .expect("run");
        let (columns, rows) = batches_to_json(&batches);
        let span = columns
            .iter()
            .position(|column| column == "span_id")
            .unwrap();

        rows.iter()
            .map(|row| row[span].as_str().unwrap().to_string())
            .collect()
    }

    #[tokio::test]
    async fn lists_the_newest_first_and_the_longest_first_when_asked() {
        assert_eq!(
            ids(&context(SpanListSort::Latest, "[]", None)).await,
            ["d", "c", "b"]
        );
        assert_eq!(
            ids(&context(SpanListSort::DurationDesc, "[]", None)).await,
            ["c", "d", "a"]
        );
        assert_eq!(
            ids(&context(SpanListSort::Earliest, "[]", None)).await,
            ["a", "b", "c"]
        );
    }

    #[tokio::test]
    async fn a_filter_holds_for_the_span_itself_and_in_takes_several_values() {
        let failed = r#"[{"field":"status_code","op":"=","value":"ERROR"}]"#;
        assert_eq!(
            ids(&context(SpanListSort::Latest, failed, None)).await,
            ["d", "c"]
        );

        let services = r#"[{"field":"service.name","op":"in","values":["db","pay"]}]"#;
        assert_eq!(
            ids(&context(SpanListSort::Latest, services, None)).await,
            ["d", "b"]
        );
    }

    #[tokio::test]
    async fn a_page_after_the_cursor_begins_after_its_span() {
        let boundary = SpanCursorBoundary {
            direction: CursorDirection::After,
            position: SpanCursorPosition {
                primary: 30,
                start_ns: 30,
                span_id: "c".into(),
                trace_id: "t2".into(),
            },
        };

        assert_eq!(
            ids(&context(SpanListSort::Latest, "[]", Some(boundary))).await,
            ["b", "a"]
        );
    }
}
