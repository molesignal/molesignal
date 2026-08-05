// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Cursor-paginated trace summaries for the investigation workbench.

use axum::{
    Extension,
    extract::{Query, State},
    response::Json,
};
use serde::{Deserialize, Serialize};

use self::{
    cursor::{TraceCursorBoundary, TraceCursorPayload},
    filter::TraceFilter,
};
use super::resolve_traces_stream_definition;
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
mod filter;
mod projection;
mod scan;
mod span_filter;

const DEFAULT_WINDOW_SECS: i64 = 24 * 60 * 60;
const DEFAULT_TRACE_PAGE_SIZE: usize = 20;
pub(super) const MAX_TRACE_PAGE_SIZE: usize = 100;
const MAX_TRACE_FILTERS: usize = 32;

#[derive(Debug, Deserialize)]
pub(super) struct ListTracesQuery {
    from: Option<i64>,
    to: Option<i64>,
    q: Option<String>,
    filters: Option<String>,
    sort: Option<TraceListSort>,
    #[serde(default = "default_trace_page_size")]
    limit: usize,
    cursor: Option<String>,
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
    boundary: Option<TraceCursorBoundary>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TraceListItem {
    pub trace_id: String,
    pub service: String,
    pub operation: String,
    pub start_ns: i64,
    pub duration_ms: f64,
    pub span_count: i64,
    pub error_count: i64,
}

#[derive(Clone, Debug)]
pub(super) struct TraceListRow {
    item: TraceListItem,
    duration_ns: i64,
}

fn default_trace_page_size() -> usize {
    DEFAULT_TRACE_PAGE_SIZE
}

#[permission(any("streams.query", "sys.telemetry.read"))]
pub(super) async fn list_traces(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<ListTracesQuery>,
) -> Result<Json<CursorPage<TraceListItem>>, Error> {
    let Some(definition) = resolve_traces_stream_definition(&state, &ctx.org_id).await else {
        return Ok(Json(CursorPage::empty()));
    };
    let context = resolve_context(&state, &ctx, query, &definition.schema)?;
    let stream = definition.name.clone();
    if !projection::available(&state, &ctx, &stream).await {
        // Cursor pagination is backed exclusively by the write-time summary
        // projection. Legacy span-only streams intentionally stay empty
        // instead of falling back to an unbounded aggregation over every span.
        return Ok(Json(CursorPage::empty()));
    }

    let fetch_limit = context.page_size.saturating_add(1);
    let rows = if context.filters.iter().any(TraceFilter::is_span_filter) {
        span_filter::run(&state, &ctx.org_id, &definition, &context, fetch_limit).await?
    } else {
        scan::run(&state, &ctx.org_id, &stream, &context, fetch_limit).await?
    };
    let direction = context.boundary.as_ref().map(|boundary| boundary.direction);
    let page = trim_cursor_page(rows, context.page_size, direction);

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
        return Ok(TraceListContext {
            from: payload.from,
            to: payload.to,
            sort: payload.sort,
            page_size: payload.page_size,
            q: payload.q,
            filters: payload.filters,
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
    Ok(TraceListContext {
        from,
        to,
        sort: query.sort.unwrap_or_default(),
        page_size: requested_page_size,
        q: query.q.as_deref().and_then(clean_input),
        filters: filter::parse(query.filters.as_deref(), schema, MAX_TRACE_FILTERS)?,
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
        });
    if mismatched {
        return Err(Error::invalid(
            "trace cursor does not match the active query",
        ));
    }
    Ok(())
}

fn clean_input(value: &str) -> Option<String> {
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
            "field": "service_name",
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
}
