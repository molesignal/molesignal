// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::{BTreeSet, HashSet};

use axum::{
    Extension, Json,
    extract::{Path, Query, State},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::permission,
    infra::rum::read_model::{RumActionRecord, RumErrorRecord, RumReadModelReader},
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

const DEFAULT_WINDOW_MICROS: i64 = 24 * 60 * 60 * 1_000_000;
const MAX_SESSION_EVENTS: usize = 500;
const MAX_ERROR_SAMPLES: usize = 50;

#[derive(Debug, Default, Deserialize)]
pub(super) struct DetailQuery {
    from: Option<i64>,
    to: Option<i64>,
}

#[derive(Debug, Serialize)]
pub(super) struct SessionDetailResponse {
    session: Option<Map<String, Value>>,
    events: Vec<Map<String, Value>>,
}

#[derive(Debug, Serialize)]
pub(super) struct ErrorDetailResponse {
    fingerprint: String,
    message: String,
    stack: Vec<Value>,
    recent_sessions: Vec<String>,
    count: usize,
    users: usize,
    first_seen_micros: i64,
    last_seen_micros: i64,
    pages: Vec<String>,
    versions: Vec<String>,
}

#[permission(any("streams.query", "sys.telemetry.read"))]
pub(super) async fn session(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Path(session_id): Path<String>,
    Query(query): Query<DetailQuery>,
) -> Result<Json<SessionDetailResponse>> {
    if session_id.is_empty() {
        return Err(Error::invalid("empty session id"));
    }
    let range = resolve_range(query)?;
    let ids = HashSet::from([session_id.clone()]);
    let reader = reader(&state);
    let mut sessions = Vec::new();
    let mut actions = Vec::new();
    let (session_stats, action_stats, replay_ids) = tokio::try_join!(
        reader.visit_raw_sessions_for_ids(&iam.org_id, range, &ids, |record| {
            sessions.push(record);
        }),
        reader.visit_raw_actions_for_sessions(&iam.org_id, range, &ids, |record| {
            actions.push(record);
        }),
        state
            .telemetry
            .rum_replay
            .existing_session_ids(&iam.org_id, std::slice::from_ref(&session_id)),
    )?;
    tracing::debug!(
        session_files = session_stats.files,
        action_files = action_stats.files,
        action_rows = action_stats.rows,
        "RUM session detail read model completed"
    );
    sessions.sort_by_key(|record| std::cmp::Reverse(record.timestamp_micros));
    actions.sort_by_key(|record| record.timestamp_micros);
    actions.truncate(MAX_SESSION_EVENTS);
    let session = sessions
        .into_iter()
        .next()
        .map(|record| session_map(record, &actions, replay_ids.contains(&session_id)));
    let events = actions.into_iter().map(action_map).collect();
    Ok(Json(SessionDetailResponse { session, events }))
}

#[permission(any("streams.query", "sys.telemetry.read"))]
pub(super) async fn error(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Path(fingerprint): Path<String>,
    Query(query): Query<DetailQuery>,
) -> Result<Json<Option<ErrorDetailResponse>>> {
    if fingerprint.is_empty() {
        return Err(Error::invalid("empty RUM error fingerprint"));
    }
    let range = resolve_range(query)?;
    let reader = reader(&state);
    let mut records = Vec::new();
    let stats = reader
        .visit_raw_errors_by_fingerprint(&iam.org_id, range, &fingerprint, |record| {
            records.push(record);
        })
        .await?;
    records.sort_by_key(|record| std::cmp::Reverse(record.timestamp_micros));
    records.truncate(MAX_ERROR_SAMPLES);
    trace_detail_stats("error", stats.files, stats.rows, records.len());
    Ok(Json(build_error_detail(fingerprint, records)))
}

fn session_map(
    record: crate::infra::rum::read_model::RumSessionRecord,
    actions: &[RumActionRecord],
    replay_available: bool,
) -> Map<String, Value> {
    let mut item = Map::new();
    item.insert(
        "session_id".into(),
        Value::String(record.session_id.clone()),
    );
    insert_string(&mut item, "user_id", record.user_id);
    insert_string(&mut item, "ip_address", record.ip_address);
    insert_string(&mut item, "country", record.country);
    insert_string(&mut item, "browser", record.browser);
    insert_string(&mut item, "application", record.application);
    insert_string(&mut item, "environment", record.environment);
    insert_string(&mut item, "version", record.version);
    insert_string(&mut item, "device", record.device);
    insert_string(&mut item, "os", record.os);
    insert_string(&mut item, "landing_page", record.landing_page);
    insert_string(&mut item, "last_page", record.last_page);
    insert_number(&mut item, "duration_ms", record.duration_ms);
    item.insert("error_count".into(), json!(record.error_count));
    item.insert("started_at_micros".into(), json!(record.timestamp_micros));
    item.insert("replay_available".into(), json!(replay_available));

    let mut journey = Vec::<String>::new();
    let mut rage = 0_i64;
    let mut dead = 0_i64;
    let mut slow = 0_i64;
    let mut failed = 0_i64;
    let mut crashes = 0_i64;
    for action in actions {
        if action.event_type == "view"
            && let Some(page) = action.page_key()
            && journey.last().is_none_or(|current| current != page)
        {
            journey.push(page.to_string());
        }
        rage += i64::from(matches!(
            action.event_type.as_str(),
            "rage_click" | "rageclick"
        ));
        dead += i64::from(matches!(
            action.event_type.as_str(),
            "dead_click" | "deadclick"
        ));
        crashes += i64::from(action.event_type == "crash");
        failed += i64::from(
            action.status.unwrap_or_default() >= 400
                || matches!(action.event_type.as_str(), "error" | "network_error"),
        );
        slow += i64::from(
            action.event_type == "resource" && action.duration_ms.unwrap_or_default() >= 1_000.0,
        );
    }
    item.insert("journey".into(), json!(journey));
    item.insert("rage_click_count".into(), json!(rage));
    item.insert("dead_click_count".into(), json!(dead));
    item.insert("slow_resource_count".into(), json!(slow));
    item.insert("failed_request_count".into(), json!(failed));
    item.insert("crash_count".into(), json!(crashes));
    let experience = if record.error_count > 0 || rage > 0 || dead > 0 || failed > 0 || crashes > 0
    {
        "poor"
    } else if slow > 0 {
        "needs_improvement"
    } else if actions.is_empty() {
        "unknown"
    } else {
        "good"
    };
    item.insert("experience".into(), json!(experience));
    item
}

fn action_map(record: RumActionRecord) -> Map<String, Value> {
    let mut item = Map::new();
    item.insert("ts_micros".into(), json!(record.timestamp_micros));
    item.insert("type".into(), json!(record.event_type));
    insert_string(&mut item, "name", record.name);
    insert_string(
        &mut item,
        "url",
        record.page.filter(|value| !value.is_empty()).or(record.url),
    );
    insert_number(&mut item, "duration_ms", record.duration_ms);
    if let Some(status) = record.status {
        item.insert("status".into(), json!(status));
    }
    item.insert(
        "payload".into(),
        record.payload.unwrap_or_else(|| Value::Object(Map::new())),
    );
    insert_string(&mut item, "service", record.service);
    insert_string(&mut item, "trace_id", record.trace_id);
    insert_string(&mut item, "parent_span_id", record.parent_span_id);
    item
}

fn build_error_detail(
    fingerprint: String,
    records: Vec<RumErrorRecord>,
) -> Option<ErrorDetailResponse> {
    let first = records.first()?;
    let message = first.message.clone().unwrap_or_default();
    let stack = error_stack(first.error.as_ref());
    let users = records
        .iter()
        .filter_map(|record| record.user_id.as_deref())
        .collect::<HashSet<_>>()
        .len();
    let recent_sessions = unique_in_order(
        records
            .iter()
            .filter_map(|record| record.session_id.clone()),
        10,
    );
    let pages = records
        .iter()
        .filter_map(|record| record.page.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let versions = records
        .iter()
        .filter_map(|record| record.version.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let first_seen_micros = records
        .iter()
        .map(|record| record.timestamp_micros)
        .min()
        .unwrap_or_default();
    let last_seen_micros = records
        .iter()
        .map(|record| record.timestamp_micros)
        .max()
        .unwrap_or_default();
    Some(ErrorDetailResponse {
        fingerprint,
        message,
        stack,
        recent_sessions,
        count: records.len(),
        users,
        first_seen_micros,
        last_seen_micros,
        pages,
        versions,
    })
}

fn error_stack(error: Option<&Value>) -> Vec<Value> {
    match error.and_then(|value| value.get("stack")) {
        Some(Value::Array(stack)) => stack.clone(),
        Some(Value::String(stack)) => serde_json::from_str(stack).unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn unique_in_order(values: impl IntoIterator<Item = String>, limit: usize) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert(value.clone()))
        .take(limit)
        .collect()
}

fn resolve_range(query: DetailQuery) -> Result<TimeRange> {
    let now = TimestampMicros::now().0;
    let from = query
        .from
        .unwrap_or_else(|| now.saturating_sub(DEFAULT_WINDOW_MICROS));
    let to = query.to.unwrap_or(now);
    if to <= from {
        return Err(Error::invalid("RUM range end must be greater than start"));
    }
    Ok(TimeRange::new(TimestampMicros(from), TimestampMicros(to)))
}

fn reader(state: &AppState) -> RumReadModelReader {
    RumReadModelReader::new(
        state.storage.catalog_files.clone(),
        state.storage.read_store.clone(),
    )
    .with_catalog_source(state.storage.catalog_query.clone())
}

fn insert_string(item: &mut Map<String, Value>, name: &str, value: Option<String>) {
    if let Some(value) = value {
        item.insert(name.into(), Value::String(value));
    }
}

fn insert_number(item: &mut Map<String, Value>, name: &str, value: Option<f64>) {
    if let Some(value) = value.filter(|value| value.is_finite()) {
        item.insert(name.into(), json!(value));
    }
}

fn trace_detail_stats(endpoint: &str, files: usize, rows: usize, returned: usize) {
    tracing::debug!(
        endpoint,
        scanned_files = files,
        scanned_rows = rows,
        returned,
        "RUM detail read model completed"
    );
}
