// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! RED figures of the requests a trace query matches: how many there were
//! (rate), how many of them failed (errors) and how long they took
//! (duration), over the queried range and bar by bar of it, and broken down
//! by the values of a field.
//!
//! The trace list counts traces; RED counts requests, which are Spans. By
//! default those where a service took a request in (a server or consumer
//! Span, or the root of a trace), every Span when asked. A filter on a field
//! of a Span holds for the Span itself, so `service.name = api` reads the
//! requests `api` served. Of the fields the list compares on the trace,
//! `trace_id` and `duration_ns` are read of the Span itself, the span and
//! error counts of the trace it belongs to.
//!
//! Every figure is read with a statement that runs as a tracked query:
//! admitted by role, listed among the running queries, masked like any other
//! query and cached once its range is past.

use axum::{
    Extension, Json,
    extract::{Query, State},
};
use serde::{Deserialize, Serialize};

use self::{
    reading::{Bucket, Figures, Group, LatencyBar},
    statements::{Grouping, Source},
};
use super::{
    filter::{self, TraceCondition},
    list::{MAX_TRACE_FILTERS, clean_input},
    resolve_traces_stream_definition,
};
use crate::{
    api::{
        AppState,
        http::routes::web::histogram::{BucketPlan, within_budget},
    },
    app::iam::IamContext,
    domain::{
        iam::permission,
        query::{QueryLanguage, QueryRequest, QueryResult, StreamHint},
        stream::{StreamDefinition, StreamType},
    },
    infra::query::scan_limit::is_over_scan_limit,
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

mod reading;
pub(super) mod statements;
#[cfg(test)]
mod tests;

/// Which Spans are counted as requests.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum RedScope {
    /// Where a service took a request in: a server or consumer Span, or the
    /// root of a trace.
    #[default]
    Entry,
    /// Every Span.
    All,
}

