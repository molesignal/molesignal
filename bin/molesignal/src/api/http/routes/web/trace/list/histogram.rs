// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! How many traces a list query matches, bar by bar of the explorer histogram.
//!
//! The list returns a page. This counts every trace the same query matches in
//! its time range, by when the trace began, the traces with a failed Span
//! apart from the rest, and how long the traces of each bar took at the
//! median and the 95th and 99th percentile. It reads what the list reads: the
//! summaries, and the Spans when a filter is on a field of a Span. Bars counted
//! before are kept (see [`bars`]); only the rest are counted.

use axum::{
    Extension, Json,
    extract::{Query, State},
};
use serde::Deserialize;

use super::{
    MAX_TRACE_FILTERS, TraceListContext, TraceListSort, clean_input, filter, projection, scan,
    span_filter,
};
use crate::{
    api::{
        AppState,
        http::routes::web::{
            histogram::{BucketPlan, HistogramResponse, Unavailable, bars, within_budget},
            trace::resolve_traces_stream_definition,
        },
    },
    app::iam::IamContext,
    domain::iam::permission,
    shared::{Result, time::TimestampMicros},
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(in super::super) struct TraceHistogramQuery {
    from: Option<i64>,
    to: Option<i64>,
    q: Option<String>,
    filters: Option<String>,
    #[serde(rename = "where")]
    condition: Option<String>,
    /// The field query as text, read as the Traces page reads it; it stands
    /// for `q`, `filters` and `where`.
    expression: Option<String>,
    bucket_time: Option<i64>,
    origin_time: Option<i64>,
}

#[permission(any("traces.read", "sys.telemetry.read"))]
pub(in super::super) async fn histogram(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<TraceHistogramQuery>,
) -> Result<Json<HistogramResponse>> {
    let mut query = query;
    super::super::expression::expand(
        &mut query.expression,
        &mut query.q,
        &mut query.filters,
        &mut query.condition,
    )?;

    let plan = BucketPlan::new(query.from, query.to, query.bucket_time, query.origin_time)?;

    let Some(definition) = resolve_traces_stream_definition(&state, &ctx.org_id).await else {
        return Ok(Json(plan.counted(Vec::new(), true)));
    };

    let filters = filter::parse(
        query.filters.as_deref(),
        &definition.schema,
        MAX_TRACE_FILTERS,
    )?;

    let condition = filter::condition::parse(
        query.condition.as_deref(),
        &definition.schema,
        MAX_TRACE_FILTERS - filters.len(),
    )?;

    // The query the list would page through, without a page.
    let context = TraceListContext {
        from: plan.from,
        to: plan.to,
        sort: TraceListSort::default(),
        page_size: 1,
        q: query.q.as_deref().and_then(clean_input),
        filters,
        condition,
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

    let layout = bars::Layout::of(&plan);

    // The query without its range: the bars of every range of it are the same bars.
    let query_key = format!(
        "traces\0{}\0{}\0{}\0{}\0{}\0{}",
        definition.name,
        context.q.as_deref().unwrap_or_default(),
        serde_json::to_string(&context.filters).unwrap_or_default(),
        serde_json::to_string(&context.condition).unwrap_or_default(),
        layout.width,
        layout.origin.rem_euclid(layout.width),
    );

    let key = bars::key(ctx.org_id.as_str(), ctx.organization_role_key(), &query_key);

    let known = match bars::cache() {
        Some(cache) => cache.get(&key).await,
        None => None,
    };

    let gaps = bars::gaps(&layout, known.as_deref());
    let span_filtered = context.query().has_span_filter();

    let count = async {
        let mut counted = Vec::with_capacity(gaps.len());

        for gap in &gaps {
            let gap_plan = layout.plan(gap)?;

            let gap_context = TraceListContext {
                from: gap.from,
                to: gap.to,
                ..context.clone()
            };

            let found = if span_filtered {
                span_filter::count::bars(&state, &definition, &gap_context, &gap_plan).await?
            } else {
                scan::count::bars(
                    &state,
                    &ctx.org_id,
                    &definition.name,
                    &gap_context,
                    &gap_plan,
                )
                .await?
            };

            // The gap holds more summaries than one query may read.
            let Some((found, latency)) = found else {
                return Ok(None);
            };

            counted.push((*gap, bars::by_start(&layout, &found, &latency)));
        }

        Ok(Some(counted))
    };

    Ok(Json(match within_budget(count).await? {
        Some(Some(counted)) => {
            let assembled = bars::assemble(
                &layout,
                &counted,
                known.as_deref(),
                TimestampMicros::now().0,
            );

            let response =
                plan.counted_with_latency(assembled.bars.clone(), true, &assembled.latency);

            bars::store(key, known.as_deref(), true, assembled).await;
            response
        }
        Some(None) => plan.unavailable(Unavailable::ScanLimit),
        None => plan.unavailable(Unavailable::TimeBudget),
    }))
}
