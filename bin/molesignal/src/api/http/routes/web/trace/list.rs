// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Cursor-paginated trace summaries for the investigation workbench.

use std::collections::BTreeMap;

use axum::{
    Extension,
    extract::{Query, State},
    response::Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use self::cursor::{TraceCursorBoundary, TraceCursorPayload};
use super::{
    filter::{self, TraceCondition, TraceFilter},
    resolve_traces_stream_definition,
};
use crate::{
    api::{
        AppState,
        http::pagination::cursor::{CursorDirection, CursorPage, trim_cursor_page},
    },
    app::iam::IamContext,
    domain::iam::permission,
    shared::{Error, Result, time::TimestampMicros},
};

mod cursor;
pub(super) mod histogram;
mod projection;
mod roots;
mod scan;
mod span_filter;

const DEFAULT_WINDOW_SECS: i64 = 24 * 60 * 60;
const DEFAULT_TRACE_PAGE_SIZE: usize = 20;
/// The most traces one answer holds. The Traces explorer offers pages up to this size
/// (`EXPLORER_PAGE_SIZE_OPTIONS` in the web app), so the two change together.
pub(super) const MAX_TRACE_PAGE_SIZE: usize = 5_000;
pub(super) const MAX_TRACE_FILTERS: usize = 32;

#[derive(Debug, Deserialize)]
pub(super) struct ListTracesQuery {
    from: Option<i64>,
    to: Option<i64>,
    q: Option<String>,
    filters: Option<String>,
    /// The condition tree, as JSON text: alternatives the list of filters
    /// cannot say. It holds together with the filters and the free text.
    #[serde(rename = "where")]
    condition: Option<String>,
    /// The field query as text, read as the Traces page reads it; it stands
    /// for `q`, `filters` and `where`.
    expression: Option<String>,
    sort: Option<TraceListSort>,
    #[serde(default = "default_trace_page_size")]
    limit: usize,
    cursor: Option<String>,
    /// Fields of the root Span to answer with, comma separated. They are read
    /// for the page alone, so they are no part of the query a cursor keeps.
    fields: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum TraceListSort {
    #[default]
    Latest,
    Earliest,
    DurationDesc,
    DurationAsc,
    SpanCountDesc,
    ErrorsDesc,
}

#[derive(Clone, Debug)]
pub(super) struct TraceListContext {
    from: i64,
    to: i64,
    sort: TraceListSort,
    page_size: usize,
    q: Option<String>,
    filters: Vec<TraceFilter>,
    condition: Option<TraceCondition>,
    boundary: Option<TraceCursorBoundary>,
}

impl TraceListContext {
    /// The whole query: the filters, the free text and the condition tree.
    pub(super) fn query(&self) -> TraceCondition {
        TraceCondition::of(&self.filters, self.q.as_deref(), self.condition.as_ref())
    }
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceListItem {
    pub trace_id: String,
    pub service: String,
    pub operation: String,
    pub start_ns: i64,
    pub duration_ms: f64,
    pub span_count: i64,
    pub error_count: i64,
    /// The status of the trace's root Span: `OK`, `ERROR` or `UNSET`.
    pub status: String,
    /// The HTTP status the root Span answered with, where it kept one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status_code: Option<i64>,
    /// The fields of the root Span that were asked for, by their names.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug)]
pub(super) struct TraceListRow {
    item: TraceListItem,
    duration_ns: i64,
    /// The Span the trace's summary was written from: its root.
    root_span_id: String,
}

fn default_trace_page_size() -> usize {
    DEFAULT_TRACE_PAGE_SIZE
}

#[permission(any("traces.read", "sys.telemetry.read"))]
pub(super) async fn list_traces(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<ListTracesQuery>,
) -> Result<Json<CursorPage<TraceListItem>>, Error> {
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

    let requested = roots::requested_fields(query.fields.as_deref(), &definition.schema)?;
    let context = resolve_context(&state, &ctx, query, &definition.schema)?;
    let stream = definition.name.clone();

    if !projection::available(&state, &ctx, &stream).await {
        // Cursor pagination is backed exclusively by the write-time summary
        // projection. Legacy span-only streams intentionally stay empty
        // instead of falling back to an unbounded aggregation over every span.
        return Ok(Json(CursorPage::empty()));
    }

    let fetch_limit = context.page_size.saturating_add(1);

    let rows = if context.query().has_span_filter() {
        span_filter::run(&state, &definition, &context, fetch_limit).await?
    } else {
        scan::run(&state, &ctx.org_id, &stream, &context, fetch_limit).await?
    };

    let direction = context.boundary.as_ref().map(|boundary| boundary.direction);
    let mut page = trim_cursor_page(rows, context.page_size, direction);

    roots::attach(&state, &ctx, &definition, &requested, &mut page.items).await?;

    let previous_cursor = if page.has_previous {
        page.items
            .first()
            .map(|row| {
                cursor::encode(
                    state.iam.service.as_ref(),
                    &ctx.org_id,
                    &context,
                    CursorDirection::Before,
                    row,
                )
            })
            .transpose()?
    } else {
        None
    };

    let next_cursor = if page.has_next {
        page.items
            .last()
            .map(|row| {
                cursor::encode(
                    state.iam.service.as_ref(),
                    &ctx.org_id,
                    &context,
                    CursorDirection::After,
                    row,
                )
            })
            .transpose()?
    } else {
        None
    };

    Ok(Json(CursorPage {
        items: page.items.into_iter().map(|row| row.item).collect(),
        has_more: next_cursor.is_some(),
        next_cursor,
        previous_cursor,
    }))
}

fn resolve_context(
    state: &AppState,
    ctx: &IamContext,
    query: ListTracesQuery,
    schema: &crate::domain::stream::Schema,
) -> Result<TraceListContext> {
    let requested_page_size = query.limit.clamp(1, MAX_TRACE_PAGE_SIZE);

    if let Some(token) = query.cursor.as_deref() {
        let payload = cursor::decode(state.iam.service.as_ref(), &ctx.org_id, token)?;
        validate_cursor_request(&query, requested_page_size, &payload, schema)?;
        filter::validate(&payload.filters, schema)?;

        if let Some(condition) = &payload.condition {
            filter::condition::validate(condition, schema)?;
        }

        return Ok(TraceListContext {
            from: payload.from,
            to: payload.to,
            sort: payload.sort,
            page_size: payload.page_size,
            q: payload.q,
            filters: payload.filters,
            condition: payload.condition,
            boundary: Some(TraceCursorBoundary {
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

    Ok(TraceListContext {
        from,
        to,
        sort: query.sort.unwrap_or_default(),
        page_size: requested_page_size,
        q: query.q.as_deref().and_then(clean_input),
        filters,
        condition,
        boundary: None,
    })
}

fn validate_cursor_request(
    query: &ListTracesQuery,
    requested_page_size: usize,
    payload: &TraceCursorPayload,
    schema: &crate::domain::stream::Schema,
) -> Result<()> {
    let mismatched = query.from.is_some_and(|from| from != payload.from)
        || query.to.is_some_and(|to| to != payload.to)
        || query.sort.is_some_and(|sort| sort != payload.sort)
        || requested_page_size != payload.page_size
        || query
            .q
            .as_deref()
            .is_some_and(|q| clean_input(q) != payload.q)
        || query.filters.as_deref().is_some_and(|filters| {
            filter::parse(Some(filters), schema, MAX_TRACE_FILTERS)
                .map(|parsed| parsed != payload.filters)
                .unwrap_or(true)
        })
        || query.condition.as_deref().is_some_and(|condition| {
            filter::condition::parse(Some(condition), schema, MAX_TRACE_FILTERS)
                .map(|parsed| parsed != payload.condition)
                .unwrap_or(true)
        });

    if mismatched {
        return Err(Error::invalid(
            "trace cursor does not match the active query",
        ));
    }

    Ok(())
}

pub(super) fn clean_input(value: &str) -> Option<String> {
    let trimmed = value.trim();

    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.chars().take(256).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_filters_reject_unknown_field_before_scan() {
        let schema = crate::shared::trace::normalization::canonical_trace_schema();
        let filters = serde_json::json!([{
            "field": "duration_ms",
            "op": "=",
            "value": "10"
        }])
        .to_string();
        let error = filter::parse(Some(&filters), &schema, MAX_TRACE_FILTERS)
            .expect_err("unknown field must fail");
        assert!(error.to_string().contains("unsupported trace filter field"));
    }

    #[test]
    fn trace_filters_reject_unknown_operator_before_scan() {
        let schema = crate::shared::trace::normalization::canonical_trace_schema();
        let filters = serde_json::json!([{
            "field": "service.name",
            "op": "regex",
            "value": "api.*"
        }])
        .to_string();
        let error = filter::parse(Some(&filters), &schema, MAX_TRACE_FILTERS)
            .expect_err("unknown operator must fail");
        assert!(
            error
                .to_string()
                .contains("unsupported trace filter operator")
        );
    }

    /// The Grafana plugin and other clients read a listed trace by these names.
    #[test]
    fn a_listed_trace_keeps_the_names_clients_read() {
        let item = TraceListItem {
            trace_id: "t1".into(),
            service: "api".into(),
            operation: "GET /".into(),
            start_ns: 1,
            duration_ms: 2.5,
            span_count: 3,
            error_count: 0,
            status: "OK".into(),
            http_status_code: Some(200),
            fields: BTreeMap::new(),
        };

        assert_eq!(
            serde_json::to_value(item).unwrap(),
            serde_json::json!({
                "traceId": "t1",
                "service": "api",
                "operation": "GET /",
                "startNs": 1,
                "durationMs": 2.5,
                "spanCount": 3,
                "errorCount": 0,
                "status": "OK",
                "httpStatusCode": 200
            })
        );
    }
}