/// Why there are no figures. The client words the reason for its reader.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum RedUnavailable {
    /// Reading took longer than the time budget.
    TimeBudget,
    /// Too many queries are running for this role.
    Busy,
    /// The range holds more Spans than one query may read.
    ScanLimit,
    /// Time is masked: there is nothing to lay the bars out along.
    TimestampMasked,
    /// The field is masked for this reader, or encrypted at rest.
    FieldProtected,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TraceRedQuery {
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
    #[serde(default)]
    scope: RedScope,
    /// The field a breakdown groups the requests by.
    by: Option<String>,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TraceRedResponse {
    bucket_time: i64,
    origin_time: i64,
    scope: RedScope,
    /// The figures of the whole range.
    total: Figures,
    /// The bars that hold a request, oldest first.
    buckets: Vec<Bucket>,
    /// The requests by how long they took: the bars that hold one, shortest first.
    latency: Vec<LatencyBar>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unavailable: Option<RedUnavailable>,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TraceRedBreakdownResponse {
    field: String,
    bucket_time: i64,
    origin_time: i64,
    scope: RedScope,
    /// The values the requests have, the commonest first.
    groups: Vec<Group>,
    /// More values than are listed.
    more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    unavailable: Option<RedUnavailable>,
}

impl TraceRedQuery {
    fn plan(&self) -> Result<BucketPlan> {
        BucketPlan::new(self.from, self.to, self.bucket_time, self.origin_time)
    }

    /// The requests of the scope that pass the list's own filters, free text
    /// and condition tree.
    fn source(&self, definition: &StreamDefinition) -> Result<Source> {
        let filters = filter::parse(
            self.filters.as_deref(),
            &definition.schema,
            MAX_TRACE_FILTERS,
        )?;

        let condition = filter::condition::parse(
            self.condition.as_deref(),
            &definition.schema,
            MAX_TRACE_FILTERS - filters.len(),
        )?;

        let text = self.q.as_deref().and_then(clean_input);
        let query = TraceCondition::of(&filters, text.as_deref(), condition.as_ref());

        Source::new(definition, self.scope, &query)
    }
}

#[permission(any("traces.read", "sys.telemetry.read"))]
pub(super) async fn red(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<TraceRedQuery>,
) -> Result<Json<TraceRedResponse>> {
    let mut query = query;
    super::expression::expand(
        &mut query.expression,
        &mut query.q,
        &mut query.filters,
        &mut query.condition,
    )?;

    let plan = query.plan()?;

    let mut response = TraceRedResponse {
        bucket_time: plan.bucket_micros(),
        origin_time: plan.origin_micros(),
        scope: query.scope,
        total: Figures::default(),
        buckets: Vec::new(),
        latency: Vec::new(),
        unavailable: None,
    };

    let Some(definition) = resolve_traces_stream_definition(&state, &ctx.org_id).await else {
        return Ok(Json(response));
    };

    let source = query.source(&definition)?;
    let run = |statement| run_tracked(&state, &ctx, &definition.name, &plan, statement);

    match settled(within_budget(reading::overview(&source, &plan, run)).await)? {
        Ok(overview) => {
            response.total = overview.total;
            response.latency = overview.latency;

            match overview.buckets {
                Some(buckets) => response.buckets = buckets,
                None => response.unavailable = Some(RedUnavailable::TimestampMasked),
            }
        }
        Err(reason) => response.unavailable = Some(reason),
    }

    Ok(Json(response))
}

#[permission(any("traces.read", "sys.telemetry.read"))]
pub(super) async fn breakdown(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<TraceRedQuery>,
) -> Result<Json<TraceRedBreakdownResponse>> {
    let mut query = query;
    super::expression::expand(
        &mut query.expression,
        &mut query.q,
        &mut query.filters,
        &mut query.condition,
    )?;

    let plan = query.plan()?;

    let field = query
        .by
        .as_deref()
        .map(str::trim)
        .filter(|field| !field.is_empty() && field.len() <= 128)
        .ok_or_else(|| Error::invalid("a breakdown needs the field to group requests by"))?
        .to_string();

    let mut response = TraceRedBreakdownResponse {
        field: field.clone(),
        bucket_time: plan.bucket_micros(),
        origin_time: plan.origin_micros(),
        scope: query.scope,
        groups: Vec::new(),
        more: false,
        unavailable: None,
    };

    let Some(definition) = resolve_traces_stream_definition(&state, &ctx.org_id).await else {
        return Ok(Json(response));
    };

    // The summaries' own fields are not on a Span: every request would be without a value.
    let declared = definition
        .schema
        .fields
        .iter()
        .filter(|candidate| !candidate.name.starts_with("molesignal.trace."))
        .find(|candidate| candidate.name == field)
        .ok_or_else(|| Error::invalid(format!("unknown trace field: {field}")))?;

    // An encrypted field is stored as ciphertext: its groups would be of that.
    if declared.encrypted {
        response.unavailable = Some(RedUnavailable::FieldProtected);
        return Ok(Json(response));
    }

    let grouping = Grouping::new(declared)
        .ok_or_else(|| Error::invalid(format!("trace field `{field}` cannot group requests")))?;

    let source = query.source(&definition)?;
    let run = |statement| run_tracked(&state, &ctx, &definition.name, &plan, statement);

    match settled(within_budget(reading::breakdown(&source, &grouping, &plan, run)).await)? {
        Ok(Some(breakdown)) => {
            response.groups = breakdown.groups;
            response.more = breakdown.more;
        }
        Ok(None) => response.unavailable = Some(RedUnavailable::FieldProtected),
        Err(reason) => response.unavailable = Some(reason),
    }

    Ok(Json(response))
}

/// Runs a statement the way a query a user writes runs: admitted by role,
/// listed among the running queries, masked, and cached once its range is past.
pub(super) async fn run_tracked(
    state: &AppState,
    ctx: &IamContext,
    stream: &str,
    plan: &BucketPlan,
    statement: String,
) -> Result<QueryResult> {
    let request = QueryRequest {
        org_id: ctx.org_id.clone(),
        language: QueryLanguage::Sql,
        statement,
        time_range: TimeRange::new(TimestampMicros(plan.from), TimestampMicros(plan.to)),
        stream: Some(StreamHint {
            name: stream.to_string(),
            stream_type: StreamType::TRACES,
        }),
        limit: None,
        step: None,
        federation_clusters: Vec::new(),
    };

    state
        .query
        .run_tracked(request, ctx.user_id.clone(), ctx.organization_role_key())
        .await
}

/// A read's answer, or why there is none: it took longer than the budget, too
/// many queries are running, or it would read more Spans than one query may.
/// Any other failure is the request's.
pub(super) fn settled<T>(
    outcome: Result<Option<T>>,
) -> Result<std::result::Result<T, RedUnavailable>> {
    match outcome {
        Ok(Some(answer)) => Ok(Ok(answer)),
        Ok(None) => Ok(Err(RedUnavailable::TimeBudget)),
        Err(Error::ResourceExhausted(_)) => Ok(Err(RedUnavailable::Busy)),
        Err(error) if is_over_scan_limit(&error) => Ok(Err(RedUnavailable::ScanLimit)),
        Err(error) => Err(error),
    }
}
