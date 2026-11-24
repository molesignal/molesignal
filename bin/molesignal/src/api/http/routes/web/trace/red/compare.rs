// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! How the chosen requests of a trace query differ from the rest, field by field: those that
//! took a stretch of time picked on the latency chart, the failed ones, or both, against the
//! others the query matches. Each set is read as a sample of its first rows, and the answer
//! says how many each holds.

use axum::{
    Extension, Json,
    extract::{Query, State},
};
use domain::iam::{IamContext, permission};
use kernel::Result;
use serde::Serialize;

use super::{RedScope, RedUnavailable, TraceRedQuery, run_tracked, settled};
use crate::{
    api::{
        AppState,
        http::routes::web::{histogram::within_budget, trace::resolve_traces_stream_definition},
    },
    app::attribute_diff::{Comparison, Selection, SpanComparison, compare_spans},
};

/// The most rows read of each set.
const SAMPLE_ROWS: usize = 1_000;

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in super::super) struct TraceRedCompareResponse {
    scope: RedScope,
    #[serde(flatten)]
    comparison: Comparison,
    #[serde(skip_serializing_if = "Option::is_none")]
    unavailable: Option<RedUnavailable>,
}

#[permission(any("traces.read", "sys.telemetry.read"))]
pub(in super::super) async fn compare(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<TraceRedQuery>,
) -> Result<Json<TraceRedCompareResponse>> {
    let mut query = query;
    super::super::expression::expand(
        &mut query.expression,
        &mut query.q,
        &mut query.filters,
        &mut query.condition,
    )?;

    let plan = query.plan()?;

    let selection = Selection {
        min_duration: query.min_duration,
        max_duration: query.max_duration,
        errors: query.errors.unwrap_or(false),
    };

    let mut response = TraceRedCompareResponse {
        scope: query.scope,
        comparison: Comparison {
            selected: 0,
            baseline: 0,
            sampled: false,
            dimensions: Vec::new(),
        },
        unavailable: None,
    };

    selection.check()?;

    let Some(definition) = resolve_traces_stream_definition(&state, &ctx.org_id).await else {
        return Ok(Json(response));
    };

    // Checked before reading, so a choice the stream cannot tell is the request's fault.
    selection.clause(&definition.schema)?;

    let source = query.source(&definition)?;
    let run = |statement| run_tracked(&state, &ctx, &definition.name, &plan, statement);

    let asked = SpanComparison {
        definition: &definition,
        clauses: source.conditions().to_vec(),
        selection: &selection,
        limit: SAMPLE_ROWS,
    };

    match settled(within_budget(compare_spans(asked, run)).await)? {
        Ok(comparison) => response.comparison = comparison,
        Err(reason) => response.unavailable = Some(reason),
    }

    Ok(Json(response))
}
