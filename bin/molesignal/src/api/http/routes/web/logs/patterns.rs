// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The kinds of message among the log rows a structured query matches, over its whole time range,
//! compared with a baseline stretch.
//!
//! The list returns pages, and grouping only the pages read says little of a busy range. This
//! reads up to a sample of the rows the same query matches, spread across the range, counts every
//! match, and scales each pattern's count to the total; the answer says when it sampled.

use axum::{Extension, Json, extract::State};
use domain::{
    iam::{IamContext, permission},
    stream::StreamType,
};
use kernel::{
    Error, Result,
    time::{TimeRange, TimestampMicros},
};
use serde::Deserialize;

use super::{LogFilter, clean_required, filter_clauses, validate_filter_fields};
use crate::{
    api::AppState,
    app::{
        log_patterns::{
            MAX_LIMIT, PatternQuery, PatternReport, Span, analyze, level_field, message_field,
        },
        query::baseline::Baseline,
    },
};

const DEFAULT_LIMIT: usize = 100;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LogPatternsRequest {
    stream: Option<String>,
    from: Option<i64>,
    to: Option<i64>,
    #[serde(default)]
    filters: Vec<LogFilter>,
    #[serde(default)]
    free_text: Vec<String>,
    #[serde(default, rename = "where")]
    condition: Option<serde_json::Value>,
    /// The field query as text; it stands for `filters`, `free_text` and `where`.
    #[serde(default)]
    expression: Option<String>,
    /// The field that holds the message; found from the stream's schema when left out.
    #[serde(default)]
    field: Option<String>,
    /// `previous`, `week_before` or `none`.
    #[serde(default)]
    baseline: Baseline,
    #[serde(default)]
    limit: Option<usize>,
}

#[permission(any("logs.read", "sys.telemetry.read"))]
pub(super) async fn patterns(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Json(request): Json<LogPatternsRequest>,
) -> Result<Json<PatternReport>> {
    let (Some(from), Some(to)) = (request.from, request.to) else {
        return Err(Error::invalid("pattern range is required"));
    };

    if to <= from {
        return Err(Error::invalid(
            "pattern range end must be greater than start",
        ));
    }

    let stream_name = clean_required(request.stream, "log stream is required", 128)?;
    let (filters, free_text, condition) = super::expression::read_match(
        request.expression,
        request.filters,
        request.free_text,
        request.condition,
    )?;

    let stream = match state
        .telemetry
        .streams
        .get(&iam.org_id, &stream_name, StreamType::LOGS)
        .await
    {
        Ok(stream) => stream,
        // A stream nothing was written to has no patterns: its list is empty too.
        Err(Error::NotFound(_)) => return Ok(Json(empty(stream_name, from, to))),
        Err(error) => return Err(error),
    };

    let fields = &stream.schema.fields;
    validate_filter_fields(&filters, &free_text, condition.as_ref(), fields)?;

    let message = message_field(&stream.schema, &stream_name, request.field.as_deref())?;

    // An encrypted message is stored as ciphertext: its patterns would be of that.
    if fields
        .iter()
        .any(|field| field.name == message && field.encrypted)
    {
        return Err(Error::invalid(format!(
            "field `{message}` is encrypted at rest and has no patterns"
        )));
    }

    let clauses = filter_clauses(&filters, &free_text, condition.as_ref(), fields)?;
    let filter = if clauses.is_empty() {
        String::new()
    } else {
        format!(" AND ({})", clauses.join(") AND ("))
    };

    let level = level_field(&stream.schema, &message);

    // The range is closed at both ends, the list's end is open: one microsecond in.
    let query = PatternQuery {
        org_id: &iam.org_id,
        stream: &stream_name,
        message: &message,
        level: level.as_deref(),
        filter,
        window: TimeRange::new(TimestampMicros(from), TimestampMicros(to - 1)),
        baseline: request.baseline,
        limit: request.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT),
        tokens: true,
    };

    Ok(Json(analyze(&state.query, &query).await?))
}

fn empty(stream: String, from: i64, to: i64) -> PatternReport {
    PatternReport {
        stream,
        field: String::new(),
        level_field: None,
        time_range: Span {
            start_time: from,
            end_time: to,
        },
        logs: 0,
        read: 0,
        unread: 0,
        sampled: false,
        baseline: None,
        new_patterns: None,
        surging_patterns: None,
        dropping_patterns: None,
        pattern_count: 0,
        patterns: Vec::new(),
    }
}
