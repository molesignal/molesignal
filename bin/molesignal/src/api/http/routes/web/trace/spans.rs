// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `GET /api/v1/web/traces/spans` — the Spans a trace query matches, one row
//! each, a page at a time.
//!
//! The trace list answers with traces. This answers with Spans, and reads its
//! query the way the RED figures do: a filter on a field of a Span holds for
//! the Span itself, and a Span without the field is kept where the filter
//! leaves a value out; `duration_ns` is the Span's own, the span and error
//! counts are those of the trace it belongs to.
//!
//! It runs as a tracked query: admitted by role, listed among the running
//! queries and masked like any other.

use axum::{
    Extension,
    extract::{Query, State},
    response::Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use self::{
    cursor::{SpanCursorBoundary, SpanCursorPosition},
    statement::SpanStatement,
};
use super::{
    filter::{self, TraceCondition, TraceFilter},
    list::{MAX_TRACE_FILTERS, MAX_TRACE_PAGE_SIZE, clean_input},
    red::{RedScope, statements::Source},
    resolve_traces_stream_definition,
};
use crate::{
    api::{
        AppState,
        http::pagination::cursor::{CursorDirection, CursorPage, trim_cursor_page},
    },
    app::iam::IamContext,
    domain::{
        iam::permission,
        query::{QueryLanguage, QueryRequest, QueryResult, StreamHint},
        stream::StreamType,
    },
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

mod cursor;
mod statement;

const DEFAULT_WINDOW_SECS: i64 = 24 * 60 * 60;
const DEFAULT_SPAN_PAGE_SIZE: usize = 100;

#[derive(Debug, Deserialize)]
pub(super) struct ListSpansQuery {
    from: Option<i64>,
    to: Option<i64>,
    q: Option<String>,
    filters: Option<String>,
    /// The condition tree, as JSON text, as the trace list takes it.
    #[serde(rename = "where")]
    condition: Option<String>,
    /// The field query as text, read as the Traces page reads it; it stands
    /// for `q`, `filters` and `where`.
    expression: Option<String>,
    sort: Option<SpanListSort>,
    #[serde(default = "default_span_page_size")]
    limit: usize,
    cursor: Option<String>,
}

/// The order the Spans are listed in.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum SpanListSort {
    #[default]
    Latest,
    Earliest,
    DurationDesc,
    DurationAsc,
}

/// One Span of the list: where it is, whose it is, when it ran and how it ended.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpanListItem {
    pub trace_id: String,
    pub span_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_span_id: Option<String>,
    pub service: String,
    pub operation: String,
    pub start_ns: i64,
    pub duration_ns: i64,
    /// `OK`, `ERROR` or `UNSET`.
    pub status: String,
    /// The kind as OTLP numbers it: 1 internal, 2 server, 3 client, 4 producer, 5 consumer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status_code: Option<i64>,
}

/// The query a page is of, and where the page begins.
#[derive(Clone, Debug)]
pub(super) struct SpanListContext {
    from: i64,
    to: i64,
    sort: SpanListSort,
    page_size: usize,
    q: Option<String>,
    filters: Vec<TraceFilter>,
    condition: Option<TraceCondition>,
    boundary: Option<SpanCursorBoundary>,
}

fn default_span_page_size() -> usize {
    DEFAULT_SPAN_PAGE_SIZE
}

