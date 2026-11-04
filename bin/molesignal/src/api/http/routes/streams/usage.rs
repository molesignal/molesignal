// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What each source took in, and how it stands against its budget.
//!
//! A figure that cannot be told is left out, never given as zero: while the rows cannot be read
//! nothing is known, and a source without rows has sent nothing only over hours that metering
//! covers.

use std::collections::HashMap;

use axum::{
    Extension, Json,
    extract::{Query, State},
};
use serde::{Deserialize, Serialize};

use crate::{
    api::AppState,
    app::{
        iam::IamContext,
        intake::usage::{BudgetStanding, SourceLabel, budget_standings},
    },
    domain::{
        iam::permission,
        intake::usage::{
            budget::{BudgetPeriod, IntakeBudget},
            source::{
                IntakeUsageBucket, SourceUsage, UsageDimension, UsageSelection, budgetable,
                hour_bucket_start, parse_stream_key,
            },
        },
    },
    shared::{Error, Result, time::TimestampMicros},
};

pub(super) mod budgets;

const DEFAULT_WINDOW_SECS: i64 = 24 * 60 * 60;
const MIN_WINDOW_SECS: i64 = 60 * 60;
const MAX_WINDOW_SECS: i64 = 31 * 24 * 60 * 60;
const DEFAULT_BUCKET_COUNT: usize = 24;
const MIN_BUCKET_COUNT: usize = 6;
const MAX_BUCKET_COUNT: usize = 48;
/// The most sources one answer lists; those with a budget are always among them.
const MAX_LISTED_SOURCES: usize = 5_000;
/// The most sources of a breakdown of one source.
const MAX_BREAKDOWN_SOURCES: usize = 20;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct UsageParams {
    dimension: Option<UsageDimension>,
    window_secs: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BudgetView {
    id: String,
    dimension: UsageDimension,
    source_key: String,
    period: BudgetPeriod,
    limit_size: u64,
    warn_percent: u8,
    notify_user_ids: Vec<String>,
    updated_at: i64,
    #[serde(flatten)]
    standing: BudgetStanding,
}

impl BudgetView {
    fn new(budget: &IntakeBudget, standing: BudgetStanding) -> Self {
        Self {
            id: budget.id.to_string(),
            dimension: budget.dimension,
            source_key: budget.source_key.clone(),
            period: budget.period,
            limit_size: budget.limit_size,
            warn_percent: budget.warn_percent,
            notify_user_ids: budget
                .notify_user_ids
                .iter()
                .map(ToString::to_string)
                .collect(),
            updated_at: budget.updated_at.0,
            standing,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceRow {
    source_key: String,
    label: SourceLabel,
    budgetable: bool,
    intake_bytes: Option<u64>,
    events: Option<u64>,
    /// Whether metering covers the whole window. When it does not, the figures are lower bounds.
    complete: bool,
    share: Option<f64>,
    budget: Option<BudgetView>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct UsageResponse {
    generated_at: i64,
    window_start: i64,
    window_end: i64,
    window_secs: i64,
    dimension: UsageDimension,
    /// False when the rows could not be read.
    available: bool,
    metering_started_at: Option<i64>,
    total_bytes: Option<u64>,
    total_events: Option<u64>,
    /// Sources were left out for being too many: those that took in the least.
    truncated: bool,
    sources: Vec<SourceRow>,
}

/// The window of a request, on hour boundaries, because that is how usage is kept.
fn window_of(window_secs: Option<i64>, now: TimestampMicros) -> (i64, i64) {
    let window_secs = window_secs
        .unwrap_or(DEFAULT_WINDOW_SECS)
        .clamp(MIN_WINDOW_SECS, MAX_WINDOW_SECS);

    let start = hour_bucket_start(now.0.saturating_sub(window_secs.saturating_mul(1_000_000)));

    (window_secs, start)
}

#[permission(any("intake_usage.read", "sys.telemetry.read"))]
pub(super) async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(params): Query<UsageParams>,
) -> Result<Json<UsageResponse>> {
    let dimension = params.dimension.unwrap_or(UsageDimension::Credential);
    let now = TimestampMicros::now();
    let (window_secs, start) = window_of(params.window_secs, now);

    let (first_bucket, rows) = tokio::join!(
        state.platform.usage.first_bucket(&ctx.org_id),
        state.platform.usage.by_source(
            dimension,
            UsageSelection {
                org_id: &ctx.org_id,
                start_micros: start,
                end_micros: now.0,
                source: None,
            },
        ),
    );

    let read = match (first_bucket, rows) {
        (Ok(first_bucket), Ok(rows)) => Some((first_bucket, rows)),
        (Err(error), _) | (_, Err(error)) => {
            tracing::warn!(org_id = %ctx.org_id, %error, "intake usage unavailable");
            None
        }
    };

    let budgets = state
        .platform
        .intake_budgets
        .list(&ctx.org_id)
        .await?
        .into_iter()
        .filter(|budget| budget.dimension == dimension)
        .collect::<Vec<_>>();

    let standings = standings_of(&state, &ctx, &budgets, now).await;
    let available = read.is_some();
    let (first_bucket, rows) = read.unwrap_or_default();
    let complete = first_bucket.is_some_and(|first| first <= start);
    let mut usage = rows
        .into_iter()
        .map(|row| (row.source_key.clone(), row))
        .collect::<HashMap<_, _>>();

    let total = usage.values().fold((0_u64, 0_u64), |(bytes, events), row| {
        (
            bytes.saturating_add(row.intake_bytes),
            events.saturating_add(row.events),
        )
    });

    // Without a single row and with metering not covering the window, nothing can be said.
    let total_known = available && (complete || !usage.is_empty());

    let mut keys = usage.keys().map(String::as_str).collect::<Vec<_>>();
    keys.extend(budgets.iter().map(|budget| budget.source_key.as_str()));

    let book = state
        .platform
        .intake_sources
        .book(&ctx.org_id, dimension, &keys)
        .await;

    let mut listed = usage.keys().cloned().collect::<Vec<_>>();
    listed.extend(budgets.iter().map(|budget| budget.source_key.clone()));
    listed.extend(book.existing.iter().cloned());
    listed.sort();
    listed.dedup();

    let mut budget_views = budgets
        .iter()
        .zip(standings)
        .map(|(budget, standing)| (budget.source_key.clone(), BudgetView::new(budget, standing)))
        .collect::<HashMap<_, _>>();

    let mut sources = listed
        .into_iter()
        .filter(|key| shown(&ctx, dimension, key))
        .map(|key| {
            let row = usage.remove(&key);

            let label = book
                .labels
                .get(&key)
                .cloned()
                .unwrap_or_else(|| fallback_label(dimension, &key));

            let budget = budget_views.remove(&key);
            let figures = figures_of(row.as_ref(), available, complete);

            SourceRow {
                budgetable: budgetable(dimension, &key) && !label.missing,
                source_key: key,
                label,
                intake_bytes: figures.0,
                events: figures.1,
                complete,
                share: figures
                    .0
                    .filter(|_| total.0 > 0)
                    .map(|bytes| bytes as f64 / total.0 as f64),
                budget,
            }
        })
        .collect::<Vec<_>>();

    sources.sort_by(|left, right| {
        right
            .intake_bytes
            .cmp(&left.intake_bytes)
            .then_with(|| left.label.name.cmp(&right.label.name))
    });

    let truncated = keep_most(&mut sources);

    Ok(Json(UsageResponse {
        generated_at: now.0,
        window_start: start,
        window_end: now.0,
        window_secs,
        dimension,
        available,
        metering_started_at: first_bucket,
        total_bytes: total_known.then_some(total.0),
        total_events: total_known.then_some(total.1),
        truncated,
        sources,
    }))
}

/// Whether the caller sees this source: `_audit` is there only for who may read it.
fn shown(ctx: &IamContext, dimension: UsageDimension, key: &str) -> bool {
    if dimension != UsageDimension::Stream {
        return true;
    }

    // A key that names no stream has no stream to hide.
    parse_stream_key(key).map_or(true, |(_, stream)| super::audit::shows(ctx, stream))
}

/// Refuses a source the caller does not see, asked about by its key.
fn refuse_unseen(ctx: &IamContext, dimension: UsageDimension, key: &str) -> Result<()> {
    if shown(ctx, dimension, key) {
        return Ok(());
    }

    Err(Error::forbidden("`_audit` needs the audit.read permission"))
}

/// The figures of a source: what its rows say, zero where it sent nothing over hours metering
/// covers, and nothing where that is not known.
fn figures_of(
    row: Option<&SourceUsage>,
    available: bool,
    complete: bool,
) -> (Option<u64>, Option<u64>) {
    match row {
        Some(row) => (Some(row.intake_bytes), Some(row.events)),
        None if available && complete => (Some(0), Some(0)),
        None => (None, None),
    }
}

/// Keeps the sources that took in most, and every one with a budget. Says whether any were left out.
fn keep_most(sources: &mut Vec<SourceRow>) -> bool {
    if sources.len() <= MAX_LISTED_SOURCES {
        return false;
    }

    let mut position = 0;

    sources.retain(|source| {
        position += 1;
        position <= MAX_LISTED_SOURCES || source.budget.is_some()
    });

    true
}

fn fallback_label(dimension: UsageDimension, key: &str) -> SourceLabel {
    SourceLabel {
        kind: match dimension {
            UsageDimension::Credential => "api_token",
            UsageDimension::Stream => "stream",
            UsageDimension::Service => "service",
        },
        subkind: None,
        name: key.to_string(),
        detail: None,
        missing: false,
    }
}

/// The standings, or unknown ones when the rows cannot be read: the budgets themselves still show.
async fn standings_of(
    state: &AppState,
    ctx: &IamContext,
    budgets: &[IntakeBudget],
    now: TimestampMicros,
) -> Vec<BudgetStanding> {
    match budget_standings(state.platform.usage.as_ref(), &ctx.org_id, budgets, now).await {
        Ok(standings) => standings,
        Err(error) => {
            tracing::warn!(org_id = %ctx.org_id, %error, "intake budget standings unavailable");

            budgets
                .iter()
                .map(|budget| BudgetStanding::unknown(budget, now))
                .collect()
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DetailParams {
    dimension: UsageDimension,
    source_key: String,
    window_secs: Option<i64>,
    bucket_count: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DetailBucket {
    start_time: i64,
    end_time: i64,
    intake_bytes: u64,
    events: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BreakdownRow {
    source_key: String,
    label: SourceLabel,
    intake_bytes: u64,
    events: u64,
}

#[derive(Debug, Serialize)]
struct Breakdown {
    dimension: UsageDimension,
    sources: Vec<BreakdownRow>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DetailResponse {
    generated_at: i64,
    window_start: i64,
    window_end: i64,
    window_secs: i64,
    metering_started_at: Option<i64>,
    intake_bytes: u64,
    events: u64,
    buckets: Vec<DetailBucket>,
    /// What the source is made of along the other two dimensions.
    breakdowns: Vec<Breakdown>,
}

/// One source over the window: its hours, and how it divides along the other dimensions.
#[permission(any("intake_usage.read", "sys.telemetry.read"))]
pub(super) async fn detail(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(params): Query<DetailParams>,
) -> Result<Json<DetailResponse>> {
    refuse_unseen(&ctx, params.dimension, &params.source_key)?;

    let now = TimestampMicros::now();
    let (window_secs, start) = window_of(params.window_secs, now);

    let bucket_count = params
        .bucket_count
        .unwrap_or(DEFAULT_BUCKET_COUNT)
        .clamp(MIN_BUCKET_COUNT, MAX_BUCKET_COUNT);

    let selection = UsageSelection {
        org_id: &ctx.org_id,
        start_micros: start,
        end_micros: now.0,
        source: Some((params.dimension, params.source_key.as_str())),
    };

    let hours = state.platform.usage.hourly(selection).await?;
    let first_bucket = state.platform.usage.first_bucket(&ctx.org_id).await?;
    let mut breakdowns = Vec::with_capacity(2);

    for dimension in UsageDimension::ALL {
        if dimension == params.dimension {
            continue;
        }

        let mut rows = state.platform.usage.by_source(dimension, selection).await?;
        rows.retain(|row| shown(&ctx, dimension, &row.source_key));
        rows.truncate(MAX_BREAKDOWN_SOURCES);

        let keys = rows
            .iter()
            .map(|row| row.source_key.as_str())
            .collect::<Vec<_>>();

        let book = state
            .platform
            .intake_sources
            .book(&ctx.org_id, dimension, &keys)
            .await;

        breakdowns.push(Breakdown {
            dimension,
            sources: rows
                .into_iter()
                .map(|row| BreakdownRow {
                    label: book
                        .labels
                        .get(&row.source_key)
                        .cloned()
                        .unwrap_or_else(|| fallback_label(dimension, &row.source_key)),
                    source_key: row.source_key,
                    intake_bytes: row.intake_bytes,
                    events: row.events,
                })
                .collect(),
        });
    }

    let (intake_bytes, events) = hours.iter().fold((0_u64, 0_u64), |(bytes, events), hour| {
        (
            bytes.saturating_add(hour.intake_bytes),
            events.saturating_add(hour.events),
        )
    });

    Ok(Json(DetailResponse {
        generated_at: now.0,
        window_start: start,
        window_end: now.0,
        window_secs,
        metering_started_at: first_bucket,
        intake_bytes,
        events,
        buckets: detail_buckets(&hours, start, now.0, bucket_count),
        breakdowns,
    }))
}

/// The hours gathered into `count` equal buckets over the window.
fn detail_buckets(
    hours: &[IntakeUsageBucket],
    start: i64,
    end: i64,
    count: usize,
) -> Vec<DetailBucket> {
    let duration = i128::from(end.saturating_sub(start).max(1));
    let edge = |index: usize| start + ((duration * index as i128) / count as i128) as i64;

    let mut buckets = (0..count)
        .map(|index| DetailBucket {
            start_time: edge(index),
            end_time: edge(index + 1),
            intake_bytes: 0,
            events: 0,
        })
        .collect::<Vec<_>>();

    for hour in hours {
        let offset = i128::from(hour.bucket_start_micros.saturating_sub(start).max(0));
        let index = ((offset * count as i128) / duration) as usize;

        if let Some(bucket) = buckets.get_mut(index.min(count - 1)) {
            bucket.intake_bytes = bucket.intake_bytes.saturating_add(hour.intake_bytes);
            bucket.events = bucket.events.saturating_add(hour.events);
        }
    }

    buckets
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: i64 = 3_600 * 1_000_000;

    fn row(key: &str, bytes: u64) -> SourceUsage {
        SourceUsage {
            source_key: key.into(),
            intake_bytes: bytes,
            events: 1,
        }
    }

    #[test]
    fn a_silent_source_is_zero_only_when_metering_covers_the_window() {
        assert_eq!(figures_of(None, true, true), (Some(0), Some(0)));
        assert_eq!(figures_of(None, true, false), (None, None));
        assert_eq!(figures_of(None, false, true), (None, None));
        assert_eq!(
            figures_of(Some(&row("a", 5)), true, false),
            (Some(5), Some(1))
        );
    }

    #[test]
    fn the_window_starts_on_the_hour() {
        let now = TimestampMicros(100 * HOUR + 123);
        let (secs, start) = window_of(Some(2 * 3_600), now);

        assert_eq!(secs, 7_200);
        assert_eq!(start, 98 * HOUR);

        assert_eq!(window_of(Some(1), now).0, MIN_WINDOW_SECS);
        assert_eq!(window_of(Some(i64::MAX), now).0, MAX_WINDOW_SECS);
        assert_eq!(window_of(None, now).0, DEFAULT_WINDOW_SECS);
    }

    #[test]
    fn hours_gather_into_their_buckets() {
        let hours = [
            IntakeUsageBucket {
                bucket_start_micros: 0,
                intake_bytes: 10,
                events: 1,
            },
            IntakeUsageBucket {
                bucket_start_micros: HOUR,
                intake_bytes: 20,
                events: 2,
            },
            IntakeUsageBucket {
                bucket_start_micros: 5 * HOUR,
                intake_bytes: 40,
                events: 4,
            },
        ];

        let buckets = detail_buckets(&hours, 0, 6 * HOUR, 3);

        assert_eq!(
            buckets
                .iter()
                .map(|bucket| bucket.intake_bytes)
                .collect::<Vec<_>>(),
            [30, 0, 40]
        );
        assert_eq!(buckets[0].start_time, 0);
        assert_eq!(buckets[2].end_time, 6 * HOUR);
    }

    #[test]
    fn being_too_many_keeps_the_largest_and_every_budget() {
        let source = |key: &str, budget: Option<BudgetView>| SourceRow {
            source_key: key.into(),
            label: fallback_label(UsageDimension::Service, key),
            budgetable: true,
            intake_bytes: Some(1),
            events: Some(1),
            complete: true,
            share: None,
            budget,
        };

        let budget = IntakeBudget {
            id: crate::shared::ids::Id::from_string("b"),
            org_id: crate::shared::ids::Id::from_string("o"),
            dimension: UsageDimension::Service,
            source_key: "last".into(),
            period: BudgetPeriod::Day,
            limit_size: 1,
            warn_percent: 80,
            notify_user_ids: Vec::new(),
            created_by: crate::shared::ids::Id::from_string("u"),
            updated_by: crate::shared::ids::Id::from_string("u"),
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        };

        let mut sources = (0..MAX_LISTED_SOURCES + 2)
            .map(|index| source(&index.to_string(), None))
            .collect::<Vec<_>>();

        sources.push(source(
            "last",
            Some(BudgetView::new(
                &budget,
                BudgetStanding::unknown(&budget, TimestampMicros(0)),
            )),
        ));

        assert!(keep_most(&mut sources));
        assert_eq!(sources.len(), MAX_LISTED_SOURCES + 1);
        assert_eq!(
            sources.last().map(|row| row.source_key.as_str()),
            Some("last")
        );

        let mut few = vec![source("a", None)];

        assert!(!keep_most(&mut few));
        assert_eq!(few.len(), 1);
    }
}
