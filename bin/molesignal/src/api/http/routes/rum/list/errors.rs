// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{cmp::Ordering, collections::HashMap};

use axum::{
    Extension, Json,
    extract::{Query, State},
};
use serde::Serialize;

use super::{
    ListQuery,
    cursor::{self, ErrorBoundary, ErrorCursorPayload},
    initial_page_context, normalize_text, shared_cursor_mismatch,
};
use crate::{
    api::{
        AppState,
        http::pagination::cursor::{CursorDirection, CursorPage, trim_cursor_page},
    },
    app::iam::IamContext,
    domain::iam::permission,
    infra::rum::read_model::RumReadModelReader,
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

const COLUMNS: &[&str] = &[
    "_timestamp",
    "fingerprint",
    "message",
    "user_id",
    "session_id",
    "page",
    "version",
    "error_type",
];

#[derive(Clone, Debug)]
pub(super) struct ErrorContext {
    pub(super) from: i64,
    pub(super) to: i64,
    pub(super) page_size: usize,
    pub(super) query: Option<String>,
    pub(super) status: Option<String>,
    pub(super) boundary: Option<ErrorBoundary>,
}

#[derive(Debug, Serialize)]
pub struct RumErrorSummary {
    pub fingerprint: String,
    pub message: String,
    pub count: i64,
    pub users: i64,
    pub sessions: i64,
    pub first_seen_micros: i64,
    pub last_seen_micros: i64,
    pub page: Option<String>,
    pub version: Option<String>,
    pub error_type: Option<String>,
    pub trend_pct: i64,
    pub status: &'static str,
    pub recent_sessions: Vec<String>,
    pub recent_users: Vec<String>,
}

#[derive(Debug)]
pub(super) struct ErrorRow {
    pub(super) item: RumErrorSummary,
}

#[derive(Default)]
struct ErrorGroup {
    message: String,
    count: i64,
    previous_count: i64,
    users: HashMap<String, i64>,
    sessions: HashMap<String, i64>,
    first_seen: i64,
    last_seen: i64,
    page: Option<String>,
    version: Option<String>,
    error_type: Option<String>,
}

#[permission(any("streams.query", "sys.telemetry.read"))]
pub(super) async fn list(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Query(request): Query<ListQuery>,
) -> Result<Json<CursorPage<RumErrorSummary>>> {
    let context = resolve_context(&state, &iam, request)?;
    let midpoint = context.from + (context.to - context.from) / 2;
    let new_threshold = context.from + (context.to - context.from) * 3 / 4;
    let reader = RumReadModelReader::new(
        state.storage.parquet_file_meta.clone(),
        state.storage.object_store.clone(),
    );
    let mut groups = HashMap::<String, ErrorGroup>::new();
    let stats = reader
        .visit_errors(
            &iam.org_id,
            TimeRange::new(TimestampMicros(context.from), TimestampMicros(context.to)),
            COLUMNS,
            |record| {
                let group = groups.entry(record.fingerprint).or_default();
                group.count += 1;
                group.previous_count += i64::from(record.timestamp_micros < midpoint);
                if group.first_seen == 0 || record.timestamp_micros < group.first_seen {
                    group.first_seen = record.timestamp_micros;
                }
                group.last_seen = group.last_seen.max(record.timestamp_micros);
                if let Some(message) = record.message
                    && (group.message.is_empty() || message < group.message)
                {
                    group.message = message;
                }
                update_min(&mut group.page, record.page);
                update_min(&mut group.version, record.version);
                update_min(&mut group.error_type, record.error_type);
                if let Some(user) = record.user_id {
                    group
                        .users
                        .entry(user)
                        .and_modify(|seen| *seen = (*seen).max(record.timestamp_micros))
                        .or_insert(record.timestamp_micros);
                }
                if let Some(session) = record.session_id {
                    group
                        .sessions
                        .entry(session)
                        .and_modify(|seen| *seen = (*seen).max(record.timestamp_micros))
                        .or_insert(record.timestamp_micros);
                }
            },
        )
        .await?;
    tracing::debug!(
        scanned_files = stats.files,
        scanned_rows = stats.rows,
        "RUM error list read model completed"
    );

    let mut rows = groups
        .into_iter()
        .map(|(fingerprint, group)| group_to_row(fingerprint, group, new_threshold))
        .filter(|row| matches_filters(row, &context))
        .filter(|row| matches_boundary(row, context.boundary.as_ref()))
        .collect::<Vec<_>>();
    let before = context
        .boundary
        .as_ref()
        .is_some_and(|boundary| boundary.direction == CursorDirection::Before);
    rows.sort_by(|left, right| effective_cmp(left, right, before));
    rows.truncate(context.page_size.saturating_add(1));
    let direction = context.boundary.as_ref().map(|boundary| boundary.direction);
    let page = trim_cursor_page(rows, context.page_size, direction);
    let previous_cursor = if page.has_previous {
        page.items
            .first()
            .map(|row| {
                cursor::encode_error(
                    state.iam.service.as_ref(),
                    &iam.org_id,
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
                cursor::encode_error(
                    state.iam.service.as_ref(),
                    &iam.org_id,
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

fn group_to_row(fingerprint: String, group: ErrorGroup, new_threshold: i64) -> ErrorRow {
    let recent = group.count.saturating_sub(group.previous_count);
    let trend_pct = if group.previous_count == 0 {
        i64::from(recent > 0) * 100
    } else {
        ((recent - group.previous_count) as f64 / group.previous_count as f64 * 100.0).round()
            as i64
    };
    let first_seen_micros = group.first_seen;
    ErrorRow {
        item: RumErrorSummary {
            fingerprint,
            message: group.message,
            count: group.count,
            users: i64::try_from(group.users.len()).unwrap_or(i64::MAX),
            sessions: i64::try_from(group.sessions.len()).unwrap_or(i64::MAX),
            first_seen_micros,
            last_seen_micros: group.last_seen,
            page: group.page,
            version: group.version,
            error_type: group.error_type,
            trend_pct,
            status: if first_seen_micros >= new_threshold {
                "new"
            } else {
                "ongoing"
            },
            recent_sessions: latest_keys(group.sessions, 10),
            recent_users: latest_keys(group.users, 50),
        },
    }
}

fn latest_keys(values: HashMap<String, i64>, limit: usize) -> Vec<String> {
    let mut values = values.into_iter().collect::<Vec<_>>();
    values.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    values
        .into_iter()
        .take(limit)
        .map(|(value, _)| value)
        .collect()
}

fn matches_filters(row: &ErrorRow, context: &ErrorContext) -> bool {
    if context
        .status
        .as_deref()
        .is_some_and(|status| row.item.status != status)
    {
        return false;
    }
    let Some(query) = context.query.as_deref() else {
        return true;
    };
    [
        Some(row.item.fingerprint.as_str()),
        Some(row.item.message.as_str()),
        row.item.page.as_deref(),
        row.item.version.as_deref(),
        row.item.error_type.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|value| value.contains(query))
}

fn canonical_cmp(left: &ErrorRow, right: &ErrorRow) -> Ordering {
    right
        .item
        .count
        .cmp(&left.item.count)
        .then_with(|| right.item.last_seen_micros.cmp(&left.item.last_seen_micros))
        .then_with(|| right.item.fingerprint.cmp(&left.item.fingerprint))
}

fn effective_cmp(left: &ErrorRow, right: &ErrorRow, before: bool) -> Ordering {
    let ordering = canonical_cmp(left, right);
    if before { ordering.reverse() } else { ordering }
}

fn matches_boundary(row: &ErrorRow, boundary: Option<&ErrorBoundary>) -> bool {
    let Some(boundary) = boundary else {
        return true;
    };
    let position = (
        boundary.count,
        boundary.last_seen_micros,
        boundary.fingerprint.as_str(),
    );
    let current = (
        row.item.count,
        row.item.last_seen_micros,
        row.item.fingerprint.as_str(),
    );
    match boundary.direction {
        CursorDirection::After => current < position,
        CursorDirection::Before => current > position,
    }
}

fn resolve_context(state: &AppState, iam: &IamContext, request: ListQuery) -> Result<ErrorContext> {
    if let Some(token) = request.cursor.as_deref() {
        let payload = cursor::decode_error(state.iam.service.as_ref(), &iam.org_id, token)?;
        validate_cursor_request(&request, &payload)?;
        return Ok(ErrorContext {
            from: payload.from,
            to: payload.to,
            page_size: payload.page_size,
            query: payload.query,
            status: payload.status,
            boundary: Some(ErrorBoundary {
                direction: payload.direction,
                count: payload.count,
                last_seen_micros: payload.last_seen_micros,
                fingerprint: payload.fingerprint,
            }),
        });
    }
    let (from, to, page_size) = initial_page_context(&request)?;
    Ok(ErrorContext {
        from,
        to,
        page_size,
        query: normalize_text(request.q, 256),
        status: normalize_status(request.status)?,
        boundary: None,
    })
}

fn validate_cursor_request(request: &ListQuery, payload: &ErrorCursorPayload) -> Result<()> {
    let requested_status = if request.status.is_some() {
        normalize_status(request.status.clone())?
    } else {
        None
    };
    let mismatch = shared_cursor_mismatch(request, payload.from, payload.to, payload.page_size)
        || request
            .q
            .as_ref()
            .is_some_and(|value| normalize_text(Some(value.clone()), 256) != payload.query)
        || (request.status.is_some() && requested_status != payload.status);
    if mismatch {
        return Err(Error::invalid(
            "RUM error cursor does not match active query",
        ));
    }
    Ok(())
}

fn normalize_status(value: Option<String>) -> Result<Option<String>> {
    let value = normalize_text(value, 16).map(|value| value.to_ascii_lowercase());
    if value
        .as_deref()
        .is_some_and(|value| !matches!(value, "new" | "ongoing"))
    {
        return Err(Error::invalid("unsupported RUM error status"));
    }
    Ok(value)
}

fn update_min(target: &mut Option<String>, value: Option<String>) {
    if let Some(value) = value
        && target.as_ref().is_none_or(|current| value < *current)
    {
        *target = Some(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_keys_are_ordered_by_last_seen() {
        assert_eq!(
            latest_keys(HashMap::from([("a".into(), 1), ("b".into(), 2)]), 10),
            vec!["b", "a"]
        );
    }
}
