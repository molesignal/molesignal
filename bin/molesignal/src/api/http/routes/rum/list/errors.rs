// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Json,
    extract::{Query, State},
};
use serde::Serialize;
use serde_json::Value;

use super::{
    ListQuery, column,
    cursor::{self, ErrorBoundary, ErrorCursorPayload},
    has_field, initial_page_context, log_stream, non_empty_string, normalize_text, run_log_query,
    shared_cursor_mismatch, sql_literal, value_i64, value_string,
};
use crate::{
    api::{
        AppState,
        http::pagination::cursor::{CursorDirection, CursorPage, trim_cursor_page},
    },
    app::iam::IamContext,
    domain::{
        iam::permission, query::QueryResult, storage::PhysicalDatasetKind, stream::StreamDefinition,
    },
    infra::query::escape_sql_ident,
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

#[derive(Clone, Debug)]
pub(super) struct ErrorContext {
    pub(super) from: i64,
    pub(super) to: i64,
    pub(super) page_size: usize,
    pub(super) query: Option<String>,
    pub(super) status: Option<String>,
    pub(super) boundary: Option<ErrorBoundary>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RumErrorSummary {
    pub(super) fingerprint: String,
    message: String,
    pub(super) count: i64,
    users: i64,
    sessions: i64,
    first_seen_micros: i64,
    pub(super) last_seen_micros: i64,
    page: Option<String>,
    version: Option<String>,
    error_type: Option<String>,
    trend_pct: i64,
    status: &'static str,
    recent_sessions: Vec<String>,
    recent_users: Vec<String>,
}

#[derive(Debug)]
pub(super) struct ErrorRow {
    pub(super) item: RumErrorSummary,
}

#[permission(any("streams.query", "sys.telemetry.read"))]
pub(super) async fn list(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Query(request): Query<ListQuery>,
) -> Result<Json<CursorPage<RumErrorSummary>>> {
    let context = resolve_context(&state, &iam, request)?;
    let Some(stream) = log_stream(&state, &iam, "rum_errors").await? else {
        return Ok(Json(CursorPage::empty()));
    };
    if !has_field(&stream.schema.fields, "fingerprint") {
        return Ok(Json(CursorPage::empty()));
    }

    let fetch_limit = context.page_size.saturating_add(1);
    let timestamp = "CAST(_timestamp AS BIGINT)".to_string();
    let sql = build_sql(&context, &stream, &timestamp, fetch_limit);
    let output = run_log_query(
        &state,
        &iam,
        "rum_errors",
        TimeRange::new(TimestampMicros(context.from), TimestampMicros(context.to)),
        sql,
        fetch_limit,
        PhysicalDatasetKind::RumErrorSummary,
    )
    .await?;
    let direction = context.boundary.as_ref().map(|boundary| boundary.direction);
    let mut page = trim_cursor_page(rows(output, &context), context.page_size, direction);
    enrich_members(&state, &iam, &stream, &context, &timestamp, &mut page.items).await?;
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

fn build_sql(
    context: &ErrorContext,
    stream: &StreamDefinition,
    timestamp: &str,
    fetch_limit: usize,
) -> String {
    let fields = &stream.schema.fields;
    let midpoint = context.from + (context.to - context.from) / 2;
    let new_threshold = context.from + (context.to - context.from) * 3 / 4;
    let field = |name: &str, fallback: &str| {
        if has_field(fields, name) {
            format!("CAST(\"{}\" AS VARCHAR)", escape_sql_ident(name))
        } else {
            fallback.into()
        }
    };
    let distinct = |name: &str| {
        if has_field(fields, name) {
            format!("COUNT(DISTINCT \"{}\")", escape_sql_ident(name))
        } else {
            "0".into()
        }
    };
    let mut outer = Vec::new();
    if let Some(query) = context.query.as_deref() {
        let pattern = sql_literal(&format!("%{query}%"));
        outer.push(format!(
            "(fingerprint LIKE {pattern} OR message LIKE {pattern} OR page LIKE {pattern} OR version LIKE {pattern} OR error_type LIKE {pattern})"
        ));
    }
    if let Some(status) = context.status.as_deref() {
        outer.push(if status == "new" {
            format!("first_seen_micros >= {new_threshold}")
        } else {
            format!("first_seen_micros < {new_threshold}")
        });
    }
    if let Some(boundary) = &context.boundary {
        outer.push(cursor::error_seek(boundary));
    }
    let outer_where = if outer.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", outer.join(" AND "))
    };
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
        "WITH error_groups AS (\
           SELECT fingerprint, MIN({}) AS message, COUNT(*) AS error_count, \
                  {} AS users, {} AS sessions, MIN({timestamp}) AS first_seen_micros, \
                  MAX({timestamp}) AS last_seen_micros, MIN({}) AS page, \
                  MIN({}) AS version, MIN({}) AS error_type, \
                  SUM(CASE WHEN {timestamp} < {midpoint} THEN 1 ELSE 0 END) AS previous_count \
           FROM \"{}\" WHERE fingerprint IS NOT NULL AND fingerprint != '' \
             AND {timestamp} >= {} AND {timestamp} < {} GROUP BY fingerprint\
         ) SELECT * FROM error_groups {outer_where} \
           ORDER BY error_count {order}, last_seen_micros {order}, fingerprint {order} \
           LIMIT {fetch_limit}",
        field("message", "''"),
        distinct("user_id"),
        distinct("session_id"),
        field("page", "''"),
        field("version", "''"),
        field("error_type", "''"),
        escape_sql_ident(&stream.name),
        context.from,
        context.to,
    )
}

fn rows(output: QueryResult, context: &ErrorContext) -> Vec<ErrorRow> {
    let index = |name: &str| column(&output, name);
    let fingerprint = index("fingerprint");
    let message = index("message");
    let count = index("error_count");
    let users = index("users");
    let sessions = index("sessions");
    let first = index("first_seen_micros");
    let last = index("last_seen_micros");
    let page = index("page");
    let version = index("version");
    let error_type = index("error_type");
    let previous = index("previous_count");
    let new_threshold = context.from + (context.to - context.from) * 3 / 4;
    output
        .rows
        .into_iter()
        .filter_map(|row| {
            let fingerprint = fingerprint
                .and_then(|index| row.get(index))
                .and_then(Value::as_str)?
                .to_string();
            let count = count
                .and_then(|index| row.get(index))
                .and_then(value_i64)
                .unwrap_or_default();
            let previous = previous
                .and_then(|index| row.get(index))
                .and_then(value_i64)
                .unwrap_or_default();
            let recent = count.saturating_sub(previous);
            let trend_pct = if previous == 0 {
                if recent > 0 { 100 } else { 0 }
            } else {
                ((recent - previous) as f64 / previous as f64 * 100.0).round() as i64
            };
            let first_seen_micros = first
                .and_then(|index| row.get(index))
                .and_then(value_i64)
                .unwrap_or_default();
            Some(ErrorRow {
                item: RumErrorSummary {
                    fingerprint,
                    message: message
                        .and_then(|index| row.get(index))
                        .and_then(value_string)
                        .unwrap_or_default(),
                    count,
                    users: users
                        .and_then(|index| row.get(index))
                        .and_then(value_i64)
                        .unwrap_or_default(),
                    sessions: sessions
                        .and_then(|index| row.get(index))
                        .and_then(value_i64)
                        .unwrap_or_default(),
                    first_seen_micros,
                    last_seen_micros: last
                        .and_then(|index| row.get(index))
                        .and_then(value_i64)
                        .unwrap_or_default(),
                    page: page
                        .and_then(|index| row.get(index))
                        .and_then(non_empty_string),
                    version: version
                        .and_then(|index| row.get(index))
                        .and_then(non_empty_string),
                    error_type: error_type
                        .and_then(|index| row.get(index))
                        .and_then(non_empty_string),
                    trend_pct,
                    status: if first_seen_micros >= new_threshold {
                        "new"
                    } else {
                        "ongoing"
                    },
                    recent_sessions: Vec::new(),
                    recent_users: Vec::new(),
                },
            })
        })
        .collect()
}

async fn enrich_members(
    state: &AppState,
    iam: &IamContext,
    stream: &StreamDefinition,
    context: &ErrorContext,
    timestamp: &str,
    rows: &mut [ErrorRow],
) -> Result<()> {
    if rows.is_empty()
        || (!has_field(&stream.schema.fields, "session_id")
            && !has_field(&stream.schema.fields, "user_id"))
    {
        return Ok(());
    }
    let fingerprints = rows
        .iter()
        .map(|row| sql_literal(&row.item.fingerprint))
        .collect::<Vec<_>>()
        .join(", ");
    let session = if has_field(&stream.schema.fields, "session_id") {
        "session_id"
    } else {
        "NULL AS session_id"
    };
    let user = if has_field(&stream.schema.fields, "user_id") {
        "user_id"
    } else {
        "NULL AS user_id"
    };
    let limit = rows.len().saturating_mul(50).clamp(50, 2_000);
    let sql = format!(
        "SELECT fingerprint, {session}, {user} FROM \"{}\" \
         WHERE fingerprint IN ({fingerprints}) AND {timestamp} >= {} AND {timestamp} < {} \
         ORDER BY {timestamp} DESC LIMIT {limit}",
        escape_sql_ident(&stream.name),
        context.from,
        context.to,
    );
    let result = run_log_query(
        state,
        iam,
        "rum_errors",
        TimeRange::new(TimestampMicros(context.from), TimestampMicros(context.to)),
        sql,
        limit,
        PhysicalDatasetKind::Raw,
    )
    .await?;
    let fingerprint = column(&result, "fingerprint");
    let session = column(&result, "session_id");
    let user = column(&result, "user_id");
    for values in result.rows {
        let Some(key) = fingerprint
            .and_then(|index| values.get(index))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let Some(row) = rows.iter_mut().find(|row| row.item.fingerprint == key) else {
            continue;
        };
        if let Some(value) = session
            .and_then(|index| values.get(index))
            .and_then(non_empty_string)
            && row.item.recent_sessions.len() < 10
            && !row.item.recent_sessions.contains(&value)
        {
            row.item.recent_sessions.push(value);
        }
        if let Some(value) = user
            .and_then(|index| values.get(index))
            .and_then(non_empty_string)
            && row.item.recent_users.len() < 50
            && !row.item.recent_users.contains(&value)
        {
            row.item.recent_users.push(value);
        }
    }
    Ok(())
}
