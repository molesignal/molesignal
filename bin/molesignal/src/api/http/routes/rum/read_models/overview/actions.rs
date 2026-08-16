// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashMap;

use super::{
    super::ReadModelContext,
    model::{ActionAggregate, ExperienceBucket, SatisfactionCounts, SlowPage, lcp_grade},
};
use crate::{
    api::AppState,
    app::iam::IamContext,
    infra::rum::read_model::{RumActionRecord, RumReadModelReader, RumScope},
    shared::Result,
};

const BUCKET_COUNT: usize = 12;
const METRIC_COLUMNS: &[&str] = &[
    "_timestamp",
    "ts_micros",
    "session_id",
    "type",
    "application",
    "environment",
    "version",
    "country",
    "device",
    "lcp_ms",
    "inp_ms",
    "cls",
];
const INSIGHT_COLUMNS: &[&str] = &[
    "_timestamp",
    "ts_micros",
    "session_id",
    "type",
    "page",
    "url",
    "application",
    "environment",
    "version",
    "country",
    "device",
    "duration_ms",
    "status",
    "lcp_ms",
    "fid_ms",
    "inp_ms",
    "cls",
    "ttfb_ms",
];

#[derive(Clone, Copy)]
enum ActionProjection {
    Metrics,
    Insights,
}

#[derive(Default)]
struct SessionExperience {
    started_at: i64,
    failed: bool,
    rage: bool,
    dead: bool,
    crash: bool,
    slow: bool,
    lcp: Option<f64>,
    fid: Option<f64>,
    inp: Option<f64>,
    cls: Option<f64>,
    ttfb: Option<f64>,
}

#[derive(Default)]
struct PageSession {
    lcp: Option<f64>,
    failed: bool,
}

pub(super) async fn load_metrics(
    state: &AppState,
    iam: &IamContext,
    context: &ReadModelContext,
) -> Result<ActionAggregate> {
    load(state, iam, context, ActionProjection::Metrics).await
}

pub(super) async fn load_insights(
    state: &AppState,
    iam: &IamContext,
    context: &ReadModelContext,
) -> Result<ActionAggregate> {
    load(state, iam, context, ActionProjection::Insights).await
}

async fn load(
    state: &AppState,
    iam: &IamContext,
    context: &ReadModelContext,
    projection: ActionProjection,
) -> Result<ActionAggregate> {
    let reader = RumReadModelReader::new(
        state.storage.parquet_file_meta.clone(),
        state.storage.object_store.clone(),
    );
    let scope = RumScope {
        application: context.application.as_deref(),
        environment: context.environment.as_deref(),
        version: context.version.as_deref(),
        country: context.country.as_deref(),
        device: context.device.as_deref(),
    };
    let mut lcp = Vec::new();
    let mut inp = Vec::new();
    let mut cls = Vec::new();
    let mut sessions = HashMap::<String, SessionExperience>::new();
    let mut pages = HashMap::<(String, String), PageSession>::new();
    let columns = match projection {
        ActionProjection::Metrics => METRIC_COLUMNS,
        ActionProjection::Insights => INSIGHT_COLUMNS,
    };
    let stats = reader
        .visit_actions(&iam.org_id, context.time_range(), columns, |record| {
            if !record.matches_scope(scope) {
                return;
            }
            match projection {
                ActionProjection::Metrics => collect_metric(&record, &mut lcp, &mut inp, &mut cls),
                ActionProjection::Insights => {
                    collect_experience(&record, &mut sessions, &mut pages)
                }
            }
        })
        .await?;
    let aggregate = match projection {
        ActionProjection::Metrics => ActionAggregate {
            lcp_p75: percentile(&mut lcp, 0.75),
            inp_p75: percentile(&mut inp, 0.75),
            cls_p75: percentile(&mut cls, 0.75),
            ..empty(context)
        },
        ActionProjection::Insights => build_insights(context, sessions, pages),
    };
    tracing::debug!(
        scanned_files = stats.files,
        scanned_rows = stats.rows,
        "RUM overview action read model completed"
    );
    Ok(aggregate)
}

fn collect_metric(
    record: &RumActionRecord,
    lcp: &mut Vec<f64>,
    inp: &mut Vec<f64>,
    cls: &mut Vec<f64>,
) {
    if record.event_type != "view" {
        return;
    }
    push_finite(lcp, record.lcp_ms);
    push_finite(inp, record.inp_ms);
    push_finite(cls, record.cls);
}

