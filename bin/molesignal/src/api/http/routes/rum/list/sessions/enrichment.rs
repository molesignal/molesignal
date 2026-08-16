// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value, json};

use super::{SessionRow, action_range};
use crate::{
    api::AppState,
    app::iam::IamContext,
    infra::rum::read_model::{RumActionRecord, RumReadModelReader},
    shared::Result,
};

const ACTION_COLUMNS: &[&str] = &[
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
    "browser",
    "device",
    "os",
    "duration_ms",
    "status",
    "lcp_ms",
    "fid_ms",
    "inp_ms",
    "cls",
    "ttfb_ms",
];

#[derive(Default)]
struct ActionSummary {
    has_actions: bool,
    journey: Vec<(i64, String)>,
    fallback_page: Option<(i64, String)>,
    rage_clicks: i64,
    dead_clicks: i64,
    slow_resources: i64,
    failed_requests: i64,
    crashes: i64,
    lcp: Option<f64>,
    fid: Option<f64>,
    inp: Option<f64>,
    cls: Option<f64>,
    ttfb: Option<f64>,
    application: Option<String>,
    environment: Option<String>,
    version: Option<String>,
    country: Option<String>,
    browser: Option<String>,
    device: Option<String>,
    os: Option<String>,
}

pub(super) async fn enrich_rows(
    state: &AppState,
    iam: &IamContext,
    reader: &RumReadModelReader,
    rows: &mut [SessionRow],
) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let ids = rows
        .iter()
        .map(|row| row.session_id.clone())
        .collect::<HashSet<_>>();
    let session_ids = ids.iter().cloned().collect::<Vec<_>>();
    let range = action_range(rows);
    let mut actions = HashMap::<String, ActionSummary>::new();
    let (action_stats, replay_ids) = tokio::try_join!(
        reader.visit_actions_for_sessions(&iam.org_id, range, &ids, ACTION_COLUMNS, |record| {
            collect_action(&mut actions, record);
        },),
        state
            .telemetry
            .rum_replay
            .existing_session_ids(&iam.org_id, &session_ids),
    )?;
    tracing::debug!(
        scanned_files = action_stats.files,
        scanned_rows = action_stats.rows,
        selected_sessions = ids.len(),
        "RUM session action enrichment completed"
    );
    for row in rows {
        let summary = actions.remove(&row.session_id).unwrap_or_default();
        apply_summary(&mut row.item, summary);
        row.item.insert(
            "replay_available".into(),
            Value::Bool(replay_ids.contains(&row.session_id)),
        );
    }
    Ok(())
}

fn collect_action(actions: &mut HashMap<String, ActionSummary>, record: RumActionRecord) {
    let page = record.page_key().map(str::to_string);
    let summary = actions.entry(record.session_id).or_default();
    summary.has_actions = true;
    if let Some(page) = page {
        if record.event_type == "view" {
            summary.journey.push((record.timestamp_micros, page));
        } else if summary
            .fallback_page
            .as_ref()
            .is_none_or(|(timestamp, _)| record.timestamp_micros < *timestamp)
        {
            summary.fallback_page = Some((record.timestamp_micros, page));
        }
    }
    summary.rage_clicks += i64::from(matches!(
        record.event_type.as_str(),
        "rage_click" | "rageclick"
    ));
    summary.dead_clicks += i64::from(matches!(
        record.event_type.as_str(),
        "dead_click" | "deadclick"
    ));
    summary.crashes += i64::from(record.event_type == "crash");
    summary.failed_requests += i64::from(
        record.status.unwrap_or_default() >= 400
            || matches!(record.event_type.as_str(), "error" | "network_error"),
    );
    summary.slow_resources += i64::from(
        record.event_type == "resource" && record.duration_ms.unwrap_or_default() >= 1_000.0,
    );
    maximize(&mut summary.lcp, record.lcp_ms);
    maximize(&mut summary.fid, record.fid_ms);
    maximize(&mut summary.inp, record.inp_ms);
    maximize(&mut summary.cls, record.cls);
    maximize(&mut summary.ttfb, record.ttfb_ms);
    fill(&mut summary.application, record.application);
    fill(&mut summary.environment, record.environment);
    fill(&mut summary.version, record.version);
    fill(&mut summary.country, record.country);
    fill(&mut summary.browser, record.browser);
    fill(&mut summary.device, record.device);
    fill(&mut summary.os, record.os);
}

