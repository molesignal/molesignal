// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Server-side RUM list projections with stable cursor pagination.

use axum::{Router, routing::get};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{
        query::{QueryLanguage, QueryRequest, QueryResult, StreamHint},
        storage::PhysicalDatasetKind,
        stream::{FieldDef, StreamDefinition, StreamType},
    },
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

mod cursor;
mod errors;
mod sessions;

const DEFAULT_WINDOW_MICROS: i64 = 24 * 60 * 60 * 1_000_000;
const DEFAULT_PAGE_SIZE: usize = 20;
pub(super) const MAX_PAGE_SIZE: usize = 100;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/rum/sessions", get(sessions::list))
        .route("/rum/errors", get(errors::list))
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(super) struct ListQuery {
    pub(super) from: Option<i64>,
    pub(super) to: Option<i64>,
    pub(super) q: Option<String>,
    pub(super) country: Option<String>,
    pub(super) browser: Option<String>,
    pub(super) replay_available: Option<bool>,
    pub(super) status: Option<String>,
    pub(super) limit: Option<usize>,
    pub(super) cursor: Option<String>,
}

pub(super) fn initial_page_context(request: &ListQuery) -> Result<(i64, i64, usize)> {
    let now = TimestampMicros::now().0;
    let from = request
        .from
        .unwrap_or_else(|| now.saturating_sub(DEFAULT_WINDOW_MICROS));
    let to = request.to.unwrap_or(now);
    if to <= from {
        return Err(Error::invalid("RUM range end must be greater than start"));
    }
    Ok((
        from,
        to,
        request
            .limit
            .unwrap_or(DEFAULT_PAGE_SIZE)
            .clamp(1, MAX_PAGE_SIZE),
    ))
}

pub(super) fn shared_cursor_mismatch(
    request: &ListQuery,
    from: i64,
    to: i64,
    page_size: usize,
) -> bool {
    request.from.is_some_and(|value| value != from)
        || request.to.is_some_and(|value| value != to)
        || request
            .limit
            .is_some_and(|value| value.clamp(1, MAX_PAGE_SIZE) != page_size)
}

pub(super) fn normalize_text(value: Option<String>, max: usize) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim();
        (!value.is_empty() && !value.contains('\0')).then(|| value.chars().take(max).collect())
    })
}

pub(super) async fn log_stream(
    state: &AppState,
    iam: &IamContext,
    name: &str,
) -> Result<Option<StreamDefinition>> {
    match state
        .telemetry
        .streams
        .get(&iam.org_id, name, StreamType::Logs)
        .await
    {
        Ok(stream) => Ok(Some(stream)),
        Err(Error::NotFound(_)) => Ok(None),
        Err(error) => Err(error),
    }
}

pub(super) async fn run_log_query(
    state: &AppState,
    iam: &IamContext,
    stream: &str,
    time_range: TimeRange,
    statement: String,
    limit: usize,
    dataset_kind: PhysicalDatasetKind,
) -> Result<QueryResult> {
    state
        .query
        .run_dataset(
            QueryRequest {
                org_id: iam.org_id.clone(),
                language: QueryLanguage::Sql,
                statement,
                time_range,
                stream: Some(StreamHint {
                    name: stream.to_string(),
                    stream_type: StreamType::Logs,
                }),
                limit: Some(limit),
                federation_clusters: Vec::new(),
            },
            dataset_kind,
        )
        .await
}

pub(super) fn has_field(fields: &[FieldDef], name: &str) -> bool {
    fields.iter().any(|field| field.name == name)
}

pub(super) fn column(result: &QueryResult, name: &str) -> Option<usize> {
    result.columns.iter().position(|column| column == name)
}

pub(super) fn value_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
        .or_else(|| value.as_f64().map(|value| value as i64))
}

pub(super) fn value_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_string)
        .or_else(|| (!value.is_null()).then(|| value.to_string()))
}

pub(super) fn non_empty_string(value: &Value) -> Option<String> {
    value_string(value).filter(|value| !value.is_empty())
}

pub(super) fn sql_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}