#[permission(any("traces.read", "sys.telemetry.read"))]
pub(super) async fn list_spans(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<ListSpansQuery>,
) -> Result<Json<CursorPage<SpanListItem>>> {
    let mut query = query;
    super::expression::expand(
        &mut query.expression,
        &mut query.q,
        &mut query.filters,
        &mut query.condition,
    )?;

    let Some(definition) = resolve_traces_stream_definition(&state, &ctx.org_id).await else {
        return Ok(Json(CursorPage::empty()));
    };

    let context = resolve_context(&state, &ctx, query, &definition.schema)?;

    let asked = TraceCondition::of(
        &context.filters,
        context.q.as_deref(),
        context.condition.as_ref(),
    );

    let source = Source::new(&definition, RedScope::All, &asked)?;

    let statement = SpanStatement::new(&definition.schema, &source);
    let fetch_limit = context.page_size.saturating_add(1);

    let request = QueryRequest {
        org_id: ctx.org_id.clone(),
        language: QueryLanguage::Sql,
        statement: statement.sql(&context, fetch_limit),
        time_range: TimeRange::new(TimestampMicros(context.from), TimestampMicros(context.to)),
        stream: Some(StreamHint {
            name: definition.name.clone(),
            stream_type: StreamType::TRACES,
        }),
        limit: Some(fetch_limit),
        step: None,
        federation_clusters: Vec::new(),
    };

    let result = state
        .query
        .run_tracked(request, ctx.user_id.clone(), ctx.organization_role_key())
        .await?;

    let rows = items(&result);
    let direction = context.boundary.as_ref().map(|boundary| boundary.direction);
    let page = trim_cursor_page(rows, context.page_size, direction);
    let iam = state.iam.service.as_ref();

    let encode = |direction: CursorDirection, item: &SpanListItem| {
        cursor::encode(iam, &ctx.org_id, &context, direction, item)
    };

    let previous_cursor = match page.items.first() {
        Some(first) if page.has_previous => Some(encode(CursorDirection::Before, first)?),
        _ => None,
    };

    let next_cursor = match page.items.last() {
        Some(last) if page.has_next => Some(encode(CursorDirection::After, last)?),
        _ => None,
    };

    Ok(Json(CursorPage {
        items: page.items,
        has_more: next_cursor.is_some(),
        next_cursor,
        previous_cursor,
    }))
}

fn resolve_context(
    state: &AppState,
    ctx: &IamContext,
    query: ListSpansQuery,
    schema: &crate::domain::stream::Schema,
) -> Result<SpanListContext> {
    let page_size = query.limit.clamp(1, MAX_TRACE_PAGE_SIZE);

    if let Some(token) = query.cursor.as_deref() {
        let payload = cursor::decode(state.iam.service.as_ref(), &ctx.org_id, token)?;
        filter::validate(&payload.filters, schema)?;

        if let Some(condition) = &payload.condition {
            filter::condition::validate(condition, schema)?;
        }

        if page_size != payload.page_size {
            return Err(Error::invalid(
                "span cursor does not match the active query",
            ));
        }

        return Ok(SpanListContext {
            from: payload.from,
            to: payload.to,
            sort: payload.sort,
            page_size: payload.page_size,
            q: payload.q,
            filters: payload.filters,
            condition: payload.condition,
            boundary: Some(SpanCursorBoundary {
                direction: payload.direction,
                position: payload.position,
            }),
        });
    }

    let now = TimestampMicros::now().0;
    let from = query.from.unwrap_or(now - DEFAULT_WINDOW_SECS * 1_000_000);
    let to = query.to.unwrap_or(now);

    if to <= from {
        return Err(Error::invalid("to must be greater than from"));
    }

    let filters = filter::parse(query.filters.as_deref(), schema, MAX_TRACE_FILTERS)?;

    let condition = filter::condition::parse(
        query.condition.as_deref(),
        schema,
        MAX_TRACE_FILTERS - filters.len(),
    )?;

    Ok(SpanListContext {
        from,
        to,
        sort: query.sort.unwrap_or_default(),
        page_size,
        q: query.q.as_deref().and_then(clean_input),
        filters,
        condition,
        boundary: None,
    })
}