fn collect_experience(
    record: &RumActionRecord,
    sessions: &mut HashMap<String, SessionExperience>,
    pages: &mut HashMap<(String, String), PageSession>,
) {
    if record.session_id.is_empty() {
        return;
    }
    let failed = record.status.unwrap_or_default() >= 400
        || matches!(record.event_type.as_str(), "error" | "network_error");
    let session = sessions
        .entry(record.session_id.clone())
        .or_insert_with(|| SessionExperience {
            started_at: record.timestamp_micros,
            ..SessionExperience::default()
        });
    session.started_at = session.started_at.min(record.timestamp_micros);
    session.failed |= failed;
    session.rage |= matches!(record.event_type.as_str(), "rage_click" | "rageclick");
    session.dead |= matches!(record.event_type.as_str(), "dead_click" | "deadclick");
    session.crash |= record.event_type == "crash";
    session.slow |=
        record.event_type == "resource" && record.duration_ms.unwrap_or_default() >= 1_000.0;
    maximize(&mut session.lcp, record.lcp_ms);
    maximize(&mut session.fid, record.fid_ms);
    maximize(&mut session.inp, record.inp_ms);
    maximize(&mut session.cls, record.cls);
    maximize(&mut session.ttfb, record.ttfb_ms);

    if record.event_type == "view"
        && let (Some(page), Some(lcp)) = (record.page_key(), record.lcp_ms)
        && lcp.is_finite()
    {
        let page = pages
            .entry((page.to_string(), record.session_id.clone()))
            .or_default();
        maximize(&mut page.lcp, Some(lcp));
        page.failed |= failed;
    }
}

fn build_insights(
    context: &ReadModelContext,
    sessions: HashMap<String, SessionExperience>,
    pages: HashMap<(String, String), PageSession>,
) -> ActionAggregate {
    let mut aggregate = empty(context);
    let range = (context.to - context.from).max(1);
    for session in sessions.into_values() {
        let experience = experience(&session);
        aggregate.satisfaction.total += 1;
        let bucket = ((session.started_at - context.from).max(0) as i128 * BUCKET_COUNT as i128
            / range as i128)
            .clamp(0, BUCKET_COUNT.saturating_sub(1) as i128) as usize;
        let point = &mut aggregate.trend[bucket];
        match experience {
            "poor" => {
                aggregate.satisfaction.poor += 1;
                point.poor += 1;
            }
            "needs_improvement" => {
                aggregate.satisfaction.needs_improvement += 1;
                point.needs += 1;
            }
            _ => {
                aggregate.satisfaction.good += 1;
                point.good += 1;
            }
        }
    }

    let mut by_page = HashMap::<String, (Vec<f64>, i64)>::new();
    for ((page, _), session) in pages {
        let Some(lcp) = session.lcp else {
            continue;
        };
        let item = by_page.entry(page).or_default();
        item.0.push(lcp);
        item.1 += i64::from(session.failed);
    }
    aggregate.slow_pages = by_page
        .into_iter()
        .map(|(page, (mut values, failed))| {
            let sessions = i64::try_from(values.len()).unwrap_or(i64::MAX);
            let p75 = percentile(&mut values, 0.75);
            SlowPage {
                page,
                p75,
                sessions,
                error_rate: if sessions == 0 {
                    0.0
                } else {
                    failed as f64 / sessions as f64
                },
                grade: lcp_grade(p75),
            }
        })
        .collect();
    aggregate
        .slow_pages
        .sort_by(|left, right| right.p75.total_cmp(&left.p75));
    aggregate.slow_pages.truncate(5);
    aggregate
}

fn experience(session: &SessionExperience) -> &'static str {
    if session.failed
        || session.rage
        || session.dead
        || session.crash
        || session.lcp.is_some_and(|value| value > 4_000.0)
        || session.fid.is_some_and(|value| value > 300.0)
        || session.inp.is_some_and(|value| value > 500.0)
        || session.cls.is_some_and(|value| value > 0.25)
        || session.ttfb.is_some_and(|value| value > 1_800.0)
    {
        "poor"
    } else if session.slow
        || session.lcp.is_some_and(|value| value > 2_500.0)
        || session.fid.is_some_and(|value| value > 100.0)
        || session.inp.is_some_and(|value| value > 200.0)
        || session.cls.is_some_and(|value| value > 0.1)
        || session.ttfb.is_some_and(|value| value > 800.0)
    {
        "needs_improvement"
    } else {
        "good"
    }
}

fn maximize(target: &mut Option<f64>, value: Option<f64>) {
    if let Some(value) = value.filter(|value| value.is_finite()) {
        *target = Some(target.map_or(value, |current| current.max(value)));
    }
}

fn push_finite(target: &mut Vec<f64>, value: Option<f64>) {
    if let Some(value) = value.filter(|value| value.is_finite()) {
        target.push(value);
    }
}

fn percentile(values: &mut [f64], quantile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    let index = ((values.len() as f64 * quantile).ceil() as usize)
        .saturating_sub(1)
        .min(values.len() - 1);
    values[index]
}

fn empty(context: &ReadModelContext) -> ActionAggregate {
    let width = ((context.to - context.from) / BUCKET_COUNT as i64).max(1);
    ActionAggregate {
        trend: (0..BUCKET_COUNT)
            .map(|index| ExperienceBucket {
                start: context.from + width * index as i64,
                ..ExperienceBucket::default()
            })
            .collect(),
        satisfaction: SatisfactionCounts::default(),
        ..ActionAggregate::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poor_behavior_takes_priority_over_vitals() {
        let session = SessionExperience {
            rage: true,
            lcp: Some(1_000.0),
            ..SessionExperience::default()
        };
        assert_eq!(experience(&session), "poor");
    }

    #[test]
    fn percentile_uses_nearest_rank() {
        assert_eq!(percentile(&mut [4.0, 1.0, 3.0, 2.0], 0.75), 3.0);
    }
}
