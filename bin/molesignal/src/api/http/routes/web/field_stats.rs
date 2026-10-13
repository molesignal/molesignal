// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What a field holds across the rows a query matches: how many have a value,
//! how many different values there are, the commonest of them, and for a
//! number what it runs from and to and how it is spread.
//!
//! An explorer lists a page. Its field list can count a field among the rows
//! of that page, which says little of a range that holds a thousand pages.
//! This reads every row the same conditions select, so the numbers are of the
//! whole answer.
//!
//! It is asked in up to four statements, one after the other: the totals, the
//! commonest values, how the five commonest are spread over time, and for a
//! number the bars of its distribution. The later ones are written with what
//! the earlier found, and each runs as a tracked query, so a field that is
//! masked comes back masked.

use std::future::Future;

use serde::Serialize;

use self::{bins::BinPlan, reading::Bars, trend::TrendPlan};
use super::histogram::within_budget;
use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{
        query::{QueryLanguage, QueryRequest, QueryResult, StreamHint},
        stream::{FieldType, StreamType},
    },
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

mod bins;
mod reading;
mod statements;
#[cfg(test)]
mod tests;
mod trend;

/// How many of the commonest values are listed.
const TOP_VALUES: usize = 10;
/// How many of them are followed over time: as many lines as a small chart keeps apart.
const TREND_VALUES: usize = 5;
/// More different values than this, most of them met once, have no commonest
/// ones worth a pass over every row to find.
const DISTINCT_WORTH_RANKING: u64 = 10_000;

/// The rows a field is read from: a stream, a time range, and the conditions
/// the explorer's own list selects with.
pub(super) struct Scope {
    pub(super) stream: String,
    pub(super) stream_type: StreamType,
    pub(super) from: i64,
    pub(super) to: i64,
    /// Conditions every row read has to meet. Never empty: a statement with no
    /// condition would read the stream whole.
    pub(super) clauses: Vec<String>,
}

/// What a field's values are, which decides what can be said of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum FieldKind {
    /// Measured: it has an average and a spread.
    Number,
    /// A moment: it has an earliest and a latest, and no average.
    Time,
    Text,
    Bool,
}

impl FieldKind {
    /// `None` for a JSON field: a document per row has no values to rank or measure.
    pub(super) fn of(field_type: FieldType) -> Option<Self> {
        match field_type {
            FieldType::Int64 | FieldType::Float64 => Some(Self::Number),
            FieldType::Timestamp => Some(Self::Time),
            FieldType::Utf8 => Some(Self::Text),
            FieldType::Bool => Some(Self::Bool),
            FieldType::Json => None,
        }
    }
}

/// The field that is read, as the stream declares it.
pub(super) struct Analysed<'a> {
    pub(super) name: &'a str,
    pub(super) kind: FieldKind,
    /// Its values are whole numbers, so the bars of its distribution are too.
    pub(super) whole: bool,
}

impl<'a> Analysed<'a> {
    pub(super) fn new(name: &'a str, field_type: FieldType) -> Result<Self> {
        let kind = FieldKind::of(field_type).ok_or_else(|| {
            Error::invalid(format!(
                "field `{name}` holds JSON documents; analyse a path of it in SQL instead"
            ))
        })?;

        Ok(Self {
            name,
            kind,
            whole: field_type == FieldType::Int64,
        })
    }
}

/// Why a field has no analysis. The client words the reason for its reader.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Unavailable {
    /// Reading took longer than the time budget.
    TimeBudget,
    /// Too many queries are running for this role.
    Busy,
    /// The field is masked for this reader, or encrypted at rest: its values
    /// are not theirs to count.
    FieldProtected,
}

/// From what to what the values run, as written: a count of nanoseconds is
/// past what a JSON number holds exactly.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub(super) struct ValueRange {
    pub(super) min: String,
    pub(super) max: String,
}

/// How a number is spread. A statistic is absent where the values do not have
/// one: a spread needs two values, a shape needs values that differ.
#[derive(Debug, Default, PartialEq, Serialize)]
pub(super) struct Spread {
    pub(super) avg: Option<f64>,
    /// The quartiles are estimates: they are read in one pass, from a digest.
    pub(super) q1: Option<f64>,
    pub(super) median: Option<f64>,
    pub(super) q3: Option<f64>,
    pub(super) stddev_samp: Option<f64>,
    pub(super) stddev_pop: Option<f64>,
    /// Of the population: the third standardised moment.
    pub(super) skewness: Option<f64>,
    /// Of the population, in excess of a normal distribution's 3.
    pub(super) kurtosis: Option<f64>,
}

/// The values counted into bars of one width, the first beginning at `origin`.
#[derive(Debug, PartialEq, Serialize)]
pub(super) struct ValueBars {
    pub(super) origin: f64,
    pub(super) width: f64,
    pub(super) counts: Vec<u64>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(super) struct TopValue {
    pub(super) value: String,
    pub(super) count: u64,
}

/// How many rows hold one value in each stretch of a trend.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub(super) struct TrendSeries {
    pub(super) value: String,
    pub(super) counts: Vec<u64>,
}

/// The commonest values over time: stretches `bucket_micros` long, the first
/// beginning at `origin_micros`, and a count for every stretch of each value.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub(super) struct ValueTrend {
    origin_micros: i64,
    bucket_micros: i64,
    series: Vec<TrendSeries>,
}