/// The Spans of a result, in the order it returned them. A row without a
/// trace or Span id, which masking can leave, is no Span to open.
fn items(result: &QueryResult) -> Vec<SpanListItem> {
    let position = |name: &str| result.columns.iter().position(|column| column == name);
    let columns = statement::ResultColumns::locate(&position);

    result
        .rows
        .iter()
        .filter_map(|row| {
            let text = |index: Option<usize>| {
                index
                    .and_then(|index| row.get(index))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            };
            let number =
                |index: Option<usize>| index.and_then(|index| row.get(index)).and_then(whole);

            Some(SpanListItem {
                trace_id: text(columns.trace_id).filter(|id| !id.is_empty())?,
                span_id: text(columns.span_id).filter(|id| !id.is_empty())?,
                parent_span_id: text(columns.parent_span_id).filter(|id| !id.is_empty()),
                service: text(columns.service).unwrap_or_default(),
                operation: text(columns.operation).unwrap_or_default(),
                start_ns: number(columns.start).unwrap_or_default(),
                duration_ns: number(columns.duration).unwrap_or_default().max(0),
                status: text(columns.status).unwrap_or_default(),
                kind: columns
                    .kind
                    .and_then(|index| row.get(index))
                    .and_then(span_kind),
                http_status_code: columns
                    .http_status
                    .iter()
                    .find_map(|index| row.get(*index).and_then(whole))
                    .filter(|status| (100..600).contains(status)),
            })
        })
        .collect()
}

/// A whole number, stored as one or as its digits.
fn whole(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_f64().map(|number| number as i64))
        .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
}

/// A kind as OTLP numbers it, from a number, its digits or its name.
fn span_kind(value: &Value) -> Option<i64> {
    if let Some(number) = whole(value) {
        return (0..=5).contains(&number).then_some(number);
    }

    let name = value.as_str()?.trim().to_ascii_uppercase();

    match name.trim_start_matches("SPAN_KIND_") {
        "INTERNAL" => Some(1),
        "SERVER" => Some(2),
        "CLIENT" => Some(3),
        "PRODUCER" => Some(4),
        "CONSUMER" => Some(5),
        _ => None,
    }
}

impl SpanListContext {
    /// Where the page begins, when it is not the first.
    fn position(&self) -> Option<(&SpanCursorPosition, CursorDirection)> {
        self.boundary
            .as_ref()
            .map(|boundary| (&boundary.position, boundary.direction))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn result(columns: &[&str], rows: Vec<Vec<Value>>) -> QueryResult {
        QueryResult {
            columns: columns.iter().map(|column| column.to_string()).collect(),
            rows,
            scanned_rows: 0,
            took_ms: 0,
            federation: None,
        }
    }

    #[test]
    fn reads_a_span_of_each_row_and_skips_rows_without_ids() {
        let result = result(
            &[
                "trace_id",
                "span_id",
                "parent_span_id",
                "service.name",
                "name",
                statement::START,
                statement::DURATION,
                "status_code",
                "kind",
                "http.response.status_code",
            ],
            vec![
                vec![
                    json!("t1"),
                    json!("s1"),
                    json!(""),
                    json!("checkout"),
                    json!("POST /charge"),
                    json!(1_000),
                    json!(250),
                    json!("ERROR"),
                    json!("SPAN_KIND_SERVER"),
                    json!(503),
                ],
                vec![json!("***"), Value::Null],
            ],
        );

        assert_eq!(
            items(&result),
            vec![SpanListItem {
                trace_id: "t1".into(),
                span_id: "s1".into(),
                parent_span_id: None,
                service: "checkout".into(),
                operation: "POST /charge".into(),
                start_ns: 1_000,
                duration_ns: 250,
                status: "ERROR".into(),
                kind: Some(2),
                http_status_code: Some(503),
            }]
        );
    }

    #[test]
    fn a_kind_is_read_from_its_number_or_its_name() {
        assert_eq!(span_kind(&json!(3)), Some(3));
        assert_eq!(span_kind(&json!("5")), Some(5));
        assert_eq!(span_kind(&json!("producer")), Some(4));
        assert_eq!(span_kind(&json!("SPAN_KIND_UNSPECIFIED")), None);
        assert_eq!(span_kind(&json!(9)), None);
    }
}
