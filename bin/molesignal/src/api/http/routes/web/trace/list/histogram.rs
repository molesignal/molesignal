// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! How many traces a list query matches, bar by bar of the explorer histogram.
//!
//! The list returns a page. This counts every trace the same query matches in
//! its time range, by when the trace began, the traces with a failed Span
//! apart from the rest. It reads what the list reads: the summaries, and the
//! Spans when a filter is on a field of a Span.

use axum::{
    Extension, Json,
    extract::{Query, State},
};
use serde::Deserialize;

use super::{
    MAX_TRACE_FILTERS, TraceListContext, TraceListSort, clean_input,
    filter::{self, TraceFilter},
    projection, scan, span_filter,
};
use crate::{
    api::{
        AppState,
        http::routes::web::{
            histogram::{BucketPlan, HistogramResponse, Unavailable, within_budget},
            trace::resolve_traces_stream_definition,
        },
    },
    app::iam::IamContext,
    domain::iam::permission,
    shared::Result,
};

#[derive(Debug, Deserialize)]
pub(in super::super) struct TraceHistogramQuery {
    from: Option<i64>,
    to: Option<i64>,
    q: Option<String>,
    filters: Option<String>,
    bucket_micros: Option<i64>,
    origin_micros: Option<i64>,
}

#[permission(any("streams.query", "sys.telemetry.read"))]
pub(in super::super) async fn histogram(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<TraceHistogramQuery>,
) -> Result<Json<HistogramResponse>> {
    let plan = BucketPlan::new(
        query.from,
        query.to,
        query.bucket_micros,
        query.origin_micros,
    )?;

    let Some(definition) = resolve_traces_stream_definition(&state, &ctx.org_id).await else {
        return Ok(Json(plan.counted(Vec::new(), true)));
    };

    // The query the list would page through, without a page.
    let context = TraceListContext {
        from: plan.from,
        to: plan.to,
        sort: TraceListSort::default(),
        page_size: 1,
        q: query.q.as_deref().and_then(clean_input),
        filters: filter::parse(
            query.filters.as_deref(),
            &definition.schema,
            MAX_TRACE_FILTERS,
        )?,
        boundary: None,
    };

    if !projection::available(&state, &ctx, &definition.name).await {
        // A stream without summaries lists no trace, so there is none to count.
        return Ok(Json(plan.counted(Vec::new(), true)));
    }

    // The list reads without a slot because it stops at a page. A count reads
    // the whole range, so it waits its turn among the queries of the role.
    let Some(_slot) = state
        .query
        .admission()
        .acquire_for_role(ctx.organization_role_key())
    else {
        return Ok(Json(plan.unavailable(Unavailable::Busy)));
    };

    let count = async {
        if context.filters.iter().any(TraceFilter::is_span_filter) {
            span_filter::count::bars(&state, &definition, &context, &plan).await
        } else {
            scan::count::bars(&state, &ctx.org_id, &definition.name, &context, &plan).await
        }
    };

    Ok(Json(match within_budget(count).await? {
        Some(Some(bars)) => plan.counted(bars, true),
        Some(None) => plan.unavailable(Unavailable::ScanLimit),
        None => plan.unavailable(Unavailable::TimeBudget),
    }))
}