fn apply_summary(item: &mut Map<String, Value>, mut summary: ActionSummary) {
    let error_count = item
        .get("error_count")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let experience = classify_experience(error_count, &summary);
    summary.journey.sort_by_key(|(timestamp, _)| *timestamp);
    let mut journey = Vec::<String>::new();
    for (_, page) in summary.journey {
        if journey.last() != Some(&page) {
            journey.push(page);
        }
    }
    if journey.is_empty()
        && let Some((_, page)) = summary.fallback_page
    {
        journey.push(page);
    }
    item.insert("journey".into(), json!(journey));
    if !item.contains_key("landing_page")
        && let Some(page) = journey.first()
    {
        item.insert("landing_page".into(), json!(page));
    }
    if !item.contains_key("last_page")
        && let Some(page) = journey.last()
    {
        item.insert("last_page".into(), json!(page));
    }
    for (name, value) in [
        ("rage_click_count", summary.rage_clicks),
        ("dead_click_count", summary.dead_clicks),
        ("slow_resource_count", summary.slow_resources),
        ("failed_request_count", summary.failed_requests),
        ("crash_count", summary.crashes),
    ] {
        item.insert(name.into(), json!(value));
    }
    insert_number(item, "lcp_ms", summary.lcp);
    insert_number(item, "fid_ms", summary.fid);
    insert_number(item, "inp_ms", summary.inp);
    insert_number(item, "cls", summary.cls);
    insert_number(item, "ttfb_ms", summary.ttfb);
    insert_missing(item, "application", summary.application);
    insert_missing(item, "environment", summary.environment);
    insert_missing(item, "version", summary.version);
    insert_missing(item, "country", summary.country);
    insert_missing(item, "browser", summary.browser);
    insert_missing(item, "device", summary.device);
    insert_missing(item, "os", summary.os);
    item.insert("experience".into(), json!(experience));
}

fn classify_experience(error_count: i64, summary: &ActionSummary) -> &'static str {
    if error_count > 0
        || summary.failed_requests > 0
        || summary.rage_clicks > 0
        || summary.dead_clicks > 0
        || summary.crashes > 0
        || summary.lcp.is_some_and(|value| value > 4_000.0)
        || summary.fid.is_some_and(|value| value > 300.0)
        || summary.inp.is_some_and(|value| value > 500.0)
        || summary.cls.is_some_and(|value| value > 0.25)
        || summary.ttfb.is_some_and(|value| value > 1_800.0)
    {
        "poor"
    } else if summary.slow_resources > 0
        || summary.lcp.is_some_and(|value| value > 2_500.0)
        || summary.fid.is_some_and(|value| value > 100.0)
        || summary.inp.is_some_and(|value| value > 200.0)
        || summary.cls.is_some_and(|value| value > 0.1)
        || summary.ttfb.is_some_and(|value| value > 800.0)
    {
        "needs_improvement"
    } else if summary.has_actions {
        "good"
    } else {
        "unknown"
    }
}

fn maximize(target: &mut Option<f64>, value: Option<f64>) {
    if let Some(value) = value.filter(|value| value.is_finite()) {
        *target = Some(target.map_or(value, |current| current.max(value)));
    }
}

fn fill(target: &mut Option<String>, value: Option<String>) {
    if target.is_none() {
        *target = value;
    }
}

fn insert_missing(item: &mut Map<String, Value>, name: &str, value: Option<String>) {
    if !item.contains_key(name)
        && let Some(value) = value
    {
        item.insert(name.into(), Value::String(value));
    }
}

fn insert_number(item: &mut Map<String, Value>, name: &str, value: Option<f64>) {
    if let Some(value) = value.filter(|value| value.is_finite()) {
        item.insert(name.into(), json!(value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poor_action_takes_priority() {
        let summary = ActionSummary {
            rage_clicks: 1,
            lcp: Some(1_000.0),
            has_actions: true,
            ..ActionSummary::default()
        };
        assert_eq!(classify_experience(0, &summary), "poor");
    }
}
