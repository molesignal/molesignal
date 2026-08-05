// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Json,
    extract::{Query, State},
};
use serde_json::{Map, Value};

use super::{
    ListQuery, column,
    cursor::{self, SessionBoundary, SessionCursorPayload},
    has_field, initial_page_context, log_stream, normalize_text, run_log_query,
    shared_cursor_mismatch, sql_literal, value_i64,
};
use crate::{
    api::{
        AppState,
        http::pagination::cursor::{CursorDirection, CursorPage, trim_cursor_page},
    },
    app::iam::IamContext,
    domain::{
        iam::permission,
        ingestion::EVENT_ID_FIELD,
        storage::PhysicalDatasetKind,
        stream::{FieldDef, StreamDefinition},
    },
    infra::query::escape_sql_ident,
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

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
    let Some(stream) = log_stream(&state, &iam, "rum_sessions").await? else {
        return Ok(Json(CursorPage::empty()));
    };
    if !has_field(&stream.schema.fields, "session_id")
        || !has_field(&stream.schema.fields, EVENT_ID_FIELD)
    {
        return Ok(Json(CursorPage::empty()));
    }

    let fetch_limit = context.page_size.saturating_add(1);
    // 派生写入已把 `_timestamp` 规范为 session start。
    let timestamp = "CAST(_timestamp AS BIGINT)".to_string();
    let replay_session_ids = if context.replay_only {
        state
            .telemetry
            .rum_replay
            .session_ids_in_window(&iam.org_id, context.from, context.to)
            .await?
    } else {
        Vec::new()
    };
    if context.replay_only && replay_session_ids.is_empty() {
        return Ok(Json(CursorPage::empty()));
    }
    let sql = build_sql(
        &context,
        &stream,
        &timestamp,
        fetch_limit,
        &replay_session_ids,
    );
    let output = run_log_query(
        &state,
        &iam,
        "rum_sessions",
        TimeRange::new(TimestampMicros(context.from), TimestampMicros(context.to)),
        sql,
        fetch_limit,
        PhysicalDatasetKind::RumSessionSummary,
    )
    .await?;
    let direction = context.boundary.as_ref().map(|boundary| boundary.direction);
    let mut page = trim_cursor_page(rows(output), context.page_size, direction);
    let session_ids = page
        .items
        .iter()
        .map(|row| row.session_id.clone())
        .collect::<Vec<_>>();
    let available = state
        .telemetry
        .rum_replay
        .existing_session_ids(&iam.org_id, &session_ids)
        .await?;
    for row in &mut page.items {
        row.item.insert(
            "replay_available".into(),
            Value::Bool(available.contains(&row.session_id)),
        );
    }
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

fn build_sql(
    context: &SessionContext,
    stream: &StreamDefinition,
    timestamp: &str,
    fetch_limit: usize,
    replay_session_ids: &[String],
) -> String {
    let mut clauses = vec![
        "session_id IS NOT NULL AND session_id != ''".into(),
        format!("{timestamp} >= {}", context.from),
        format!("{timestamp} < {}", context.to),
    ];
    if let Some(query) = context.query.as_deref() {
        let searchable = [
            "session_id",
            "user_id",
            "country",
            "browser",
            "application",
            "environment",
            "version",
            "landing_page",
            "last_page",
        ]
        .into_iter()
        .filter(|field| *field == "session_id" || has_field(&stream.schema.fields, field))
        .map(|field| {
            format!(
                "CAST(\"{}\" AS VARCHAR) LIKE {}",
                escape_sql_ident(field),
                sql_literal(&format!("%{query}%"))
            )
        })
        .collect::<Vec<_>>();
        clauses.push(format!("({})", searchable.join(" OR ")));
    }
    add_exact_filter(
        &mut clauses,
        &stream.schema.fields,
        "country",
        context.country.as_deref(),
    );
    add_exact_filter(
        &mut clauses,
        &stream.schema.fields,
        "browser",
        context.browser.as_deref(),
    );
    if context.replay_only {
        clauses.push(format!(
            "session_id IN ({})",
            replay_session_ids
                .iter()
                .map(|session_id| sql_literal(session_id))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if let Some(boundary) = &context.boundary {
        clauses.push(cursor::session_seek(boundary, timestamp));
    }
    let order = if context
        .boundary
        .as_ref()
        .is_some_and(|boundary| boundary.direction == CursorDirection::Before)
    {
        "ASC"
    } else {
        "DESC"
    };
    format!(
        "SELECT *, {timestamp} AS __cursor_started_at FROM \"{}\" \
         WHERE {} ORDER BY _timestamp {order}, session_id {order}, \
         \"{EVENT_ID_FIELD}\" {order} LIMIT {fetch_limit}",
        escape_sql_ident(&stream.name),
        clauses.join(" AND "),
    )
}

fn add_exact_filter(
    clauses: &mut Vec<String>,
    fields: &[FieldDef],
    field: &str,
    value: Option<&str>,
) {
    let Some(value) = value else {
        return;
    };
    if has_field(fields, field) {
        clauses.push(format!(
            "\"{}\" = {}",
            escape_sql_ident(field),
            sql_literal(value)
        ));
    } else {
        clauses.push("1 = 0".into());
    }
}

fn rows(output: crate::domain::query::QueryResult) -> Vec<SessionRow> {
    let started = column(&output, "__cursor_started_at");
    let session = column(&output, "session_id");
    let event = column(&output, EVENT_ID_FIELD);
    output
        .rows
        .into_iter()
        .filter_map(|row| {
            let started_at_micros = started
                .and_then(|index| row.get(index))
                .and_then(value_i64)?;
            let session_id = session
                .and_then(|index| row.get(index))
                .and_then(Value::as_str)?
                .to_string();
            let event_id = event
                .and_then(|index| row.get(index))
                .and_then(Value::as_str)?
                .to_string();
            let mut item = Map::new();
            for (index, name) in output.columns.iter().enumerate() {
                if name == "__cursor_started_at" || name == EVENT_ID_FIELD {
                    continue;
                }
                item.insert(name.clone(), row.get(index).cloned().unwrap_or(Value::Null));
            }
            Some(SessionRow {
                item,
                started_at_micros,
                session_id,
                event_id,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::stream::{Schema, StreamType},
        shared::{ids::Id, time::TimestampMicros},
    };

    #[test]
    fn query_has_strict_window_and_compound_order() {
        let context = SessionContext {
            from: 10,
            to: 20,
            page_size: 20,
            query: None,
            country: None,
            browser: None,
            replay_only: false,
            boundary: None,
        };
        let stream = StreamDefinition {
            id: Id::new(),
            org_id: Id::new(),
            name: "rum_sessions".into(),
            stream_type: StreamType::Logs,
            schema: Schema { fields: vec![] },
            retention: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        };
        let sql = build_sql(&context, &stream, "CAST(_timestamp AS BIGINT)", 21, &[]);
        assert!(sql.contains("CAST(_timestamp AS BIGINT) >= 10"));
        assert!(sql.contains("CAST(_timestamp AS BIGINT) < 20"));
        assert!(
            sql.contains("ORDER BY _timestamp DESC, session_id DESC, \"_event_id\" DESC LIMIT 21")
        );
    }

    #[test]
    fn replay_only_query_restricts_session_ids() {
        let context = SessionContext {
            from: 10,
            to: 20,
            page_size: 20,
            query: None,
            country: None,
            browser: None,
            replay_only: true,
            boundary: None,
        };
        let stream = StreamDefinition {
            id: Id::new(),
            org_id: Id::new(),
            name: "rum_sessions".into(),
            stream_type: StreamType::Logs,
            schema: Schema { fields: vec![] },
            retention: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        };
        let sql = build_sql(
            &context,
            &stream,
            "CAST(_timestamp AS BIGINT)",
            21,
            &["ses_a".into(), "ses_b".into()],
        );
        assert!(sql.contains("session_id IN ('ses_a', 'ses_b')"));
    }
}