#[derive(Debug, PartialEq, Serialize)]
pub(super) struct FieldStatsResponse {
    kind: FieldKind,
    /// The rows the query matches.
    total: u64,
    /// Those of them that have a value for the field.
    present: u64,
    distinct: u64,
    /// Whether `distinct` was counted. Otherwise it is an estimate.
    distinct_exact: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    range: Option<ValueRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    spread: Option<Spread>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bars: Option<ValueBars>,
    /// The commonest values, commonest first. Empty when `top_skipped`.
    top: Vec<TopValue>,
    /// The values are nearly all different, so none is commoner than the rest.
    top_skipped: bool,
    /// Absent where no value is commoner than the rest, and where time is masked.
    #[serde(skip_serializing_if = "Option::is_none")]
    trend: Option<ValueTrend>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unavailable: Option<Unavailable>,
}

impl FieldStatsResponse {
    /// A field among no rows: a stream nothing was written to matches nothing.
    pub(super) fn empty(kind: FieldKind) -> Self {
        Self {
            kind,
            total: 0,
            present: 0,
            distinct: 0,
            distinct_exact: true,
            range: None,
            spread: None,
            bars: None,
            top: Vec::new(),
            top_skipped: false,
            trend: None,
            unavailable: None,
        }
    }

    pub(super) fn unavailable(kind: FieldKind, reason: Unavailable) -> Self {
        Self {
            unavailable: Some(reason),
            ..Self::empty(kind)
        }
    }
}

/// Reads `field` across the rows of `scope`, inside the time budget of an
/// explorer's count. An analysis that cannot be had says why instead of
/// failing: the client then falls back to the rows it has loaded.
pub(super) async fn analyze(
    state: &AppState,
    iam: &IamContext,
    scope: &Scope,
    field: &Analysed<'_>,
) -> Result<FieldStatsResponse> {
    let run = |statement: String| {
        // The same path as a query a user writes: admitted by role, listed
        // among the running queries, masked, and cached once its range is past.
        state.query.run_tracked(
            QueryRequest {
                org_id: iam.org_id.clone(),
                language: QueryLanguage::Sql,
                statement,
                time_range: TimeRange::new(TimestampMicros(scope.from), TimestampMicros(scope.to)),
                stream: Some(StreamHint {
                    name: scope.stream.clone(),
                    stream_type: scope.stream_type,
                }),
                limit: None,
                federation_clusters: Vec::new(),
            },
            iam.user_id.clone(),
            iam.organization_role_key(),
        )
    };

    match within_budget(read(scope, field, run)).await {
        Ok(Some(response)) => Ok(response),
        Ok(None) => Ok(FieldStatsResponse::unavailable(
            field.kind,
            Unavailable::TimeBudget,
        )),
        Err(Error::ResourceExhausted(_)) => Ok(FieldStatsResponse::unavailable(
            field.kind,
            Unavailable::Busy,
        )),
        Err(error) => Err(error),
    }
}

async fn read<Run, Running>(
    scope: &Scope,
    field: &Analysed<'_>,
    run: Run,
) -> Result<FieldStatsResponse>
where
    Run: Fn(String) -> Running,
    Running: Future<Output = Result<QueryResult>>,
{
    let totals = run(statements::summary(scope, field)).await?;

    // Counts that came back masked are not this reader's: nothing more is asked.
    let Some(summary) = reading::summary(&totals.columns, &totals.rows, field.kind)? else {
        return Ok(FieldStatsResponse::unavailable(
            field.kind,
            Unavailable::FieldProtected,
        ));
    };

    let mut response = FieldStatsResponse {
        total: summary.total,
        present: summary.present,
        distinct: summary.distinct,
        distinct_exact: false,
        range: summary.range,
        spread: summary.spread,
        ..FieldStatsResponse::empty(field.kind)
    };

    if response.present == 0 {
        response.distinct = 0;
        response.distinct_exact = true;
        return Ok(response);
    }

    // Values met about once each have no commonest: ranking them would group
    // every row for a list of ones.
    response.top_skipped = response.distinct > DISTINCT_WORTH_RANKING
        && response.distinct.saturating_mul(2) >= response.present;

    if !response.top_skipped {
        let ranked = run(statements::top_values(scope, field, TOP_VALUES + 1)).await?;
        let mut top = reading::top_values(&ranked.columns, &ranked.rows)?;

        // Fewer values than were asked for are all there are: counted, not estimated.
        if top.len() <= TOP_VALUES {
            response.distinct = top.len() as u64;
            response.distinct_exact = true;
        }

        top.truncate(TOP_VALUES);
        response.top = top;
    }

    // Values met once each have no trend: every line would lie along the axis.
    let repeated = response.top.first().is_some_and(|top| top.count > 1);

    if let Some(plan) = TrendPlan::new(scope.from, scope.to).filter(|_| repeated) {
        let followed = response
            .top
            .iter()
            .take(TREND_VALUES)
            .map(|top| top.value.as_str())
            .collect::<Vec<_>>();

        let counted = run(statements::trend(scope, field, &plan, &followed)).await?;

        response.trend =
            reading::trend(&counted.columns, &counted.rows, &followed, plan.stretches())?.map(
                |series| ValueTrend {
                    origin_micros: plan.origin(),
                    bucket_micros: plan.stretch(),
                    series,
                },
            );
    }

    let plan = summary
        .bounds
        .and_then(|(min, max)| BinPlan::new(min, max, field.whole));

    if let (Some(plan), Some(mean)) = (plan, response.spread.as_ref().and_then(|s| s.avg)) {
        let counted = run(statements::bars(scope, field, &plan, mean)).await?;
        let Bars { counts, moments } = reading::bars(&counted.columns, &counted.rows, plan.bins())?;

        if let Some(spread) = response.spread.as_mut() {
            spread.skewness = moments.skewness();
            spread.kurtosis = moments.excess_kurtosis();
        }

        response.bars = Some(ValueBars {
            origin: plan.origin(),
            width: plan.width(),
            counts,
        });
    }

    Ok(response)
}
