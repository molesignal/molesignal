// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashSet;

use axum::{
    Extension, Json,
    extract::{Query, State},
};
use serde_json::{Map, Value, json};

use super::{
    ListQuery,
    cursor::{self, SessionBoundary, SessionCursorPayload},
    initial_page_context, normalize_text, shared_cursor_mismatch,
};
use crate::{
    api::{
        AppState,
        http::pagination::cursor::{CursorDirection, CursorPage, trim_cursor_page},
    },
    app::iam::IamContext,
    domain::iam::permission,
    infra::rum::read_model::{
        RumReadModelReader, RumSessionRecord, SessionPageBoundary, SessionPageQuery,
    },
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

mod enrichment;

const DEFAULT_ACTION_LOOKAHEAD_MICROS: i64 = 4 * 60 * 60 * 1_000_000;
const MAX_ACTION_LOOKAHEAD_MICROS: i64 = 24 * 60 * 60 * 1_000_000;
const ACTION_GRACE_MICROS: i64 = 5 * 60 * 1_000_000;

#[derive(Clone, Debug)]
pub(super) struct SessionContext {
    pub(super) from: i64,
    pub(super) to: i64,
    pub(super) page_size: usize,
    pub(super) query: Option<String>,
    pub(super) country: Option<String>,
    pub(super) browser: Option<String>,
    pub(super) replay_only: bool,
    pub(super) boundary: Option<SessionBoundary>,
}

#[derive(Debug)]
pub(super) struct SessionRow {
    pub(super) item: Map<String, Value>,
    pub(super) started_at_micros: i64,
    pub(super) session_id: String,
    pub(super) event_id: String,
}

#[permission(any("streams.query", "sys.telemetry.read"))]
pub(super) async fn list(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Query(request): Query<ListQuery>,
) -> Result<Json<CursorPage<Map<String, Value>>>> {
    let context = resolve_context(&state, &iam, request)?;
    let replay_filter = if context.replay_only {
        state
            .telemetry
            .rum_replay
            .session_ids_in_window(&iam.org_id, context.from, context.to)
            .await?
            .into_iter()
            .collect::<HashSet<_>>()
    } else {
        HashSet::new()
    };
    if context.replay_only && replay_filter.is_empty() {
        return Ok(Json(CursorPage::empty()));
    }

    let reader = RumReadModelReader::new(
        state.storage.catalog_files.clone(),
        state.storage.read_store.clone(),
    )
    .with_catalog_source(state.storage.catalog_query.clone());
    let boundary = context
        .boundary
        .as_ref()
        .map(|boundary| SessionPageBoundary {
            direction: boundary.direction,
            started_at_micros: boundary.started_at_micros,
            session_id: &boundary.session_id,
            event_id: &boundary.event_id,
        });
    let records = reader
        .session_page(
            &iam.org_id,
            TimeRange::new(TimestampMicros(context.from), TimestampMicros(context.to)),
            SessionPageQuery {
                text: context.query.as_deref(),
                country: context.country.as_deref(),
                browser: context.browser.as_deref(),
                allowed_session_ids: context.replay_only.then_some(&replay_filter),
                boundary,
                limit: context.page_size.saturating_add(1),
            },
        )
        .await?;
    let direction = context.boundary.as_ref().map(|boundary| boundary.direction);
    let mut page = trim_cursor_page(
        records.into_iter().map(record_to_row).collect(),
        context.page_size,
        direction,
    );
    enrichment::enrich_rows(&state, &iam, &reader, &mut page.items).await?;

    let previous_cursor = if page.has_previous {
        page.items
            .first()
            .map(|row| {
                cursor::encode_session(
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
                cursor::encode_session(
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

fn record_to_row(record: RumSessionRecord) -> SessionRow {
    let mut item = Map::new();
    item.insert(
        "session_id".into(),
        Value::String(record.session_id.clone()),
    );
    insert_optional(&mut item, "user_id", record.user_id);
    insert_optional(&mut item, "ip_address", record.ip_address);
    insert_number(&mut item, "duration_ms", record.duration_ms);
    insert_optional(&mut item, "application", record.application);
    insert_optional(&mut item, "service", record.service);
    insert_optional(&mut item, "environment", record.environment);
    insert_optional(&mut item, "version", record.version);
    insert_optional(&mut item, "country", record.country);
    insert_optional(&mut item, "browser", record.browser);
    insert_optional(&mut item, "device", record.device);
    insert_optional(&mut item, "os", record.os);
    insert_optional(&mut item, "landing_page", record.landing_page);
    insert_optional(&mut item, "last_page", record.last_page);
    insert_integer(&mut item, "view_count", record.view_count);
    insert_integer(&mut item, "action_count", record.action_count);
    item.insert("error_count".into(), json!(record.error_count));
    insert_optional(&mut item, "trace_id", record.trace_id);
    item.insert("started_at_micros".into(), json!(record.timestamp_micros));
    SessionRow {
        item,
        started_at_micros: record.timestamp_micros,
        session_id: record.session_id,
        event_id: record.event_id,
    }
}

fn action_range(rows: &[SessionRow]) -> TimeRange {
    let start = rows
        .iter()
        .map(|row| row.started_at_micros)
        .min()
        .unwrap_or_default();
    let end = rows
        .iter()
        .map(|row| {
            let duration = row
                .item
                .get("duration_ms")
                .and_then(Value::as_f64)
                .map(|value| (value.max(0.0) * 1_000.0) as i64)
                .unwrap_or(DEFAULT_ACTION_LOOKAHEAD_MICROS)
                .min(MAX_ACTION_LOOKAHEAD_MICROS);
            row.started_at_micros
                .saturating_add(duration)
                .saturating_add(ACTION_GRACE_MICROS)
        })
        .max()
        .unwrap_or(start);
    TimeRange::new(TimestampMicros(start), TimestampMicros(end.max(start + 1)))
}

fn resolve_context(
    state: &AppState,
    iam: &IamContext,
    request: ListQuery,
) -> Result<SessionContext> {
    if let Some(token) = request.cursor.as_deref() {
        let payload = cursor::decode_session(state.iam.service.as_ref(), &iam.org_id, token)?;
        validate_cursor_request(&request, &payload)?;
        return Ok(SessionContext {
            from: payload.from,
            to: payload.to,
            page_size: payload.page_size,
            query: payload.query,
            country: payload.country,
            browser: payload.browser,
            replay_only: payload.replay_only,
            boundary: Some(SessionBoundary {
                direction: payload.direction,
                started_at_micros: payload.started_at_micros,
                session_id: payload.session_id,
                event_id: payload.event_id,
            }),
        });
    }
    let (from, to, page_size) = initial_page_context(&request)?;
    Ok(SessionContext {
        from,
        to,
        page_size,
        query: normalize_text(request.q, 256),
        country: normalize_text(request.country, 128),
        browser: normalize_text(request.browser, 128),
        replay_only: request.replay_available.unwrap_or(false),
        boundary: None,
    })
}

fn validate_cursor_request(request: &ListQuery, payload: &SessionCursorPayload) -> Result<()> {
    let mismatch = shared_cursor_mismatch(request, payload.from, payload.to, payload.page_size)
        || request
            .q
            .as_ref()
            .is_some_and(|value| normalize_text(Some(value.clone()), 256) != payload.query)
        || request
            .country
            .as_ref()
            .is_some_and(|value| normalize_text(Some(value.clone()), 128) != payload.country)
        || request
            .browser
            .as_ref()
            .is_some_and(|value| normalize_text(Some(value.clone()), 128) != payload.browser)
        || request
            .replay_available
            .is_some_and(|value| value != payload.replay_only);
    if mismatch {
        return Err(Error::invalid(
            "RUM session cursor does not match active query",
        ));
    }
    Ok(())
}

fn insert_optional(item: &mut Map<String, Value>, name: &str, value: Option<String>) {
    if let Some(value) = value {
        item.insert(name.into(), Value::String(value));
    }
}

fn insert_number(item: &mut Map<String, Value>, name: &str, value: Option<f64>) {
    if let Some(value) = value.filter(|value| value.is_finite()) {
        item.insert(name.into(), json!(value));
    }
}

fn insert_integer(item: &mut Map<String, Value>, name: &str, value: Option<i64>) {
    if let Some(value) = value {
        item.insert(name.into(), json!(value));
    }
}
